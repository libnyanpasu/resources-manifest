use std::collections::BTreeMap;
use std::path::Path;

use manifest_core::{ArtifactRenderOptions, TemplateValue, render_template, validate_manifest};
use manifest_metadata::{
    ResourceArtifactVersionTemplate, ResourceManifest, SourceVariables, SupportedPlatform,
};
use manifest_parser::Limits;
use serde::Deserialize;
use serde_json::{Value, json};

const SNAPSHOT: &str = include_str!("../../manifest-metadata/tests/fixtures/resources.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    input: Input,
    expect: Value,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Input {
    Artifact(ArtifactInput),
    Template(TemplateInput),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactInput {
    manifest: Value,
    resource: String,
    variant: String,
    platform: SupportedPlatform,
    version: Option<String>,
    source: Option<SourceVariables>,
    #[serde(default)]
    limits: LimitOverrides,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateInput {
    template: String,
    values: BTreeMap<String, WireValue>,
    path: String,
    #[serde(default)]
    limits: LimitOverrides,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireValue {
    Authored(String),
    Literal(String),
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LimitOverrides {
    template_depth: Option<usize>,
    render_output_bytes: Option<usize>,
}

impl LimitOverrides {
    fn apply(&self) -> Limits {
        let mut limits = Limits::default();
        if let Some(depth) = self.template_depth {
            limits = limits.with_template_depth(depth);
        }
        if let Some(bytes) = self.render_output_bytes {
            limits = limits.with_render_output_bytes(bytes);
        }
        limits
    }
}

fn load(directory: &Path, cases: &mut Vec<Case>) {
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            load(&path, cases);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let loaded: Vec<Case> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(!loaded.is_empty(), "{}", path.display());
            cases.extend(loaded);
        }
    }
}

#[test]
fn corpus() {
    let mut cases = Vec::new();
    load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/resolve"),
        &mut cases,
    );
    assert!(!cases.is_empty());
    let count = cases.len();
    let mut failures = Vec::new();
    let mut codes = std::collections::BTreeSet::new();
    for case in cases {
        let result = match case.input {
            Input::Template(input) => {
                let values = input
                    .values
                    .iter()
                    .map(|(key, value)| {
                        let value = match value {
                            WireValue::Authored(value) => TemplateValue::Authored(value),
                            WireValue::Literal(value) => TemplateValue::Literal(value),
                        };
                        (key.as_str(), value)
                    })
                    .collect();
                render_template(&input.template, &values, &input.path, &input.limits.apply())
                    .map(Value::String)
            }
            Input::Artifact(input) => {
                let raw: ResourceManifest =
                    if input.manifest == json!({ "snapshot": "resources.json" }) {
                        serde_json::from_str(SNAPSHOT).unwrap()
                    } else {
                        serde_json::from_value(input.manifest)
                            .unwrap_or_else(|error| panic!("{}: {error}", case.name))
                    };
                let validated = validate_manifest(raw)
                    .unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
                validated.artifact(&input.resource, &input.variant, input.platform)
                    .unwrap_or_else(|| panic!("{}: explicit artifact exists", case.name))
                    .render(ArtifactRenderOptions { version: input.version.as_deref(), source: input.source.as_ref() }, &input.limits.apply())
                    .map(|result| json!({ "artifact": result.artifact, "canonical_url": result.canonical_url }))
            }
        };
        let actual = match result {
            Ok(value) => json!({ "ok": value }),
            Err(error) => {
                codes.insert(error.code());
                json!({ "error": { "code": error.code(), "path": error.path(), "fields": error.fields() } })
            }
        };
        if actual != case.expect {
            failures.push(format!(
                "{}: expected {}, got {}",
                case.name, case.expect, actual
            ));
        } else {
            println!("PASS {}", case.name);
        }
    }
    for code in [
        "template.unknown_variable",
        "template.cycle",
        "template.depth_exceeded",
        "template.output_too_large",
        "template.invalid_literal",
        "url.invalid",
        "url.unsupported_scheme",
        "url.forbidden_component",
    ] {
        assert!(codes.contains(code), "corpus covers {code}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!("resolve: {count} cases");
}

#[test]
fn every_static_history_url_has_no_query_or_fragment() {
    let validated = validate_manifest(serde_json::from_str(SNAPSHOT).unwrap()).unwrap();
    let mut count = 0;
    for (id, resource) in &validated.manifest().resources {
        for variant in &resource.variants {
            for (platform, artifact) in &variant.artifacts {
                if let ResourceArtifactVersionTemplate::Static { releases, .. } = &artifact.version
                {
                    for release in releases {
                        let result = validated
                            .artifact(id, &variant.id, *platform)
                            .unwrap()
                            .render(
                                ArtifactRenderOptions {
                                    version: Some(&release.version),
                                    source: None,
                                },
                                &Limits::default(),
                            )
                            .unwrap_or_else(|error| {
                                panic!(
                                    "{id}/{}/{platform:?}/{}: {error}",
                                    variant.id, release.version
                                )
                            });
                        assert!(
                            !result.canonical_url.contains(['?', '#']),
                            "{}",
                            result.canonical_url
                        );
                        count += 1;
                    }
                }
            }
        }
    }
    assert!(count > 0);
    println!("rendered {count} static history URLs without query or fragment");
}
