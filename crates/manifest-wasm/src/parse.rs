//! Remote version and checksum parsing.

use manifest_parser::{RemoteChecksumParser, RemoteResponse, RemoteVersionParser};
use serde_json::json;
use wasm_bindgen::prelude::*;

use crate::bridge::{decode, encode};
use crate::error::{core_error, error};
use crate::handles::{DownloadPlan, VersionPlan};
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

/// Completes the remote version request bound to a version plan: one response
/// per part, in the order of `plan.value.remote.parts`.
#[wasm_bindgen(js_name = finishVersion, unchecked_return_type = "string")]
pub fn finish_version(
    plan: &VersionPlan,
    #[wasm_bindgen(unchecked_param_type = "RemoteResponseInput[]")] responses: JsValue,
) -> Result<JsValue, JsValue> {
    let remote = plan.inner.remote().ok_or_else(|| {
        error(
            "wasm.version_plan_missing",
            "arg.plan",
            "version plan has no remote version request",
            json!({}),
        )
    })?;
    let responses: Vec<RemoteResponseInput> = decode(&responses, "arg.responses")?;
    let responses: Vec<_> = responses
        .iter()
        .map(|response| RemoteResponse {
            body: &response.body,
            headers: response.headers(),
        })
        .collect();
    let version = manifest_core::finish_version(remote, &responses).map_err(core_error)?;
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
