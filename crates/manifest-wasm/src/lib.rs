use std::collections::BTreeMap;
use std::rc::Rc;

use manifest_core::{ChecksumMode, DownloadOptions, VariantSelection, VersionOptions};
use manifest_metadata::{
    ManifestVersion, ParseManifestVersionError, ResourceArtifactTemplate, ResourceManifest,
    SourceVariables, SupportedPlatform,
};
use manifest_parser::{Limits, RemoteChecksumParser, RemoteResponse, RemoteVersionParser};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use wasm_bindgen::prelude::*;

fn error(code: &str, path: &str, message: impl std::fmt::Display, fields: Value) -> JsValue {
    json!({ "code": code, "path": path, "message": message.to_string(), "fields": fields })
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .expect("JSON error objects are serializable to JavaScript")
}

fn invalid_input(path: &str, message: impl std::fmt::Display) -> JsValue {
    error("wasm.invalid_input", path, message, json!({}))
}

fn from_js<T: DeserializeOwned>(value: JsValue, path: &str) -> Result<T, JsValue> {
    serde_wasm_bindgen::from_value(value).map_err(|source| invalid_input(path, source))
}

fn to_js(value: &impl Serialize) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|source| error("wasm.serialization_failed", "arg.result", source, json!({})))
}

fn object(value: JsValue, path: &str) -> Result<Map<String, Value>, JsValue> {
    from_js(value, path)
}

fn options(value: JsValue, path: &str) -> Result<Map<String, Value>, JsValue> {
    if value.is_null() || value.is_undefined() {
        Ok(Map::new())
    } else {
        object(value, path)
    }
}

fn field<T: DeserializeOwned>(
    object: &mut Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<T>, JsValue> {
    object
        .remove(key)
        .filter(|value| !value.is_null())
        .map(|value| {
            serde_json::from_value(value)
                .map_err(|source| invalid_input(&format!("{path}.{key}"), source))
        })
        .transpose()
}

fn required<T: DeserializeOwned>(
    object: &mut Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<T, JsValue> {
    field(object, key, path)?
        .ok_or_else(|| invalid_input(&format!("{path}.{key}"), "missing required field"))
}

fn limits(value: JsValue) -> Result<Limits, JsValue> {
    let mut input = options(value, "arg.limits")?;
    let mut limits = Limits::default();
    if let Some(value) = field(&mut input, "body_bytes", "arg.limits")? {
        limits = limits.with_body_bytes(value);
    }
    if let Some(value) = field(&mut input, "header_value_bytes", "arg.limits")? {
        limits = limits.with_header_value_bytes(value);
    }
    if let Some(value) = field(&mut input, "regex_pattern_bytes", "arg.limits")? {
        limits = limits.with_regex_pattern_bytes(value);
    }
    if let Some(value) = field(&mut input, "regex_size_bytes", "arg.limits")? {
        limits = limits.with_regex_size_bytes(value);
    }
    if let Some(value) = field(&mut input, "template_depth", "arg.limits")? {
        limits = limits.with_template_depth(value);
    }
    if let Some(value) = field(&mut input, "render_output_bytes", "arg.limits")? {
        limits = limits.with_render_output_bytes(value);
    }
    Ok(limits)
}

#[wasm_bindgen]
#[derive(Debug)]
#[must_use]
pub struct ValidatedManifest {
    inner: Rc<manifest_core::ValidatedManifest>,
}

#[wasm_bindgen]
impl ValidatedManifest {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        to_js(self.inner.manifest())
    }

    #[wasm_bindgen(getter, js_name = newerMinor)]
    #[must_use]
    pub fn newer_minor(&self) -> bool {
        self.inner.newer_minor()
    }
}

#[wasm_bindgen]
#[derive(Debug)]
#[must_use]
pub struct SelectedArtifact {
    manifest: Rc<manifest_core::ValidatedManifest>,
    resource: String,
    variant: String,
    platform: SupportedPlatform,
}

impl SelectedArtifact {
    fn selected(&self) -> manifest_core::SelectedArtifact<'_> {
        self.manifest
            .artifact(&self.resource, &self.variant, self.platform)
            .expect("selected artifact belongs to its immutable validated manifest")
    }
}

#[wasm_bindgen]
impl SelectedArtifact {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        to_js(self.selected().variant())
    }
}

#[wasm_bindgen]
#[derive(Debug)]
#[must_use]
pub struct VersionPlan {
    inner: manifest_core::VersionPlan,
}

#[wasm_bindgen]
impl VersionPlan {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        to_js(&self.inner)
    }
}

#[wasm_bindgen]
#[derive(Debug)]
#[must_use]
pub struct DownloadPlan {
    inner: manifest_core::DownloadPlan,
}

#[wasm_bindgen]
impl DownloadPlan {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        to_js(&self.inner)
    }
}

