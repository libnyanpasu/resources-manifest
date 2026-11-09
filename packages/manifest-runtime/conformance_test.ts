import assert from "node:assert/strict";
import { appendFileSync, readFileSync, readdirSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import type {
  ChecksumAlgorithm,
  RemoteChecksumParser,
  RemoteVersionParser,
  ResourceArtifactTemplate,
  ResourceManifest,
  SupportedPlatform,
} from "@manifest/metadata";
import {
  ManifestRuntimeError,
  artifactChecksumKind,
  checkPolicy,
  compareDigest,
  finishChecksum,
  finishVersion,
  listVariantChecksumKinds,
  parseChecksum,
  parseVersion,
  planDownload,
  planVersion,
  renderArtifact,
  renderTemplate,
  selectVariant,
  validateManifest,
  type ChecksumKind,
  type DownloadOptions,
  type LimitsInput,
  type RemoteResponseInput,
  type RuntimeErrorShape,
  type TemplateValue,
  type VerifyPolicy,
} from "@manifest/runtime";

type StableError = Pick<RuntimeErrorShape<unknown>, "code" | "path" | "fields">;
type Expectation =
  { ok: unknown } | { error: StableError; additional_errors?: StableError[] };
interface Case {
  name: string;
  input: unknown;
  expect: Expectation;
}
interface ParserInput<Parser> {
  parser: Parser;
  response: RemoteResponseInput;
  artifact: string;
  limits?: LimitsInput;
}
interface PlanInput extends DownloadOptions {
  operation: string;
  manifest: unknown;
  resource: string;
  platform: SupportedPlatform;
  variant_id?: string;
  tags?: string[];
  response?: RemoteResponseInput;
  /** Version responses, one per plan part; `response` is a single-part shorthand. */
  responses?: RemoteResponseInput[];
  limits?: LimitsInput;
}
interface ArtifactInput extends PlanInput {
  variant: string;
}
interface TemplateInput {
  template: string;
  values: Record<string, TemplateValue>;
  path: string;
  limits?: LimitsInput;
}
interface PolicyInput {
  operation: "policy" | "digest_mismatch";
  kind: ChecksumKind;
  policy: VerifyPolicy;
  algorithm: ChecksumAlgorithm;
  expected: string;
  actual: string;
}

const root = fileURLToPath(new URL("../../conformance/", import.meta.url));
const snapshot: ResourceManifest = JSON.parse(
  readFileSync(
    new URL(
      "../../crates/manifest-metadata/tests/fixtures/resources.json",
      import.meta.url,
    ),
    "utf8",
  ),
);
const names = new Set<string>();

function json(path: string): unknown {
  return JSON.parse(readFileSync(path, "utf8"));
}

function stableError(error: unknown): StableError {
  assert.ok(error instanceof ManifestRuntimeError);
  return { code: error.code, path: error.path, fields: error.fields };
}

function result(operation: () => unknown): Expectation {
  try {
    return { ok: operation() };
  } catch (error) {
    return { error: stableError(error) };
  }
}

function sorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sorted);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
        .map(([key, entry]) => [key, sorted(entry)]),
    );
  }
  return value;
}

function errorMultiset(errors: StableError[]): string[] {
  return errors.map((error) => JSON.stringify(sorted(error))).sort();
}

function validation(input: unknown, expect: Expectation): void {
  const execute = (): Expectation =>
    result(() => {
      const manifest = validateManifest(input);
      try {
        assert.deepEqual(manifest.value, input);
        assert.equal(
          manifest.newerMinor,
          Number(manifest.value.manifest_version.split(".")[1]) > 0,
        );
        return true;
      } finally {
        manifest.free();
      }
    });
  const actual = execute();
  assert.deepEqual(actual, execute());
  if ("ok" in expect) {
    assert.deepEqual(actual, expect);
    return;
  }
  assert.ok("error" in actual);
  assert.equal(actual.error.code, "validate.failed");
  assert.equal(actual.error.path, "arg.manifest");
  const errors = (actual.error.fields as { errors: StableError[] }).errors.map(
    ({ code, path, fields }) => ({ code, path, fields }),
  );
  assert.deepEqual(
    errorMultiset(errors),
    errorMultiset([expect.error, ...(expect.additional_errors ?? [])]),
  );
}

function normalizeParser<Parser extends { type: string }>(
  parser: Parser,
): Parser {
  return parser.type.startsWith("future-")
    ? ({ type: parser.type } as Parser)
    : parser;
}

