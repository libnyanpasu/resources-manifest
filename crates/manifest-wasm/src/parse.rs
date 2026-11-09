//! Remote version and checksum parsing.

use manifest_parser::{RemoteChecksumParser, RemoteResponse, RemoteVersionParser};
use serde_json::json;
use wasm_bindgen::prelude::*;

use crate::bridge::{decode, encode};
use crate::error::{core_error, error};
use crate::handles::DownloadPlan;
use crate::wire::{LimitsInput, RemoteResponseInput};

/// Compiles a version parser and runs it on a response. Returns only the
/// validated version label; it carries no checksum trust.
#[wasm_bindgen(js_name = parseVersion, unchecked_return_type = "string")]
pub fn parse_version(
    #[wasm_bindgen(unchecked_param_type = "RemoteVersionParser")] parser: JsValue,
    #[wasm_bindgen(unchecked_param_type = "RemoteResponseInput")] response: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "LimitsInput | null")] limits: JsValue,
) -> Result<JsValue, JsValue> {
    let parser: RemoteVersionParser = decode(&parser, "arg.parser")?;
    let response = RemoteResponseInput::decode(&response)?;
    let version = parser
        .compile(&LimitsInput::decode(&limits)?)
        .and_then(|parser| {
            parser.parse(&RemoteResponse {
                body: &response.body,
                headers: response.headers(),
            })
        })
        .map_err(core_error)?;
    encode(&version.as_ref())
}

/// Runs a checksum parser on a response for one artifact name.
#[wasm_bindgen(js_name = parseChecksum, unchecked_return_type = "ResourceChecksum")]
pub fn parse_checksum(
    #[wasm_bindgen(unchecked_param_type = "RemoteChecksumParser")] parser: JsValue,
    #[wasm_bindgen(unchecked_param_type = "RemoteResponseInput")] response: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] artifact: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "LimitsInput | null")] limits: JsValue,
) -> Result<JsValue, JsValue> {
    let parser: RemoteChecksumParser = decode(&parser, "arg.parser")?;
    let artifact: String = decode(&artifact, "arg.artifact")?;
    let response = RemoteResponseInput::decode(&response)?;
    let checksum = parser
        .parse(
            &RemoteResponse {
                body: &response.body,
                headers: response.headers(),
            },
            &artifact,
            &LimitsInput::decode(&limits)?,
        )
        .map_err(core_error)?;
    encode(&checksum)
}

/// Completes the remote checksum request bound to a download plan. The parser,
/// artifact name, and limits come from the plan, never from JS.
#[wasm_bindgen(
    js_name = finishChecksum,
    unchecked_return_type = "ResolvedChecksum & { kind: \"parsed\" }"
)]
pub fn finish_checksum(
    plan: &DownloadPlan,
    #[wasm_bindgen(unchecked_param_type = "RemoteResponseInput")] response: JsValue,
) -> Result<JsValue, JsValue> {
    let checksum_plan = plan.inner.checksum_request().ok_or_else(|| {
        error(
            "wasm.checksum_plan_missing",
            "arg.plan",
            "download plan has no remote checksum request",
            json!({}),
        )
    })?;
    let response = RemoteResponseInput::decode(&response)?;
    let resolved = manifest_core::finish_checksum(
        checksum_plan,
        &RemoteResponse {
            body: &response.body,
            headers: response.headers(),
        },
    )
    .map_err(core_error)?;
    encode(&resolved)
}
