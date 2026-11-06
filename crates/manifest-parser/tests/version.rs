use std::collections::BTreeSet;

use manifest_parser::{Limits, RemoteResponse, RemoteVersionParser, VersionLabel};
use serde_json::{Value, json};

#[test]
fn version_label_validates_without_normalizing() {
    for value in ["a", "0", "v1.2.3", "0.10.7-alpha+sha.85fc166", "A_B.c+-9"] {
        let version: VersionLabel = value.parse().unwrap();
        assert_eq!(version.as_ref(), value);
        assert_eq!(version.to_string(), value);
        assert_eq!(VersionLabel::try_from(value).unwrap(), version);
    }
    let longest = format!("a{}", "-".repeat(127));
    assert!(longest.parse::<VersionLabel>().is_ok());
    assert!(format!("{longest}a").parse::<VersionLabel>().is_err());
    for value in [
        "", ".a", "_a", "+a", "-a", " a", "a ", "a\n", "a/b", "a?b", "a#b", "a{b}", "a%20", "a\0",
        "版本", "aé",
    ] {
        let error = value.parse::<VersionLabel>().unwrap_err();
        assert_eq!(error.code(), "parser.invalid_version", "{value:?}");
    }
}

#[test]
fn limits_can_only_be_lowered() {
    let defaults = Limits::default();
    assert_eq!(Limits::new(), defaults);
    let raised = defaults
        .with_body_bytes(usize::MAX)
        .with_header_value_bytes(usize::MAX)
        .with_regex_pattern_bytes(usize::MAX)
        .with_regex_size_bytes(usize::MAX)
        .with_template_depth(usize::MAX)
        .with_render_output_bytes(usize::MAX);
    assert_eq!(raised, defaults);
    assert_eq!(defaults.body_bytes(), 2 * 1024 * 1024);
    assert_eq!(defaults.header_value_bytes(), 8 * 1024);
    assert_eq!(defaults.regex_pattern_bytes(), 1024);
    assert_eq!(defaults.regex_size_bytes(), 1024 * 1024);
    assert_eq!(defaults.template_depth(), 8);
    assert_eq!(defaults.render_output_bytes(), 8 * 1024);
    let lower = defaults
        .with_body_bytes(10)
        .with_header_value_bytes(11)
        .with_regex_pattern_bytes(12)
        .with_regex_size_bytes(13)
        .with_template_depth(2)
        .with_render_output_bytes(14);
    assert_eq!(lower.body_bytes(), 10);
    assert_eq!(lower.header_value_bytes(), 11);
    assert_eq!(lower.regex_pattern_bytes(), 12);
    assert_eq!(lower.regex_size_bytes(), 13);
    assert_eq!(lower.template_depth(), 2);
    assert_eq!(lower.render_output_bytes(), 14);
    assert_eq!(
        lower
            .with_body_bytes(100)
            .with_header_value_bytes(100)
            .with_regex_pattern_bytes(100)
            .with_regex_size_bytes(100)
            .with_template_depth(100)
            .with_render_output_bytes(100),
        lower
    );
    assert_eq!(lower.with_body_bytes(0).body_bytes(), 0);
}

#[test]
fn headers_preserve_values_and_collapse_identical_repeats() {
    let headers = vec![
        ("X-Version".into(), "v1".into()),
        ("unrelated".into(), "v2".into()),
        ("x-version".into(), "v1".into()),
    ];
    let response = RemoteResponse {
        body: "",
        headers: &headers,
    };
    assert_eq!(
        response.header_values("X-VERSION").collect::<Vec<_>>(),
        ["v1", "v1"]
    );
    assert_eq!(response.single_header("X-VERSION").unwrap(), "v1");
    let error = response.single_header("X-Missing").unwrap_err();
    assert_eq!(error.code(), "parser.not_found");
    assert_eq!(error.path(), "response.headers.x-missing");
    assert_eq!(error.fields(), json!({"name": "X-Missing"}));
    let headers = vec![
        ("X-Version".into(), "v1".into()),
        ("x-version".into(), "V1".into()),
    ];
    let response = RemoteResponse {
        body: "",
        headers: &headers,
    };
    let error = response.single_header("X-Version").unwrap_err();
    assert_eq!(error.code(), "parser.ambiguous");
    assert_eq!(error.path(), "response.headers.x-version");
    assert_eq!(error.fields(), json!({"name": "X-Version"}));
}

#[test]
fn default_body_limit_is_checked_before_trimming_or_matching() {
    let limits = Limits::default();
    let mut body = " ".repeat(limits.body_bytes() - 2);
    body.push_str("v1");
    for parser in [
        RemoteVersionParser::Text { trim: None },
        RemoteVersionParser::Regex {
            pattern: "(v1)".into(),
            group: None,
        },
    ] {
        let compiled = parser.compile(&limits).unwrap();
        assert_eq!(
            compiled
                .parse(&RemoteResponse {
                    body: &body,
                    headers: &[]
                })
                .unwrap()
                .as_ref(),
            "v1"
        );
        let oversized = format!("{body}\n");
        let error = compiled
            .parse(&RemoteResponse {
                body: &oversized,
                headers: &[],
            })
            .unwrap_err();
        assert_eq!(error.code(), "parser.body_too_large");
        assert_eq!(error.path(), "response.body");
        assert_eq!(
            error.fields(),
            json!({"limit": 2 * 1024 * 1024, "actual": 2 * 1024 * 1024 + 1})
        );
    }
}

#[test]
fn default_pattern_and_compilation_limits_are_enforced() {
    let limits = Limits::default();
    let at_limit = RemoteVersionParser::Regex {
        pattern: "v".repeat(limits.regex_pattern_bytes()),
        group: Some(0),
    };
    assert!(at_limit.compile(&limits).is_ok());
    let over_limit = RemoteVersionParser::Regex {
        pattern: "v".repeat(limits.regex_pattern_bytes() + 1),
        group: Some(0),
    };
    let error = over_limit.compile(&limits).unwrap_err();
    assert_eq!(error.code(), "parser.invalid_pattern");
    assert_eq!(error.fields(), json!({"limit": 1024, "actual": 1025}));
    let expanded = RemoteVersionParser::Regex {
        pattern: "(a{1000}){1000}".into(),
        group: None,
    };
    let error = expanded.compile(&limits).unwrap_err();
    assert_eq!(error.code(), "parser.invalid_pattern");
    assert!(std::error::Error::source(&error).is_some());
}

fn collect_versions(value: &Value, versions: &mut BTreeSet<String>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "version" | "latest")
                    && let Some(version) = value.as_str()
                {
                    versions.insert(version.to_owned());
                }
                collect_versions(value, versions);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_versions(value, versions);
            }
        }
        _ => {}
    }
}

#[test]
fn every_reference_version_satisfies_version_label() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../manifest-metadata/tests/fixtures/resources.json"
    ))
    .unwrap();
    let mut versions = BTreeSet::new();
    collect_versions(&fixture, &mut versions);
    assert!(
        !versions.is_empty(),
        "reference fixture must contain versions"
    );
    for version in &versions {
        assert!(version.parse::<VersionLabel>().is_ok(), "{version:?}");
    }
    println!(
        "validated {} distinct reference versions: {versions:?}",
        versions.len()
    );
}
