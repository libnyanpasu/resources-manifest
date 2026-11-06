use std::path::Path;

use manifest_core::{ValidationError, validate_manifest};
use manifest_metadata::{ResourceArtifactVersionTemplate, ResourceManifest};
use manifest_parser::{Limits, RemoteChecksumParser, RemoteVersionParser};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct Case {
    name: String,
    input: Value,
    expect: Value,
}

fn cases(directory: &str) -> Vec<Case> {
    fn load(directory: &Path, result: &mut Vec<Case>) {
        let mut paths: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                load(&path, result);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let corpus = std::fs::read_to_string(&path).unwrap();
                let loaded: Vec<Case> = serde_json::from_str(&corpus)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                assert!(!loaded.is_empty(), "{}", path.display());
                result.extend(loaded);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance");
    let mut result = Vec::new();
    load(&root.join(directory), &mut result);
    assert!(!result.is_empty());
    result
}

fn error_value(error: &ValidationError) -> Value {
    json!({ "code": error.code(), "path": error.path(), "fields": error.fields() })
}

mod validate {
    use super::*;

    #[test]
    fn corpus() {
        let cases = cases("validate");
        let count = cases.len();
        let mut failures = Vec::new();
        let mut positive = 0;
        let mut positive_codes = std::collections::BTreeSet::new();
        let mut negative_codes = std::collections::BTreeSet::new();
        for case in cases {
            let raw: ResourceManifest = serde_json::from_value(case.input)
                .unwrap_or_else(|error| panic!("{}: {error}", case.name));
            let result = validate_manifest(raw.clone());
            let passed = if let Some(ok) = case.expect.get("ok") {
                positive += 1;
                positive_codes.insert(case.name.split('.').take(2).collect::<Vec<_>>().join("."));
                match &result {
                    Ok(validated) => {
                        assert_eq!(validated.manifest(), &raw, "{}", case.name);
                        assert_eq!(
                            validated.newer_minor(),
                            raw.manifest_version.is_newer_minor()
                        );
                        *ok == json!(true)
                    }
                    Err(_) => false,
                }
            } else if let Err(errors) = &result {
                let mut actual: Vec<_> = errors.errors().iter().map(error_value).collect();
                for error in errors.errors() {
                    negative_codes.insert(error.code());
                }
                let mut expected = vec![case.expect["error"].clone()];
                if let Some(additional) = case.expect.get("additional_errors") {
                    expected.extend(additional.as_array().unwrap().iter().cloned());
                }
                actual.sort_by_key(Value::to_string);
                expected.sort_by_key(Value::to_string);
                actual == expected
            } else {
                false
            };
            if passed {
                println!("PASS {}", case.name);
            } else {
                failures.push(format!(
                    "{}: expected {}, got {result:?}",
                    case.name, case.expect
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        assert_eq!(negative_codes.len(), 10, "all invariant codes covered");
        for code in &negative_codes {
            assert!(positive_codes.contains(*code), "positive case for {code}");
        }
        assert!(positive >= 10, "one positive case per invariant");
        assert!(count >= 2 * 10);
        println!(
            "validate: {count} cases, {positive} positive, {} invariant codes",
            negative_codes.len()
        );
    }

    #[test]
    fn reference_snapshot() {
        let raw: ResourceManifest = serde_json::from_str(include_str!(
            "../../manifest-metadata/tests/fixtures/resources.json"
        ))
        .unwrap();
        let validated = validate_manifest(raw.clone()).expect("reference snapshot validates");
        assert_eq!(validated.manifest(), &raw);
        assert!(!validated.newer_minor());
    }
}

fn compat_projection(manifest: &manifest_core::ValidatedManifest) -> Value {
    let resource = &manifest.manifest().resources["sample"];
    let variant = &resource.variants[0];
    let artifacts: serde_json::Map<_, _> = variant.artifacts.iter().map(|(platform, artifact)| {
        let platform = serde_json::to_value(platform).unwrap().as_str().unwrap().to_owned();
        let (unsupported, kind) = match &artifact.version {
            ResourceArtifactVersionTemplate::Static { latest, releases } => {
                let latest = releases.iter().find(|release| release.version == *latest).unwrap();
                (None, if latest.checksum.is_some() { "authoritative" } else { "none" })
            }
            ResourceArtifactVersionTemplate::Remote { version, checksum, .. } => {
                let unsupported = if let RemoteVersionParser::Unsupported { parser_type } = &version.parser {
                    Some(parser_type)
                } else {
                    version.parser.compile(&Limits::default()).unwrap();
                    checksum.as_ref().and_then(|checksum| {
                        if let RemoteChecksumParser::Unsupported { parser_type } = &checksum.parser {
                            Some(parser_type)
                        } else { None }
                    })
                };
                (unsupported, if checksum.is_some() { "parsed" } else { "none" })
            }
        };
        let projection = if let Some(parser_type) = unsupported {
            json!({ "error": { "code": "resolve.unsupported_parser", "fields": { "parser_type": parser_type } }, "kind": "none" })
        } else {
            json!({ "ok": true, "kind": kind })
        };
        (platform, projection)
    }).collect();
    json!({ "newer_minor": manifest.newer_minor(), "default_variants": resource.default_variants, "artifacts": artifacts })
}

#[test]
fn compat() {
    let cases = cases("compat");
    let count = cases.len();
    for case in cases {
        let raw = serde_json::from_value::<ResourceManifest>(case.input.clone());
        if let Some(expected) = case.expect.get("error") {
            let error = raw.expect_err(&case.name).to_string();
            let version = case.input["manifest_version"]
                .as_str()
                .map(str::parse::<manifest_metadata::ManifestVersion>);
            let (code, fields) = match version {
                Some(Err(manifest_metadata::ParseManifestVersionError::UnsupportedMajor {
                    major,
                })) => ("manifest.unsupported_major", json!({ "major": major })),
                Some(Ok(_)) => panic!("unexpected deserialization error: {error}"),
                _ => ("manifest.invalid_version_format", json!({})),
            };
            assert!(error.contains(code), "{}: {error}", case.name);
            assert_eq!(
                json!({ "code": code, "path": "manifest_version", "fields": fields }),
                *expected,
                "{}",
                case.name
            );
        } else {
            let raw = raw.unwrap_or_else(|error| panic!("{}: {error}", case.name));
            let validated =
                validate_manifest(raw).unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
            assert_eq!(
                compat_projection(&validated),
                case.expect["ok"],
                "{}",
                case.name
            );
            let serialized = serde_json::to_value(validated.manifest()).unwrap();
            let mut normalized = case.input;
            let resource = &mut normalized["resources"]["sample"];
            resource["default_variants"]
                .as_object_mut()
                .unwrap()
                .remove("future-os-cpu");
            for variant in resource["variants"].as_array_mut().unwrap() {
                variant["artifacts"]
                    .as_object_mut()
                    .unwrap()
                    .remove("future-os-cpu");
                for artifact in variant["artifacts"].as_object_mut().unwrap().values_mut() {
                    for field in ["version", "checksum"] {
                        if let Some(parser) =
                            artifact.pointer_mut(&format!("/version/{field}/parser"))
                            && parser["type"]
                                .as_str()
                                .is_some_and(|kind| kind.starts_with("future-"))
                        {
                            *parser = json!({ "type": parser["type"] });
                        }
                    }
                }
            }
            assert_eq!(serialized, normalized, "{}: preserved wire data", case.name);
        }
        println!("PASS {}", case.name);
    }
    println!("compat: {count} cases");
}
