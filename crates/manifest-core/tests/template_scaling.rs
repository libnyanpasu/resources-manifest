//! Regressions for template validation cost on large manifests: each distinct
//! (path, variable) is reported once, and releases that do not override
//! `source` share one inspected scope.

use manifest_core::{ValidationError, validate_manifest};
use manifest_metadata::ResourceManifest;
use serde_json::{Map, Value, json};

fn snapshot() -> Value {
    serde_json::from_str(include_str!(
        "../../manifest-metadata/tests/fixtures/resources.json"
    ))
    .unwrap()
}

/// The first artifact in the snapshot whose version history is static.
fn static_artifact(manifest: &mut Value) -> &mut Map<String, Value> {
    manifest["resources"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .flat_map(|resource| resource["variants"].as_array_mut().unwrap().iter_mut())
        .flat_map(|variant| variant["artifacts"].as_object_mut().unwrap().values_mut())
        .map(|artifact| artifact.as_object_mut().unwrap())
        .find(|artifact| artifact["version"]["type"] == "static")
        .expect("snapshot has a static artifact")
}

#[test]
fn many_distinct_missing_variables_are_reported_once_each_in_order() {
    const COUNT: usize = 16_384;
    let mut manifest = snapshot();
    let names: Vec<String> = (0..COUNT).map(|index| format!("m{index:06}")).collect();
    let template: String = names.iter().map(|name| format!("{{{name}}}")).collect();
    let artifact = static_artifact(&mut manifest);
    artifact.insert("artifact".into(), Value::String(template));
    // Several releases without overrides must not repeat the errors.
    let release = artifact["version"]["releases"][0].clone();
    let releases: Vec<Value> = (0..8)
        .map(|index| {
            let mut release = release.clone();
            release["version"] = json!(format!("1.0.{index}"));
            release
        })
        .collect();
    artifact.get_mut("version").unwrap()["releases"] = Value::Array(releases);
    artifact.get_mut("version").unwrap()["latest"] = json!("1.0.0");

    let manifest: ResourceManifest = serde_json::from_value(manifest).unwrap();
    let errors = validate_manifest(manifest).unwrap_err();
    let missing: Vec<&str> = errors
        .errors()
        .iter()
        .filter_map(|error| match error {
            ValidationError::ResolveVariable { variable, .. } => Some(variable.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(missing, names);
}

#[test]
fn many_sources_and_releases_without_overrides_validate() {
    const SOURCES: usize = 4_096;
    const RELEASES: usize = 4_096;
    let mut manifest = snapshot();
    let artifact = static_artifact(&mut manifest);
    let source: Map<String, Value> = (0..SOURCES)
        .map(|index| (format!("s{index:05}"), json!("literal")))
        .collect();
    artifact.insert("source".into(), Value::Object(source));
    let releases: Vec<Value> = (0..RELEASES)
        .map(|index| json!({ "version": format!("1.0.{index}") }))
        .collect();
    artifact.get_mut("version").unwrap()["releases"] = Value::Array(releases);
    artifact.get_mut("version").unwrap()["latest"] = json!("1.0.0");

    let manifest: ResourceManifest = serde_json::from_value(manifest).unwrap();
    let validated =
        validate_manifest(manifest).expect("literal sources and plain releases are valid");
    assert!(!validated.newer_minor());
}
