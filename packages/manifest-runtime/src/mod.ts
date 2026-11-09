import { readFileSync } from "node:fs";
import * as wasm from "../wasm/manifest_wasm.js";
import type { RuntimeErrorShape } from "./types.ts";

export * from "./types.ts";
export type {
  DownloadPlan as DownloadPlanValue,
  VersionPlan as VersionPlanValue,
} from "./types.ts";
// The handle classes are exported as types only: they have no public
// constructor and are created by the functions below.
export type {
  DownloadPlan,
  SelectedArtifact,
  ValidatedManifest,
  VersionPlan,
} from "../wasm/manifest_wasm.js";

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

const handleClasses = [
  wasm.DownloadPlan,
  wasm.SelectedArtifact,
  wasm.ValidatedManifest,
  wasm.VersionPlan,
];
const handles = new WeakMap<object, object>();

/**
 * Wraps a handle so its getters and methods translate structured errors too.
 */
function wrap<Value>(value: Value): Value {
  if (
    typeof value !== "object" ||
    value === null ||
    !handleClasses.some((handleClass) => value instanceof handleClass)
  ) {
    return value;
  }
  const raw = value;
  const wrapped = new Proxy(raw, {
    get(target, key) {
      return call(() => {
        const property: unknown = Reflect.get(target, key, target);
        return typeof property === "function"
          ? (...args: unknown[]) =>
              call(() => Reflect.apply(property, target, args))
          : property;
      });
    },
  });
  handles.set(wrapped, raw);
  return wrapped;
}

/** Hands wasm the original handle behind a wrapper. */
function unwrap(value: unknown): unknown {
  return typeof value === "object" && value !== null
    ? (handles.get(value) ?? value)
    : value;
}

/**
 * Exposes a wasm export with its generated signature, translating structured
 * errors and wrapping returned handles.
 */
function bind<Args extends unknown[], Result>(
  operation: (...args: Args) => Result,
): (...args: Args) => Result {
  return (...args) =>
    wrap(call(() => operation(...(args.map(unwrap) as Args))));
}

export const validateManifest = bind(wasm.validateManifest);
export const artifactChecksumKind = bind(wasm.artifactChecksumKind);
export const selectVariant = bind(wasm.selectVariant);
export const listVariantChecksumKinds = bind(wasm.listVariantChecksumKinds);
export const planVersion = bind(wasm.planVersion);
export const planDownload = bind(wasm.planDownload);
export const renderArtifact = bind(wasm.renderArtifact);
export const renderTemplate = bind(wasm.renderTemplate);
export const parseVersion = bind(wasm.parseVersion);
export const finishVersion = bind(wasm.finishVersion);
export const parseChecksum = bind(wasm.parseChecksum);
export const finishChecksum = bind(wasm.finishChecksum);
export const checkPolicy = bind(wasm.checkPolicy);
export const compareDigest = bind(wasm.compareDigest);
