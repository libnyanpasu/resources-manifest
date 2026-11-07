use std::collections::BTreeSet;

use manifest_core::{
    DownloadOptions, VariantSelection, plan_download, select_variant, validate_manifest,
};
use manifest_metadata::{ResourceArtifactVersionTemplate, SupportedPlatform};
use manifest_parser::Limits;
use serde::Deserialize;

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
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    input: Input,
    expect: Expectation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    manifest: Snapshot,
    resource: String,
    platform: SupportedPlatform,
    variant: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    snapshot: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expectation {
    ok: Output,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    artifact: String,
    canonical_url: String,
    download_url: String,
}

#[test]
fn reference_downloads_match_every_default_artifact() {
    let manifest = validate_manifest(
        serde_json::from_str(include_str!(
            "../../manifest-metadata/tests/fixtures/resources.json"
        ))
        .expect("compiled reference snapshot decodes"),
    )
    .expect("compiled reference snapshot validates");
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "../../../conformance/resolve/reference-golden.json"
    ))
    .expect("reference golden decodes");
    let expected: BTreeSet<_> = manifest
        .manifest()
        .resources
        .iter()
        .flat_map(|(id, resource)| {
            resource.variants.iter().flat_map(move |variant| {
                variant
                    .artifacts
                    .keys()
                    .map(move |platform| (id.as_str(), *platform))
            })
        })
        .collect();
    assert!(!expected.is_empty(), "snapshot contains resource platforms");
    let mut covered = BTreeSet::new();
    let mut failures = Vec::new();
    for case in &cases {
        let input = &case.input;
        assert_eq!(input.manifest.snapshot, "resources.json", "{}", case.name);
        assert!(
            covered.insert((input.resource.as_str(), input.platform)),
            "{}: duplicate resource/platform",
            case.name
        );
        let selected = select_variant(
            &manifest,
            &input.resource,
            input.platform,
            VariantSelection::default(),
        )
        .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        assert_eq!(selected.variant().id, input.variant, "{}", case.name);
        let version = match &selected.template().version {
            ResourceArtifactVersionTemplate::Static { latest, .. } => latest,
            ResourceArtifactVersionTemplate::Remote { fallback, .. } => {
                &fallback
                    .as_ref()
                    .unwrap_or_else(|| panic!("{}: remote fallback version exists", case.name))
                    .version
            }
        };
        assert_eq!(version, &input.version, "{}", case.name);
        let options = DownloadOptions {
            version: matches!(
                selected.template().version,
                ResourceArtifactVersionTemplate::Remote { .. }
            )
            .then_some(input.version.as_str()),
            ..DownloadOptions::default()
        };
        let plan = match plan_download(&selected, options, &Limits::default()) {
            Ok(plan) => plan,
            Err(error) => {
                failures.push(format!("{}: {error}", case.name));
                continue;
            }
        };
        assert_eq!(plan.version(), input.version, "{}", case.name);
        assert_eq!(plan.request().mirror_url(), None, "{}", case.name);
        let failures_before = failures.len();
        for (field, actual, expected) in [
            (
                "artifact",
                plan.artifact(),
                case.expect.ok.artifact.as_str(),
            ),
            (
                "canonical_url",
                plan.request().canonical_url(),
                case.expect.ok.canonical_url.as_str(),
            ),
            (
                "download_url",
                plan.request()
                    .mirror_url()
                    .unwrap_or(plan.request().canonical_url()),
                case.expect.ok.download_url.as_str(),
            ),
        ] {
            if actual.as_bytes() != expected.as_bytes() {
                failures.push(format!(
                    "{}: {field}: expected {expected:?}, got {actual:?}",
                    case.name
                ));
            }
        }
        if failures.len() == failures_before {
            record_case(&case.name);
        }
    }
    assert_eq!(covered, expected, "golden covers every resource/platform");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!(
        "reference golden: {} resource/platform defaults",
        cases.len()
    );
}
