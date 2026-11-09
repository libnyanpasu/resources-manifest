// Node entry: initializes wasm synchronously on import.
import { readFileSync } from "node:fs";
import { initSync } from "../wasm/manifest_wasm.js";

export * from "./bindings.ts";

initSync({
  module: readFileSync(
    new URL("../wasm/manifest_wasm_bg.wasm", import.meta.url),
  ),
});
