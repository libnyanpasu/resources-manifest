//! Manifest compilation for the generator.

use manifest_metadata::{ManifestSnapshot, ResourceManifestTemplate};
use serde_json::json;
use wasm_bindgen::prelude::*;

use crate::bridge::{decode, encode};
use crate::error::error;

/// Compiles an authored template with a release snapshot (design §9.6).
///
/// Failures are thrown together as `compile.failed`, keeping core's order and
/// each error's code, path, and fields, like `validateManifest`.
#[wasm_bindgen(js_name = compileManifest, unchecked_return_type = "ResourceManifest")]
pub fn compile_manifest(
    #[wasm_bindgen(unchecked_param_type = "ResourceManifestTemplate")] template: JsValue,
    #[wasm_bindgen(unchecked_param_type = "ManifestSnapshot")] snapshot: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] now: JsValue,
) -> Result<JsValue, JsValue> {
    let template: ResourceManifestTemplate = decode(&template, "arg.template")?;
    let snapshot: ManifestSnapshot = decode(&snapshot, "arg.snapshot")?;
    let now: String = decode(&now, "arg.now")?;
    let manifest = manifest_core::compile(&template, &snapshot, &now).map_err(|source| {
        let errors: Vec<_> = source
            .errors()
            .iter()
            .map(|error| {
                json!({
                    "code": error.code(),
                    "path": error.path(),
                    "message": error.to_string(),
                    "fields": error.fields(),
                })
            })
            .collect();
        error(
            "compile.failed",
            "arg.template",
            &source,
            json!({ "errors": errors }),
        )
    })?;
    encode(&manifest)
}
