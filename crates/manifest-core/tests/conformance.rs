use std::path::Path;

use manifest_core::{
    ChecksumKind, ChecksumMode, DownloadOptions, FinishVersionError, ResolveError, ValidationError,
    VariantSelection, VerifyError, VerifyPolicy, VersionOptions, artifact_checksum_kind,
    check_policy, finish_checksum, finish_version, list_variant_checksum_kinds, plan_download,
    plan_version, select_variant, validate_manifest,
};
use manifest_metadata::{ResourceManifest, SourceVariables, SupportedPlatform};
use manifest_parser::{Limits, ParseError, RemoteResponse};
use serde::Deserialize;
use serde_json::{Value, json};

fn record_case(name: &str) {
    use std::io::Write;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    if let Some(path) = std::env::var_os("MANIFEST_CONFORMANCE_LIST") {
        let _guard = LOCK.lock().expect("case list lock poisoned");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("case list is writable");
        writeln!(file, "{}", serde_json::to_string(name).unwrap()).unwrap();
    }
}

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
            assert_eq!(
                result,
                validate_manifest(raw.clone()),
                "{}: validation result and error order are deterministic",
                case.name
            );
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
                record_case(&case.name);
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
        let kind = artifact_checksum_kind(artifact);
        let selected = manifest.artifact("sample", &variant.id, *platform).unwrap();
        let projection = match plan_version(&selected, VersionOptions::default(), &Limits::default()) {
            Ok(_) => json!({ "ok": true, "kind": kind }),
            Err(error) => {
                assert_eq!(kind, ChecksumKind::None);
                for allow_parsed in [false, true] {
                    for allow_snapshot in [false, true] {
                        assert!(check_policy(kind, &VerifyPolicy { allow_parsed, allow_snapshot }).is_err());
                    }
                }
                json!({ "error": { "code": error.code(), "fields": error.fields() }, "kind": kind })
            }
        };
        (platform.as_str().to_owned(), projection)
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
        record_case(&case.name);
        println!("PASS {}", case.name);
    }
    println!("compat: {count} cases");
}

fn resolve_result(result: Result<Value, ResolveError>) -> Value {
    match result {
        Ok(value) => json!({ "ok": value }),
        Err(error) => {
            json!({ "error": { "code": error.code(), "path": error.path(), "fields": error.fields() } })
        }
    }
}

fn parse_result(result: Result<Value, ParseError>) -> Value {
    match result {
        Ok(value) => json!({ "ok": value }),
        Err(error) => {
            json!({ "error": { "code": error.code(), "path": error.path(), "fields": error.fields() } })
        }
    }
}

fn finish_version_result(result: Result<Value, FinishVersionError>) -> Value {
    match result {
        Ok(value) => json!({ "ok": value }),
        Err(error) => {
            json!({ "error": { "code": error.code(), "path": error.path(), "fields": error.fields() } })
        }
    }
}

fn verify_result(result: Result<(), VerifyError>) -> Value {
    match result {
        Ok(()) => json!({ "ok": true }),
        Err(error) => {
            json!({ "error": { "code": error.code(), "path": error.path(), "fields": error.fields() } })
        }
    }
}