#[wasm_bindgen(js_name = validateManifest)]
pub fn validate_manifest(raw: JsValue) -> Result<ValidatedManifest, JsValue> {
    let mut raw = object(raw, "arg.manifest")?;
    let version = raw
        .get("manifest_version")
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
    let raw: ResourceManifest = serde_json::from_value(Value::Object(std::mem::take(&mut raw)))
        .map_err(|source| invalid_input("arg.manifest", source))?;
    manifest_core::validate_manifest(raw)
        .map(|inner| ValidatedManifest {
            inner: Rc::new(inner),
        })
        .map_err(|source| {
            let errors: Vec<_> = source
                .errors()
                .iter()
                .map(|error| {
                    json!({
                        "code": error.code(), "path": error.path(),
                        "message": error.to_string(), "fields": error.fields()
                    })
                })
                .collect();
            error(
                "validate.failed",
                "arg.manifest",
                source,
                json!({ "errors": errors }),
            )
        })
}

#[wasm_bindgen(js_name = artifactChecksumKind)]
pub fn artifact_checksum_kind(artifact: JsValue) -> Result<JsValue, JsValue> {
    let artifact: ResourceArtifactTemplate = from_js(artifact, "arg.artifact")?;
    to_js(&manifest_core::artifact_checksum_kind(&artifact))
}

#[wasm_bindgen(js_name = selectVariant)]
pub fn select_variant(
    manifest: &ValidatedManifest,
    resource: JsValue,
    platform: JsValue,
    selection: JsValue,
) -> Result<SelectedArtifact, JsValue> {
    let resource: String = from_js(resource, "arg.resource")?;
    let platform = from_js(platform, "arg.platform")?;
    let mut selection = options(selection, "arg.selection")?;
    let id: Option<String> = field(&mut selection, "id", "arg.selection")?;
    let tags: Option<Vec<String>> = field(&mut selection, "tags", "arg.selection")?;
    let selected = manifest_core::select_variant(
        &manifest.inner,
        &resource,
        platform,
        VariantSelection {
            id: id.as_deref(),
            tags: tags.as_deref(),
        },
    )
    .map_err(|source| error(source.code(), &source.path(), &source, source.fields()))?;
    let variant = selected.variant().id.clone();
    Ok(SelectedArtifact {
        manifest: Rc::clone(&manifest.inner),
        resource,
        variant,
        platform,
    })
}

#[wasm_bindgen(js_name = planVersion)]
pub fn plan_version(
    selected: &SelectedArtifact,
    input: JsValue,
    input_limits: JsValue,
) -> Result<VersionPlan, JsValue> {
    let mut input = options(input, "arg.options")?;
    let source: Option<SourceVariables> = field(&mut input, "source", "arg.options")?;
    let mirror: Option<String> = field(&mut input, "mirror", "arg.options")?;
    manifest_core::plan_version(
        &selected.selected(),
        VersionOptions {
            source: source.as_ref(),
            mirror: mirror.as_deref(),
        },
        &limits(input_limits)?,
    )
    .map(|inner| VersionPlan { inner })
    .map_err(|source| error(source.code(), &source.path(), &source, source.fields()))
}

#[wasm_bindgen(js_name = planDownload)]
pub fn plan_download(
    selected: &SelectedArtifact,
    input: JsValue,
    input_limits: JsValue,
) -> Result<DownloadPlan, JsValue> {
    let mut input = options(input, "arg.options")?;
    let version: Option<String> = field(&mut input, "version", "arg.options")?;
    let source: Option<SourceVariables> = field(&mut input, "source", "arg.options")?;
    let mirror: Option<String> = field(&mut input, "mirror", "arg.options")?;
    let checksum_mode: ChecksumMode =
        field(&mut input, "checksum_mode", "arg.options")?.unwrap_or_default();
    manifest_core::plan_download(
        &selected.selected(),
        DownloadOptions {
            version: version.as_deref(),
            source: source.as_ref(),
            mirror: mirror.as_deref(),
            checksum_mode,
        },
        &limits(input_limits)?,
    )
    .map(|inner| DownloadPlan { inner })
    .map_err(|source| error(source.code(), &source.path(), &source, source.fields()))
}

#[wasm_bindgen(js_name = parseVersion)]
pub fn parse_version(
    parser: JsValue,
    response: JsValue,
    input_limits: JsValue,
) -> Result<JsValue, JsValue> {
    let parser: RemoteVersionParser = from_js(parser, "arg.parser")?;
    let mut response = object(response, "response")?;
    let body: String = required(&mut response, "body", "response")?;
    let headers: Vec<(String, String)> =
        field(&mut response, "headers", "response")?.unwrap_or_default();
    let version = parser
        .compile(&limits(input_limits)?)
        .and_then(|parser| {
            parser.parse(&RemoteResponse {
                body: &body,
                headers: &headers,
            })
        })
        .map_err(|source| error(source.code(), &source.path(), &source, source.fields()))?;
    to_js(&version.as_ref())
}

