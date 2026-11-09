//! Structured errors thrown across the boundary.
//!
//! Every error is a plain JS object `{ code, path, message, fields }` (design
//! §8.1). Core and parser errors keep their own code, path, and fields; the
//! boundary adds only `wasm.*` codes. `message` is for display and is not stable.

use std::fmt::Display;

use js_sys::JSON;
use serde_json::{Value, json};
use wasm_bindgen::JsValue;

/// Builds a structured error object.
pub(crate) fn error(code: &str, path: &str, message: impl Display, fields: Value) -> JsValue {
    let error = json!({
        "code": code,
        "path": path,
        "message": message.to_string(),
        "fields": fields,
    });
    JSON::parse(&error.to_string()).expect("serde_json emits valid JSON")
}

/// A JS argument or field that cannot be decoded into its Rust type.
pub(crate) fn invalid_input(path: &str, message: impl Display) -> JsValue {
    error("wasm.invalid_input", path, message, json!({}))
}

/// A result that cannot be encoded as JSON.
pub(crate) fn serialization_failed(message: impl Display) -> JsValue {
    error(
        "wasm.serialization_failed",
        "arg.result",
        message,
        json!({}),
    )
}

/// Errors from core and parser that already carry a stable code, path, and
/// fields.
pub(crate) trait StructuredError: Display {
    fn to_js(&self) -> JsValue;
}

macro_rules! structured_error {
    ($($error:ty),+ $(,)?) => {$(
        impl StructuredError for $error {
            fn to_js(&self) -> JsValue {
                error(self.code(), &self.path(), self, self.fields())
            }
        }
    )+};
}

structured_error!(
    manifest_core::FinishVersionError,
    manifest_core::RenderError,
    manifest_core::ResolveError,
    manifest_core::ValidationError,
    manifest_core::VerifyError,
    manifest_parser::ParseError,
);

/// Converts a core or parser error, for use with `map_err`.
pub(crate) fn core_error(source: impl StructuredError) -> JsValue {
    source.to_js()
}
