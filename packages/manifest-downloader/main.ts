#!/usr/bin/env node
import { randomUUID } from "node:crypto";
import { mkdir, readFile, rename, rm } from "node:fs/promises";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import type { ChecksumAlgorithm, SupportedPlatform } from "@manifest/metadata";
import {
  checkPolicy,
  compareDigest,
  finishChecksum,
  finishVersion,
  ManifestRuntimeError,
  planDownload,
  planVersion,
  selectVariant,
  validateManifest,
  type ChecksumKind,
  type VerifyPolicy,
} from "@manifest/runtime";
import { downloadArtifact, hostError, readResponse } from "./http.ts";

export interface RunDependencies {
  log?: (line: string) => void;
  timeoutMs?: number;
}

export const DEFAULT_MAX_ARTIFACT_BYTES = 1024 * 1024 * 1024;

function reportPolicy(
  resource: string,
  variant: string,
  version: string,
  kind: ChecksumKind,
  policy: VerifyPolicy,
  log: (line: string) => void,
): void {
  const summary = `resource=${resource} variant=${variant} version=${version} kind=${kind}`;
  try {
    checkPolicy(kind, policy);
  } catch (error) {
    if (!(error instanceof ManifestRuntimeError)) throw error;
    log(`${summary} policy=reject reason=${error.code}: ${error.message}`);
    const hint =
      kind === "parsed"
        ? "; pass --allow-parsed to accept parsed checksums"
        : kind === "snapshot"
          ? "; pass --allow-snapshot to accept snapshot checksums"
          : "";
    throw new ManifestRuntimeError({
      code: error.code,
      path: error.path,
      message: error.message + hint,
      fields: error.fields,
    });
  }
  log(`${summary} policy=accept reason=accepted by host VerifyPolicy`);
  if (kind === "parsed" || kind === "snapshot") {
    log(`WARN ${kind} checksum accepted because --allow-${kind} was set`);
  }
}

export async function run(
  argv: string[],
  deps: RunDependencies = {},
): Promise<string> {
  const { values } = parseArgs({
    args: argv,
    options: {
      manifest: { type: "string" },
      resource: { type: "string" },
      platform: { type: "string" },
      variant: { type: "string" },
      tags: { type: "string" },
      mirror: { type: "string" },
      "allow-parsed": { type: "boolean", default: false },
      "allow-snapshot": { type: "boolean", default: false },
      out: { type: "string" },
      "max-artifact-bytes": { type: "string" },
    },
  });
  for (const option of ["manifest", "resource", "platform", "out"] as const) {
    if (!values[option]) {
      throw hostError(
        "cli.invalid_args",
        `--${option} is required`,
        "arg.argv",
      );
    }
  }
  if (values.variant !== undefined && values.tags !== undefined) {
    throw hostError(
      "cli.invalid_args",
      "--variant and --tags are mutually exclusive",
      "arg.argv",
    );
  }
  const maxBytes =
    values["max-artifact-bytes"] === undefined
      ? DEFAULT_MAX_ARTIFACT_BYTES
      : Number(values["max-artifact-bytes"]);
  if (!Number.isSafeInteger(maxBytes) || maxBytes <= 0) {
    throw hostError(
      "cli.invalid_args",
      "--max-artifact-bytes must be a positive safe integer",
      "arg.argv",
    );
  }
  const log = deps.log ?? console.log;
  const policy: VerifyPolicy = {
    allow_parsed: values["allow-parsed"],
    allow_snapshot: values["allow-snapshot"],
  };
  const manifestPath = values.manifest!;
  const text = /^https?:\/\//i.test(manifestPath)
    ? (await readResponse(manifestPath, undefined, deps.timeoutMs)).body
    : await readFile(manifestPath, "utf8");
  using manifest = validateManifest(JSON.parse(text));
  using selected = selectVariant(
    manifest,
    values.resource!,
    values.platform as SupportedPlatform,
    { id: values.variant, tags: values.tags?.split(",") },
  );
  using versionHandle = planVersion(selected, { mirror: values.mirror });
  const versionPlan = versionHandle.value;
  const snapshot =
    !!versionPlan.remote && policy.allow_snapshot && !policy.allow_parsed;
  if (versionPlan.remote) {
    reportPolicy(
      values.resource!,
      selected.value.id,
      "unresolved",
      snapshot ? "snapshot" : versionPlan.kind,
      policy,
      log,
    );
  }
  let version = versionPlan.latest!;
  if (versionPlan.remote) {
    const responses = [];
    for (const part of versionPlan.remote.parts) {
      responses.push(
        await readResponse(
          part.request.mirror_url ?? part.request.canonical_url,
          part.parser,
          deps.timeoutMs,
          "http.version",
        ),
      );
    }
    version = finishVersion(versionHandle, responses);
  }
  using planHandle = planDownload(selected, {
    version,
    mirror: values.mirror,
    checksum_mode: snapshot ? "snapshot" : "remote",
  });
  const plan = planHandle.value;
  const filename = plan.artifact.slice(plan.artifact.lastIndexOf("/") + 1);
  if (
    !filename ||
    filename === "." ||
    filename === ".." ||
    /[<>:"/\\|?*]|\p{Cc}|[. ]$/u.test(filename) ||
    /^(?:con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)/i.test(filename)
  ) {
    throw hostError(
      "download.invalid_filename",
      "artifact has no safe output filename",
      "artifact",
    );
  }
  reportPolicy(
    values.resource!,
    selected.value.id,
    plan.version,
    plan.request.kind,
    policy,
    log,
  );
  const checksum = plan.checksum_request
    ? finishChecksum(
        planHandle,
        await readResponse(
          plan.checksum_request.request.mirror_url ??
            plan.checksum_request.request.canonical_url,
          plan.checksum_request.parser,
          deps.timeoutMs,
          "http.checksum",
        ),
      )
    : plan.checksum;
  if (!checksum) {
    throw hostError(
      "verify.kind_rejected",
      "download plan has no checksum",
      "arg.kind",
    );
  }
  checkPolicy(checksum.kind, policy);
  const separator = checksum.checksum.indexOf(":");
  const algorithm = checksum.checksum.slice(0, separator) as ChecksumAlgorithm;
  const expected = checksum.checksum.slice(separator + 1);
  const out = resolve(values.out!);
  await mkdir(out, { recursive: true });
  const temporary = join(out, `.manifest-download-${randomUUID()}.part`);
  const destination = join(out, filename);
  try {
    const actual = await downloadArtifact(
      plan.request.mirror_url ?? plan.request.canonical_url,
      temporary,
      algorithm,
      maxBytes,
      deps.timeoutMs,
    );
    compareDigest(algorithm, expected, actual);
    await rename(temporary, destination);
  } finally {
    await rm(temporary, { force: true });
  }
  return destination;
}

export async function main(argv = process.argv.slice(2)): Promise<void> {
  try {
    await run(argv);
  } catch (error) {
    const code =
      error instanceof ManifestRuntimeError
        ? error.code
        : error instanceof SyntaxError
          ? "manifest.invalid_json"
          : "download.failed";
    const path =
      error instanceof ManifestRuntimeError && error.path
        ? ` [${error.path}]`
        : "";
    const message = error instanceof Error ? error.message : String(error);
    console.error(`error: ${code}: ${message.replace(/[\r\n]+/g, " ")}${path}`);
    process.exitCode = 1;
  }
}

if (import.meta.main) {
  await main();
}
