//! Version and download planning and template rendering.

use std::collections::BTreeMap;

use manifest_core::{ArtifactRenderOptions, ChecksumMode, DownloadOptions, VersionOptions};
use manifest_metadata::SourceVariables;
use wasm_bindgen::prelude::*;

use crate::bridge::{Fields, decode, encode};
use crate::error::core_error;
use crate::handles::{DownloadPlan, SelectedArtifact, VersionPlan};
use crate::wire::{LimitsInput, RenderedArtifact, TemplateValue};

/// Plans how to resolve the version of a selected artifact.
#[wasm_bindgen(js_name = planVersion)]
pub fn plan_version(
    selected: &SelectedArtifact,
    #[wasm_bindgen(unchecked_optional_param_type = "VersionOptions | null")] options: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "LimitsInput | null")] limits: JsValue,
) -> Result<VersionPlan, JsValue> {
    let mut options = Fields::optional(&options, "arg.options")?;
    let source: Option<SourceVariables> = options.take("source")?;
    let mirror: Option<String> = options.take("mirror")?;
    let inner = manifest_core::plan_version(
        &selected.selected(),
        VersionOptions {
            source: source.as_ref(),
            mirror: mirror.as_deref(),
        },
        &LimitsInput::decode(&limits)?,
    )
    .map_err(core_error)?;
    Ok(VersionPlan { inner })
}

/// Plans how to download and verify a selected artifact at one version.
#[wasm_bindgen(js_name = planDownload)]
pub fn plan_download(
    selected: &SelectedArtifact,
    #[wasm_bindgen(unchecked_optional_param_type = "DownloadOptions | null")] options: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "LimitsInput | null")] limits: JsValue,
) -> Result<DownloadPlan, JsValue> {
    let mut options = Fields::optional(&options, "arg.options")?;
    let version: Option<String> = options.take("version")?;
    let source: Option<SourceVariables> = options.take("source")?;
    let mirror: Option<String> = options.take("mirror")?;
    let checksum_mode: ChecksumMode = options.take("checksum_mode")?.unwrap_or_default();
    let inner = manifest_core::plan_download(
        &selected.selected(),
        DownloadOptions {
            version: version.as_deref(),
            source: source.as_ref(),
            mirror: mirror.as_deref(),
            checksum_mode,
        },
        &LimitsInput::decode(&limits)?,
    )
    .map_err(core_error)?;
    Ok(DownloadPlan { inner })
}

/// Renders the artifact name and canonical URL of a selected artifact, without
/// mirrors, parsers, or checksum policy.
#[wasm_bindgen(js_name = renderArtifact, unchecked_return_type = "RenderedArtifact")]
pub fn render_artifact(
    selected: &SelectedArtifact,
    #[wasm_bindgen(
        unchecked_optional_param_type = "Pick<DownloadOptions, \"version\" | \"source\"> | null"
    )]
    options: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "LimitsInput | null")] limits: JsValue,
) -> Result<JsValue, JsValue> {
    let mut options = Fields::optional(&options, "arg.options")?;
    let version: Option<String> = options.take("version")?;
    let source: Option<SourceVariables> = options.take("source")?;
    let rendered = selected
        .selected()
        .render(
            ArtifactRenderOptions {
                version: version.as_deref(),
                source: source.as_ref(),
            },
            &LimitsInput::decode(&limits)?,
        )
        .map_err(core_error)?;
    encode(&RenderedArtifact {
        artifact: rendered.artifact().to_owned(),
        canonical_url: rendered.canonical_url().to_owned(),
    })
}

/// Renders one template without URL normalization or percent encoding. Errors
/// are reported at `path`.
#[wasm_bindgen(js_name = renderTemplate, unchecked_return_type = "string")]
pub fn render_template(
    #[wasm_bindgen(unchecked_param_type = "string")] template: JsValue,
    #[wasm_bindgen(unchecked_param_type = "Record<string, TemplateValue>")] values: JsValue,
    #[wasm_bindgen(unchecked_param_type = "string")] path: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "LimitsInput | null")] limits: JsValue,
) -> Result<JsValue, JsValue> {
    let template: String = decode(&template, "arg.template")?;
    let values: BTreeMap<String, TemplateValue> = decode(&values, "arg.values")?;
    let path: String = decode(&path, "arg.path")?;
    let values = values
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_core()))
        .collect();
    let rendered =
        manifest_core::render_template(&template, &values, &path, &LimitsInput::decode(&limits)?)
            .map_err(core_error)?;
    encode(&rendered)
}