function compat(input: ResourceManifest): unknown {
  const manifest = validateManifest(input);
  try {
    const value = manifest.value;
    const resource = value.resources.sample;
    const variant = resource.variants[0];
    const artifacts = Object.fromEntries(
      Object.entries(variant.artifacts).map(([platform, artifact]) => {
        const kind = artifactChecksumKind(artifact);
        const selected = selectVariant(
          manifest,
          "sample",
          platform as SupportedPlatform,
          { id: variant.id },
        );
        try {
          const planned = result(() => {
            const plan = planVersion(selected);
            plan.free();
            return true;
          });
          if ("ok" in planned) return [platform, { ok: true, kind }];
          assert.equal(kind, "none");
          for (const allow_parsed of [false, true]) {
            for (const allow_snapshot of [false, true]) {
              assert.throws(() =>
                checkPolicy(kind, { allow_parsed, allow_snapshot }),
              );
            }
          }
          const { code, fields } = planned.error;
          return [platform, { error: { code, fields }, kind }];
        } finally {
          selected.free();
        }
      }),
    );
    const normalized: ResourceManifest = structuredClone(input);
    const normalizedResource = normalized.resources.sample;
    delete (normalizedResource.default_variants as Record<string, string>)[
      "future-os-cpu"
    ];
    for (const variant of normalizedResource.variants) {
      delete (variant.artifacts as Record<string, ResourceArtifactTemplate>)[
        "future-os-cpu"
      ];
      for (const artifact of Object.values(variant.artifacts)) {
        if (artifact.version.type === "remote") {
          const remote = artifact.version;
          remote.version.parser = normalizeParser(remote.version.parser);
          if (remote.checksum) {
            remote.checksum.parser = normalizeParser(remote.checksum.parser);
          }
        }
      }
    }
    assert.deepEqual(value, normalized);
    return {
      newer_minor: manifest.newerMinor,
      default_variants: resource.default_variants,
      artifacts,
    };
  } finally {
    manifest.free();
  }
}

function kind(input: unknown): unknown {
  const policy = input as PolicyInput;
  if (policy.operation === undefined) {
    return artifactChecksumKind(input as ResourceArtifactTemplate);
  }
  if (policy.operation === "policy") {
    checkPolicy(policy.kind, policy.policy);
    return true;
  }
  if (policy.operation === "digest_mismatch") {
    compareDigest(policy.algorithm, policy.expected, policy.actual);
    return true;
  }
  const options = input as PlanInput;
  const manifest = validateManifest(options.manifest);
  try {
    if (options.operation === "list") {
      return listVariantChecksumKinds(
        manifest,
        options.resource,
        options.platform,
      );
    }
    const selected = selectVariant(
      manifest,
      options.resource,
      options.platform,
      { id: options.variant_id, tags: options.tags },
    );
    try {
      if (options.operation === "select") {
        const variant = selected.value;
        return {
          variant_id: variant.id,
          kind: artifactChecksumKind(variant.artifacts[options.platform]!),
        };
      }
      if (options.operation === "version") {
        const plan = planVersion(selected, options, options.limits);
        try {
          const value = plan.value;
          const responses =
            options.responses ?? (options.response && [options.response]);
          if (!responses) return value;
          return { plan: value, version: finishVersion(plan, responses) };
        } finally {
          plan.free();
        }
      }
      assert.ok(
        ["download", "finish_checksum"].includes(options.operation),
        `unknown kind operation: ${options.operation}`,
      );
      const plan = planDownload(selected, options, options.limits);
      try {
        if (options.operation === "download") return plan.value;
        assert.ok(options.response);
        const checksum = finishChecksum(plan, options.response);
        assert.equal(checksum.kind, plan.value.request.kind);
        return checksum;
      } finally {
        plan.free();
      }
    } finally {
      selected.free();
    }
  } finally {
    manifest.free();
  }
}

function manifestInput(input: unknown): unknown {
  if (input !== null && typeof input === "object" && "snapshot" in input) {
    assert.deepEqual(input, { snapshot: "resources.json" });
    return snapshot;
  }
  return input;
}