#[wasm_bindgen(js_name = finishChecksum)]
pub fn finish_checksum(plan: &DownloadPlan, response: JsValue) -> Result<JsValue, JsValue> {
    let checksum_plan = plan.inner.checksum_request().ok_or_else(|| {
        error(
            "wasm.checksum_plan_missing",
            "arg.plan",
            "download plan has no remote checksum request",
            json!({}),
        )
    })?;
    let mut response = object(response, "response")?;
    let body: String = required(&mut response, "body", "response")?;
    let headers: Vec<(String, String)> =
        field(&mut response, "headers", "response")?.unwrap_or_default();
    let resolved = manifest_core::finish_checksum(
        checksum_plan,
        &RemoteResponse {
            body: &body,
            headers: &headers,
        },
    )
    .map_err(|source| error(source.code(), &source.path(), &source, source.fields()))?;
    to_js(&resolved)
}

#[wasm_bindgen(js_name = checkPolicy)]
pub fn check_policy(kind: JsValue, policy: JsValue) -> Result<(), JsValue> {
    let kind = from_js(kind, "arg.kind")?;
    let policy = if policy.is_null() || policy.is_undefined() {
        manifest_core::VerifyPolicy::default()
    } else {
        from_js(policy, "arg.policy")?
    };
    manifest_core::check_policy(kind, &policy)
        .map_err(|source| error(source.code(), source.path(), &source, source.fields()))
}

#[wasm_bindgen(js_name = parseChecksum)]
pub fn parse_checksum(
    parser: JsValue,
    response: JsValue,
    artifact: JsValue,
    input_limits: JsValue,
) -> Result<JsValue, JsValue> {
    let parser: RemoteChecksumParser = from_js(parser, "arg.parser")?;
    let artifact: String = from_js(artifact, "arg.artifact")?;
    let mut response = object(response, "response")?;
    let body: String = required(&mut response, "body", "response")?;
    let headers: Vec<(String, String)> =
        field(&mut response, "headers", "response")?.unwrap_or_default();
    let checksum = parser
        .parse(
            &RemoteResponse {
                body: &body,
                headers: &headers,
            },
            &artifact,
            &limits(input_limits)?,
        )
        .map_err(|source| error(source.code(), &source.path(), &source, source.fields()))?;
    to_js(&checksum)
}

#[wasm_bindgen(js_name = listVariantChecksumKinds)]
pub fn list_variant_checksum_kinds(
    manifest: &ValidatedManifest,
    resource: JsValue,
    platform: JsValue,
) -> Result<JsValue, JsValue> {
    let resource: String = from_js(resource, "arg.resource")?;
    let platform = from_js(platform, "arg.platform")?;
    let kinds = manifest_core::list_variant_checksum_kinds(&manifest.inner, &resource, platform)
        .map_err(|source| error(source.code(), &source.path(), &source, source.fields()))?;
    to_js(&kinds)
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireTemplateValue {
    Authored(String),
    Literal(String),
}

#[wasm_bindgen(js_name = renderTemplate)]
pub fn render_template(
    template: JsValue,
    values: JsValue,
    path: JsValue,
    input_limits: JsValue,
) -> Result<JsValue, JsValue> {
    let template: String = from_js(template, "arg.template")?;
    let values: BTreeMap<String, WireTemplateValue> = from_js(values, "arg.values")?;
    let path: String = from_js(path, "arg.path")?;
    let values = values
        .iter()
        .map(|(key, value)| {
            let value = match value {
                WireTemplateValue::Authored(value) => manifest_core::TemplateValue::Authored(value),
                WireTemplateValue::Literal(value) => manifest_core::TemplateValue::Literal(value),
            };
            (key.as_str(), value)
        })
        .collect();
    let rendered =
        manifest_core::render_template(&template, &values, &path, &limits(input_limits)?)
            .map_err(|source| error(source.code(), source.path(), &source, source.fields()))?;
    to_js(&rendered)
}

#[wasm_bindgen(js_name = renderArtifact)]
pub fn render_artifact(
    selected: &SelectedArtifact,
    input: JsValue,
    input_limits: JsValue,
) -> Result<JsValue, JsValue> {
    let mut input = options(input, "arg.options")?;
    let version: Option<String> = field(&mut input, "version", "arg.options")?;
    let source: Option<SourceVariables> = field(&mut input, "source", "arg.options")?;
    let rendered = selected
        .selected()
        .render(
            manifest_core::ArtifactRenderOptions {
                version: version.as_deref(),
                source: source.as_ref(),
            },
            &limits(input_limits)?,
        )
        .map_err(|source| error(source.code(), source.path(), &source, source.fields()))?;
    to_js(&json!({ "artifact": rendered.artifact(), "canonical_url": rendered.canonical_url() }))
}

#[wasm_bindgen(js_name = compareDigest)]
pub fn compare_digest(
    algorithm: JsValue,
    expected: JsValue,
    actual: JsValue,
) -> Result<(), JsValue> {
    let algorithm = from_js(algorithm, "arg.algorithm")?;
    let expected: String = from_js(expected, "arg.expected")?;
    let actual: String = from_js(actual, "arg.actual")?;
    if expected == actual {
        return Ok(());
    }
    let source = manifest_core::VerifyError::CompareDigest {
        algorithm,
        expected,
        actual,
    };
    Err(error(
        source.code(),
        source.path(),
        &source,
        source.fields(),
    ))
}
