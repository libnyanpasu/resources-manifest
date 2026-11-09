// Browser entry: the same bindings as the Node entry. Browsers fetch wasm
// asynchronously, so callers await `initRuntime` before using any binding.
import initWasm from "../wasm/manifest_wasm.js";

export * from "./bindings.ts";

let ready: Promise<void> | undefined;

/** Fetches and instantiates the wasm module once. */
export function initRuntime(): Promise<void> {
  ready ??= initWasm({
    module_or_path: new URL("../wasm/manifest_wasm_bg.wasm", import.meta.url),
  }).then(() => undefined);
  return ready;
}