function resolve(
  input: ArtifactInput | TemplateInput,
  golden: boolean,
): unknown {
  if ("template" in input) {
    assert.ok(!golden);
    return renderTemplate(
      input.template,
      input.values,
      input.path,
      input.limits,
    );
  }
  const manifest = validateManifest(manifestInput(input.manifest));
  try {
    const selected = selectVariant(
      manifest,
      input.resource,
      input.platform,
      golden ? undefined : { id: input.variant },
    );
    try {
      if (!golden) return renderArtifact(selected, input, input.limits);
      assert.equal(selected.value.id, input.variant);
      const version = selected.value.artifacts[input.platform]!.version;
      assert.equal(
        version.type === "static" ? version.latest : version.fallback?.version,
        input.version,
      );
      const plan = planDownload(selected, {
        version: version.type === "remote" ? input.version : undefined,
      });
      try {
        const value = plan.value;
        assert.equal(value.version, input.version);
        assert.equal(value.request.mirror_url, null);
        return {
          artifact: value.artifact,
          canonical_url: value.request.canonical_url,
          download_url: value.request.mirror_url ?? value.request.canonical_url,
        };
      } finally {
        plan.free();
      }
    } finally {
      selected.free();
    }
  } finally {
    manifest.free();
  }
}

function register(name: string, execute: () => void): void {
  assert.equal(typeof name, "string");
  assert.ok(name.length > 0);
  assert.ok(!names.has(name), `duplicate case name: ${name}`);
  names.add(name);
  test(name, () => {
    execute();
    if (process.env.MANIFEST_CONFORMANCE_LIST) {
      appendFileSync(
        process.env.MANIFEST_CONFORMANCE_LIST,
        `${JSON.stringify(name)}\n`,
      );
    }
  });
}

function walk(directory: string): void {
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort(
    (a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0),
  )) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      walk(path);
      continue;
    }
    if (!entry.name.endsWith(".json")) continue;
    const file = relative(root, path).split(sep).join("/");
    const cases = json(path);
    assert.ok(Array.isArray(cases) && cases.length > 0, file);
    if (file.startsWith("fixtures/")) {
      assert.ok(
        /^fixtures\/github-expanded-assets\/[^/]+\.expected\.json$/.test(file),
        `unknown fixture format: ${file}`,
      );
      const stem = entry.name.slice(0, -".expected.json".length);
      const body = readFileSync(join(directory, `${stem}.html`), "utf8");
      for (const asset of cases as {
        name: string;
        checksum: string | null;
      }[]) {
        register(
          `fixtures.github-expanded-assets.${stem}.${asset.name}`,
          () => {
            const actual = result(() =>
              parseChecksum(
                { type: "github-release-assets" },
                { body, headers: [] },
                asset.name,
              ),
            );
            assert.deepEqual(
              actual,
              asset.checksum === null
                ? {
                    error: {
                      code: "parser.digest_missing",
                      path: "response.body",
                      fields: { name: asset.name },
                    },
                  }
                : { ok: asset.checksum },
            );
          },
        );
      }
      continue;
    }
    const group = file.split("/")[0];
    assert.ok(
      ["parsers", "validate", "compat", "kind", "resolve"].includes(group),
      `unknown corpus directory: ${file}`,
    );
    if (group === "parsers") {
      assert.ok(
        ["parsers/version.json", "parsers/checksum.json"].includes(file),
        `unknown parser corpus: ${file}`,
      );
    }
    if (file === "resolve/reference-golden.json") {
      const covered = (cases as Case[]).map(({ input }) => {
        const { resource, platform } = input as ArtifactInput;
        return `${resource}/${platform}`;
      });
      const expected = new Set(
        Object.entries(snapshot.resources).flatMap(([id, resource]) =>
          resource.variants.flatMap((variant) =>
            Object.keys(variant.artifacts).map(
              (platform) => `${id}/${platform}`,
            ),
          ),
        ),
      );
      assert.equal(covered.length, new Set(covered).size);
      assert.deepEqual(new Set(covered), expected);
    }
    for (const item of cases as Case[]) {
      register(item.name, () => {
        if (group === "validate") {
          validation(item.input, item.expect);
          return;
        }
        const actual = result(() => {
          switch (group) {
            case "parsers": {
              if (file === "parsers/version.json") {
                const input = item.input as ParserInput<RemoteVersionParser>;
                return parseVersion(input.parser, input.response, input.limits);
              }
              const input = item.input as ParserInput<RemoteChecksumParser>;
              return parseChecksum(
                input.parser,
                input.response,
                input.artifact,
                input.limits,
              );
            }
            case "compat":
              return compat(item.input as ResourceManifest);
            case "kind":
              return kind(item.input);
            case "resolve":
              return resolve(
                item.input as ArtifactInput | TemplateInput,
                file === "resolve/reference-golden.json",
              );
            default:
              assert.fail(`unknown corpus: ${file}`);
          }
        });
        assert.deepEqual(actual, item.expect);
      });
    }
  }
}

walk(root);
assert.ok(names.size > 0, "conformance runner must execute cases");
