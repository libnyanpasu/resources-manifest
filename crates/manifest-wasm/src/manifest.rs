//! Manifest validation, variant selection, and checksum kinds.

use std::rc::Rc;

use manifest_core::VariantSelection;
use manifest_metadata::{ManifestVersion, ParseManifestVersionError, ResourceManifest};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use crate::bridge::{Fields, decode, encode};
use crate::error::{core_error, error};
use crate::handles::{SelectedArtifact, ValidatedManifest};

/// Validates a raw manifest.
///
/// `manifest_version` is checked first so an unsupported major fails with
/// `manifest.*` before the body is decoded. Semantic violations are thrown
/// together as `validate.failed`, keeping core's order and each violation's
/// code, path, and fields.
#[wasm_bindgen(js_name = validateManifest)]
pub fn validate_manifest(
    #[wasm_bindgen(unchecked_param_type = "unknown")] raw: JsValue,
) -> Result<ValidatedManifest, JsValue> {
    let raw = Fields::object(&raw, "arg.manifest")?;
    let version = raw
        .peek("manifest_version")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            error(
                "manifest.invalid_version_format",
                "manifest_version",
                "manifest_version must be a canonical major.minor string",
                json!({}),
            )
        })?;
    version.parse::<ManifestVersion>().map_err(|source| {
        let fields = match source {
            ParseManifestVersionError::UnsupportedMajor { major } => json!({ "major": major }),
            _ => json!({}),
        };
        error(source.code(), source.path(), source, fields)
    })?;
    let raw: ResourceManifest = raw.decode()?;
    let inner = manifest_core::validate_manifest(raw).map_err(|source| {
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
            "validate.failed",
            "arg.manifest",
            &source,
            json!({ "errors": errors }),
        )
    })?;
    Ok(ValidatedManifest {
        inner: Rc::new(inner),
    })
}

/// Selects the artifact of one resource for one platform, by variant id, by
/// tags, or by the platform default.
#[wasm_bindgen(js_name = selectVariant)]
pub fn select_variant(
    manifest: &ValidatedManifest,
    #[wasm_bindgen(unchecked_param_type = "string")] resource: JsValue,
    #[wasm_bindgen(unchecked_param_type = "SupportedPlatform")] platform: JsValue,
    #[wasm_bindgen(unchecked_optional_param_type = "VariantSelection | null")] selection: JsValue,
) -> Result<SelectedArtifact, JsValue> {
    let resource: String = decode(&resource, "arg.resource")?;
    let platform = decode(&platform, "arg.platform")?;
    let mut selection = Fields::optional(&selection, "arg.selection")?;
    let id: Option<String> = selection.take("id")?;
    let tags: Option<Vec<String>> = selection.take("tags")?;
    let selected = manifest_core::select_variant(
        &manifest.inner,
        &resource,
        platform,
        VariantSelection {
            id: id.as_deref(),
            tags: tags.as_deref(),
        },
    )
    .map_err(core_error)?;
    let variant = selected.variant().id.clone();
    Ok(SelectedArtifact::new(
        &manifest.inner,
        resource,
        variant,
        platform,
    ))
}

/// Lists the checksum kind of every variant of a resource that has an artifact
/// for the platform.
#[wasm_bindgen(
    js_name = listVariantChecksumKinds,
    unchecked_return_type = "VariantChecksumKind[]"
)]
pub fn list_variant_checksum_kinds(
    manifest: &ValidatedManifest,
    #[wasm_bindgen(unchecked_param_type = "string")] resource: JsValue,
    #[wasm_bindgen(unchecked_param_type = "SupportedPlatform")] platform: JsValue,
) -> Result<JsValue, JsValue> {
    let resource: String = decode(&resource, "arg.resource")?;
    let platform = decode(&platform, "arg.platform")?;
    let kinds = manifest_core::list_variant_checksum_kinds(&manifest.inner, &resource, platform)
        .map_err(core_error)?;
    encode(&kinds)
}

/// Returns the checksum kind an artifact template would resolve to.
#[wasm_bindgen(js_name = artifactChecksumKind, unchecked_return_type = "ChecksumKind")]
pub fn artifact_checksum_kind(
    #[wasm_bindgen(unchecked_param_type = "ResourceArtifactTemplate")] artifact: JsValue,
) -> Result<JsValue, JsValue> {
    let artifact = decode(&artifact, "arg.artifact")?;
    encode(&manifest_core::artifact_checksum_kind(&artifact))
}