fn kind_case(input: Value) -> Value {
    let Some(operation) = input["operation"].as_str() else {
        let artifact = serde_json::from_value(input).unwrap();
        let kind = artifact_checksum_kind(&artifact);
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
        return json!({ "ok": kind });
    };
    if operation == "policy" {
        let policy: VerifyPolicy = serde_json::from_value(input["policy"].clone()).unwrap();
        let kind = serde_json::from_value(input["kind"].clone()).unwrap();
        return verify_result(check_policy(kind, &policy));
    }
    if operation == "digest_mismatch" {
        return verify_result(Err(VerifyError::CompareDigest {
            algorithm: serde_json::from_value(input["algorithm"].clone()).unwrap(),
            expected: input["expected"].as_str().unwrap().to_owned(),
            actual: input["actual"].as_str().unwrap().to_owned(),
        }));
    }
    let manifest =
        validate_manifest(serde_json::from_value(input["manifest"].clone()).unwrap()).unwrap();
    let resource = input["resource"].as_str().unwrap();
    let platform: SupportedPlatform = serde_json::from_value(input["platform"].clone()).unwrap();
    if operation == "list" {
        return resolve_result(
            list_variant_checksum_kinds(&manifest, resource, platform)
                .map(|kinds| serde_json::to_value(kinds).unwrap()),
        );
    }
    let tags: Option<Vec<String>> = input
        .get("tags")
        .map(|tags| serde_json::from_value(tags.clone()).unwrap());
    let selected = match select_variant(
        &manifest,
        resource,
        platform,
        VariantSelection {
            id: input["variant_id"].as_str(),
            tags: tags.as_deref(),
        },
    ) {
        Ok(selected) => selected,
        Err(error) => return resolve_result(Err(error)),
    };
    if operation == "select" {
        return json!({ "ok": { "variant_id": selected.variant().id, "kind": artifact_checksum_kind(selected.template()) } });
    }
    let source: Option<SourceVariables> = input
        .get("source")
        .map(|source| serde_json::from_value(source.clone()).unwrap());
    let mut limits = Limits::default();
    if let Some(value) = input["limits"]["body_bytes"].as_u64() {
        limits = limits.with_body_bytes(usize::try_from(value).unwrap());
    }
    if let Some(value) = input["limits"]["header_value_bytes"].as_u64() {
        limits = limits.with_header_value_bytes(usize::try_from(value).unwrap());
    }
    if let Some(value) = input["limits"]["render_output_bytes"].as_u64() {
        limits = limits.with_render_output_bytes(usize::try_from(value).unwrap());
    }
    if let Some(value) = input["limits"]["regex_pattern_bytes"].as_u64() {
        limits = limits.with_regex_pattern_bytes(usize::try_from(value).unwrap());
    }
    let mirror = input["mirror"].as_str();
    let response = input.get("response");
    let body = response
        .map(|response| response["body"].as_str().unwrap())
        .unwrap_or_default();
    let headers: Vec<(String, String)> = response
        .map(|response| serde_json::from_value(response["headers"].clone()).unwrap())
        .unwrap_or_default();
    let response = RemoteResponse {
        body,
        headers: &headers,
    };
    if operation == "version" {
        let plan = plan_version(
            &selected,
            VersionOptions {
                source: source.as_ref(),
                mirror,
            },
            &limits,
        );
        // `response` is shorthand for a single-element `responses`.
        let inputs: Option<Vec<Value>> = input
            .get("responses")
            .map(|responses| responses.as_array().unwrap().clone())
            .or_else(|| input.get("response").map(|response| vec![response.clone()]));
        return match plan {
            Ok(plan) if inputs.is_some() => {
                let owned: Vec<(String, Vec<(String, String)>)> = inputs
                    .unwrap()
                    .iter()
                    .map(|response| {
                        (
                            response["body"].as_str().unwrap().to_owned(),
                            response
                                .get("headers")
                                .map(|headers| serde_json::from_value(headers.clone()).unwrap())
                                .unwrap_or_default(),
                        )
                    })
                    .collect();
                let responses: Vec<_> = owned
                    .iter()
                    .map(|(body, headers)| RemoteResponse { body, headers })
                    .collect();
                finish_version_result(
                    finish_version(plan.remote().unwrap(), &responses)
                        .map(|version| json!({ "plan": plan, "version": version.as_ref() })),
                )
            }
            result => resolve_result(result.map(|plan| serde_json::to_value(plan).unwrap())),
        };
    }
    assert!(matches!(operation, "download" | "finish_checksum"));
    let plan = plan_download(
        &selected,
        DownloadOptions {
            version: input["version"].as_str(),
            source: source.as_ref(),
            mirror,
            checksum_mode: match input["checksum_mode"].as_str() {
                None | Some("remote") => ChecksumMode::Remote,
                Some("snapshot") => ChecksumMode::Snapshot,
                other => panic!("unknown checksum_mode {other:?}"),
            },
        },
        &limits,
    );
    match plan {
        Ok(plan) if operation == "finish_checksum" => parse_result(
            finish_checksum(plan.checksum_request().unwrap(), &response).map(|checksum| {
                assert_eq!(checksum.kind(), plan.kind());
                serde_json::to_value(checksum).unwrap()
            }),
        ),
        result => resolve_result(result.map(|plan| serde_json::to_value(plan).unwrap())),
    }
}

#[test]
fn kind() {
    let cases = cases("kind");
    let count = cases.len();
    for case in cases {
        let actual = kind_case(case.input);
        assert_eq!(actual, case.expect, "{}", case.name);
        record_case(&case.name);
        println!("PASS {}", case.name);
    }
    println!("kind: {count} cases");
}
