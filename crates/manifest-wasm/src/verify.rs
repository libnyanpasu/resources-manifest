//! Checksum policy and digest comparison.

use manifest_core::{VerifyError, VerifyPolicy};
use wasm_bindgen::prelude::*;

use crate::bridge::{Fields, decode};
use crate::error::core_error;

/// Accepts or rejects a checksum kind under the host policy. The default
/// policy accepts only `authoritative`; no policy accepts `none`. The policy
/// must be an object, so a positional array cannot switch flags on.
#[wasm_bindgen(js_name = checkPolicy)]
pub fn check_policy(
    #[wasm_bindgen(unchecked_param_type = "ChecksumKind")] kind: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "VerifyPolicy | null")] policy: JsValue,
) -> Result<(), JsValue> {
    let kind = decode(&kind, "arg.kind")?;
    let policy: VerifyPolicy = Fields::optional(&policy, "arg.policy")?.decode()?;
    manifest_core::check_policy(kind, &policy).map_err(core_error)
}

/// Compares the expected digest with a host-computed one by exact string
/// equality.
#[wasm_bindgen(js_name = compareDigest)]
pub fn compare_digest(
    #[wasm_bindgen(unchecked_param_type = "ChecksumAlgorithm")] algorithm: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] expected: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] actual: JsValue,
) -> Result<(), JsValue> {
    let algorithm = decode(&algorithm, "arg.algorithm")?;
    let expected: String = decode(&expected, "arg.expected")?;
    let actual: String = decode(&actual, "arg.actual")?;
    if expected == actual {
        return Ok(());
    }
    Err(core_error(VerifyError::CompareDigest {
        algorithm,
        expected,
        actual,
    }))
}
