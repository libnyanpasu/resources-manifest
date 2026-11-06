//! Regression: template validation must stay near-linear in the number of
//! distinct missing variables (each (path, variable) is reported once).

use manifest_core::validate_manifest;
use manifest_metadata::ResourceManifest;
use serde_json::Value;

#[test]
fn many_distinct_missing_variables_are_reported_once_each() {
    const COUNT: usize = 16_384;
    let snapshot = include_str!("../../manifest-metadata/tests/fixtures/resources.json");
    let mut manifest: Value = serde_json::from_str(snapshot).unwrap();
    let artifact = manifest
        .pointer_mut("/resources")
        .and_then(Value::as_object_mut)
        .and_then(|resources| resources.values_mut().next())
        .and_then(|resource| resource.pointer_mut("/variants/0/artifacts"))
        .and_then(Value::as_object_mut)
        .and_then(|artifacts| artifacts.values_mut().next())
        .and_then(|artifact| artifact.get_mut("artifact"))
        .expect("snapshot has an artifact template");
    let template: String = (0..COUNT).map(|index| format!("{{m{index:06}}}")).collect();
    *artifact = Value::String(template);

    let manifest: ResourceManifest = serde_json::from_value(manifest).unwrap();
    let errors = validate_manifest(manifest).unwrap_err();
    let missing = errors
        .errors()
        .iter()
        .filter(|error| error.code() == "validate.template_variable")
        .count();
    assert_eq!(missing, COUNT);
}
