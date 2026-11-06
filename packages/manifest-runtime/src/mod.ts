import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import type {
  RemoteVersionParser,
  ResourceArtifactTemplate,
  ResourceVariant,
  SupportedPlatform,
} from "@manifest/metadata";
import type {
  ChecksumKind,
  DownloadOptions,
  DownloadPlan as DownloadPlanValue,
  ResolvedChecksum,
  RuntimeErrorShape,
  VariantSelection,
  VerifyPolicy,
  VersionOptions,
  VersionPlan as VersionPlanValue,
} from "./types.ts";

export * from "./types.ts";
export type {
  DownloadPlan as DownloadPlanValue,
  VersionPlan as VersionPlanValue,
} from "./types.ts";

declare const handle: unique symbol;

interface WasmHandle<Name extends string> {
  readonly [handle]: Name;
  free(): void;
  [Symbol.dispose](): void;
}

export interface ValidatedManifest extends WasmHandle<"ValidatedManifest"> {
  readonly newerMinor: boolean;
}

export interface SelectedArtifact extends WasmHandle<"SelectedArtifact"> {
  readonly value: ResourceVariant;
}

export interface VersionPlan extends WasmHandle<"VersionPlan"> {
  readonly value: VersionPlanValue;
}

export interface DownloadPlan extends WasmHandle<"DownloadPlan"> {
  readonly value: DownloadPlanValue;
}

export interface LimitsInput {
  body_bytes?: number;
  header_value_bytes?: number;
  regex_pattern_bytes?: number;
  regex_size_bytes?: number;
  template_depth?: number;
  render_output_bytes?: number;
}

export interface RemoteResponseInput {
  body: string;
  headers?: [string, string][];
}

interface WasmExports {
  initSync(input: { module: Uint8Array }): unknown;
  validateManifest: typeof validateManifest;
  artifactChecksumKind: typeof artifactChecksumKind;
  selectVariant: typeof selectVariant;
  planVersion: typeof planVersion;
  planDownload: typeof planDownload;
  parseVersion: typeof parseVersion;
  finishChecksum: typeof finishChecksum;
  checkPolicy: typeof checkPolicy;
}

const wasm = createRequire(import.meta.url)(
  "../wasm/manifest_wasm.js",
) as WasmExports;
wasm.initSync({
  module: readFileSync(
    new URL("../wasm/manifest_wasm_bg.wasm", import.meta.url),
  ),
});

type ErrorShape = RuntimeErrorShape<Record<string, unknown>>;

export class ManifestRuntimeError extends Error {
  readonly code: string;
  readonly path: string;
  readonly fields: Record<string, unknown>;

  constructor(error: ErrorShape) {
    super(error.message);
    this.name = "ManifestRuntimeError";
    this.code = error.code;
    this.path = error.path;
    this.fields = error.fields;
  }
}

function isErrorShape(error: unknown): error is ErrorShape {
  return (
    typeof error === "object" &&
    error !== null &&
    !(error instanceof Error) &&
    "code" in error &&
    typeof error.code === "string" &&
    "path" in error &&
    typeof error.path === "string" &&
    "message" in error &&
    typeof error.message === "string" &&
    "fields" in error &&
    typeof error.fields === "object" &&
    error.fields !== null &&
    !Array.isArray(error.fields)
  );
}

function call<Result>(operation: () => Result): Result {
  try {
    return operation();
  } catch (error) {
    if (isErrorShape(error)) {
      throw new ManifestRuntimeError(error);
    }
    throw error;
  }
}

export function validateManifest(raw: unknown): ValidatedManifest {
  return call(() => wasm.validateManifest(raw));
}

export function artifactChecksumKind(
  artifact: ResourceArtifactTemplate,
): ChecksumKind {
  return call(() => wasm.artifactChecksumKind(artifact));
}

export function selectVariant(
  manifest: ValidatedManifest,
  resource: string,
  platform: SupportedPlatform,
  selection?: VariantSelection | null,
): SelectedArtifact {
  return call(() =>
    wasm.selectVariant(manifest, resource, platform, selection),
  );
}

export function planVersion(
  selected: SelectedArtifact,
  options?: VersionOptions | null,
  limits?: LimitsInput | null,
): VersionPlan {
  return call(() => wasm.planVersion(selected, options, limits));
}

export function planDownload(
  selected: SelectedArtifact,
  options?: DownloadOptions | null,
  limits?: LimitsInput | null,
): DownloadPlan {
  return call(() => wasm.planDownload(selected, options, limits));
}

export function parseVersion(
  parser: RemoteVersionParser,
  response: RemoteResponseInput,
  limits?: LimitsInput | null,
): string {
  return call(() => wasm.parseVersion(parser, response, limits));
}

export function finishChecksum(
  plan: DownloadPlan,
  response: RemoteResponseInput,
): ResolvedChecksum & { kind: "parsed" } {
  return call(() => wasm.finishChecksum(plan, response));
}

export function checkPolicy(
  kind: ChecksumKind,
  policy?: VerifyPolicy | null,
): void {
  return call(() => wasm.checkPolicy(kind, policy));
}
