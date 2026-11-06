use manifest_parser::{
    Limits, ParseError, RemoteChecksumParser, RemoteResponse, RemoteVersionParser,
};
use serde::Deserialize;
use serde_json::Value;

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
    expect: Expectation,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Expectation {
    Ok { ok: Value },
    Error { error: ExpectedError },
}

#[derive(Debug, Deserialize)]
struct ExpectedError {
    code: String,
    path: String,
    fields: Value,
}

#[derive(Deserialize)]
struct Response {
    body: String,
    headers: Vec<(String, String)>,
}

impl Response {
    fn as_remote(&self) -> RemoteResponse<'_> {
        RemoteResponse {
            body: &self.body,
            headers: &self.headers,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct LimitOverrides {
    body_bytes: Option<usize>,
    header_value_bytes: Option<usize>,
    regex_pattern_bytes: Option<usize>,
    regex_size_bytes: Option<usize>,
    template_depth: Option<usize>,
    render_output_bytes: Option<usize>,
}

impl LimitOverrides {
    fn apply(&self) -> Limits {
        let mut limits = Limits::default();
        if let Some(value) = self.body_bytes {
            limits = limits.with_body_bytes(value);
        }
        if let Some(value) = self.header_value_bytes {
            limits = limits.with_header_value_bytes(value);
        }
        if let Some(value) = self.regex_pattern_bytes {
            limits = limits.with_regex_pattern_bytes(value);
        }
        if let Some(value) = self.regex_size_bytes {
            limits = limits.with_regex_size_bytes(value);
        }
        if let Some(value) = self.template_depth {
            limits = limits.with_template_depth(value);
        }
        if let Some(value) = self.render_output_bytes {
            limits = limits.with_render_output_bytes(value);
        }
        limits
    }
}

#[derive(Deserialize)]
struct VersionInput {
    parser: RemoteVersionParser,
    response: Response,
    #[serde(default)]
    limits: LimitOverrides,
}

#[derive(Deserialize)]
struct ChecksumInput {
    parser: RemoteChecksumParser,
    response: Response,
    artifact: String,
    #[serde(default)]
    limits: LimitOverrides,
}

fn compare(result: Result<Value, ParseError>, expect: &Expectation) -> Result<(), String> {
    match (result, expect) {
        (Ok(actual), Expectation::Ok { ok }) if actual == *ok => Ok(()),
        (Err(actual), Expectation::Error { error }) => {
            if actual.code() != error.code {
                return Err(format!("expected code {}, got {actual:?}", error.code));
            }
            if actual.path() != error.path {
                return Err(format!(
                    "expected path {:?}, got {:?}",
                    error.path,
                    actual.path()
                ));
            }
            if actual.fields() != error.fields {
                return Err(format!(
                    "expected fields {}, got {}",
                    error.fields,
                    actual.fields()
                ));
            }
            Ok(())
        }
        (actual, expected) => Err(format!("expected {expected:?}, got {actual:?}")),
    }
}

fn run_cases(corpus: &str, execute: impl Fn(Value) -> Result<Value, ParseError>) {
    let cases: Vec<Case> = serde_json::from_str(corpus).expect("valid conformance corpus");
    assert!(!cases.is_empty(), "conformance corpus must contain cases");
    let mut failures = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    let mut executed = 0;
    for case in cases {
        if !names.insert(case.name.clone()) {
            failures.push(format!("{}: duplicate case name", case.name));
        }
        executed += 1;
        match compare(execute(case.input), &case.expect) {
            Ok(()) => {
                record_case(&case.name);
                println!("PASS {}", case.name);
            }
            Err(error) => failures.push(format!("{}: {error}", case.name)),
        }
    }
    assert!(executed > 0, "conformance runner must execute cases");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn corpus_case_names_are_globally_unique() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance");
    let mut pending = vec![root.clone()];
    let mut names = std::collections::BTreeMap::new();
    let mut failures = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("corpus directory exists") {
            let path = entry.expect("corpus directory entry").path();
            if path == root.join("fixtures") {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let corpus = std::fs::read_to_string(&path).expect("UTF-8 corpus");
            let cases: Vec<Case> = serde_json::from_str(&corpus)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            for case in cases {
                if let Some(previous) = names.insert(case.name.clone(), path.clone()) {
                    failures.push(format!(
                        "{}: duplicate case name in {} and {}",
                        case.name,
                        previous.display(),
                        path.display()
                    ));
                }
            }
        }
    }
    assert!(!names.is_empty(), "corpus must contain named cases");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn version() {
    run_cases(
        include_str!("../../../conformance/parsers/version.json"),
        |input| {
            let input: VersionInput = serde_json::from_value(input).expect("valid version input");
            let parser = input.parser.compile(&input.limits.apply())?;
            let version = parser.parse(&input.response.as_remote())?;
            Ok(Value::String(version.to_string()))
        },
    );
}

#[test]
fn checksum() {
    run_cases(
        include_str!("../../../conformance/parsers/checksum.json"),
        |input| {
            let input: ChecksumInput = serde_json::from_value(input).expect("valid checksum input");
            let checksum = input.parser.parse(
                &input.response.as_remote(),
                &input.artifact,
                &input.limits.apply(),
            )?;
            Ok(Value::String(checksum.to_string()))
        },
    );
}

#[test]
fn checksum_unsupported_parsers() {
    let response = RemoteResponse {
        body: "",
        headers: &[],
    };
    let limits = Limits::default();
    let parser: RemoteChecksumParser = serde_json::from_value(serde_json::json!({
        "type": "future-checksum",
        "options": { "enabled": true }
    }))
    .unwrap();
    let error = parser
        .parse(&response, "selected.zip", &limits)
        .unwrap_err();
    assert!(
        matches!(&error, ParseError::ResolveParser { parser_type } if parser_type == "future-checksum")
    );
    assert_eq!(error.code(), "resolve.unsupported_parser");
    assert_eq!(error.path(), "parser.type");
    assert_eq!(
        error.fields(),
        serde_json::json!({ "parser_type": "future-checksum" })
    );
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    checksum: Option<String>,
}

#[test]
fn github_expanded_assets_fixtures() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/fixtures/github-expanded-assets");
    let mut fixtures = 0;
    let mut with_digest = 0;
    let mut without_digest = 0;
    for file in std::fs::read_dir(root).expect("fixture directory exists") {
        let path = file.expect("fixture directory entry").path();
        if path.extension().is_none_or(|ext| ext != "html") {
            continue;
        }
        let body = std::fs::read_to_string(&path).expect("UTF-8 HTML fixture");
        let expected: Vec<GithubAsset> = serde_json::from_str(
            &std::fs::read_to_string(path.with_extension("expected.json"))
                .expect("independent expected asset list"),
        )
        .expect("valid expected asset list");
        assert!(!expected.is_empty(), "{}", path.display());
        fixtures += 1;
        let response = RemoteResponse {
            body: &body,
            headers: &[],
        };
        for asset in expected {
            let result = RemoteChecksumParser::GithubReleaseAssets.parse(
                &response,
                &asset.name,
                &Limits::default(),
            );
            match asset.checksum {
                Some(checksum) => {
                    with_digest += 1;
                    assert_eq!(
                        result
                            .map(|value| value.to_string())
                            .map_err(|error| error.to_string()),
                        Ok(checksum),
                        "{}: {}",
                        path.display(),
                        asset.name
                    );
                }
                None => {
                    without_digest += 1;
                    let error = result.expect_err("asset has no digest");
                    assert_eq!(
                        error.code(),
                        "parser.digest_missing",
                        "{}: {}",
                        path.display(),
                        asset.name
                    );
                    assert_eq!(error.path(), "response.body");
                    assert_eq!(error.fields(), serde_json::json!({ "name": asset.name }));
                }
            }
            record_case(&format!(
                "fixtures.github-expanded-assets.{}.{}",
                path.file_stem().unwrap().to_str().unwrap(),
                asset.name
            ));
        }
    }
    assert!(
        fixtures >= 4,
        "real pages and encoded fixture must be covered"
    );
    assert!(with_digest > 0, "fixtures must contain digests");
    assert!(
        without_digest > 0,
        "fixtures must contain assets without digests"
    );
}

#[test]
fn checksum_header_default_limits() {
    let limits = Limits::default();
    let parser = RemoteChecksumParser::Header {
        name: "X-Digest".into(),
        algorithm: manifest_parser::ChecksumAlgorithm::Sha256,
        encoding: manifest_parser::HeaderEncoding::Hex,
    };
    let body = "!".repeat(limits.body_bytes() + 1);
    let headers = vec![("x-digest".into(), "AB".repeat(32))];
    let response = RemoteResponse {
        body: &body,
        headers: &headers,
    };
    assert_eq!(
        parser
            .parse(&response, "unused", &limits)
            .unwrap()
            .to_string(),
        format!("sha256:{}", "ab".repeat(32))
    );
    for actual in [limits.header_value_bytes(), limits.header_value_bytes() + 1] {
        let headers = vec![("x-digest".into(), "g".repeat(actual))];
        let response = RemoteResponse {
            body: &body,
            headers: &headers,
        };
        let error = parser.parse(&response, "unused", &limits).unwrap_err();
        assert_eq!(error.path(), "response.headers.x-digest");
        if actual == limits.header_value_bytes() {
            assert_eq!(error.code(), "parser.invalid_checksum");
            assert_eq!(error.fields(), serde_json::json!({}));
        } else {
            assert_eq!(error.code(), "parser.body_too_large");
            assert_eq!(
                error.fields(),
                serde_json::json!({"limit": limits.header_value_bytes(), "actual": actual})
            );
        }
    }
}

#[test]
fn checksum_default_body_limit_precedes_dispatch() {
    let limits = Limits::default();
    let body = " ".repeat(limits.body_bytes());
    let response = RemoteResponse {
        body: &body,
        headers: &[],
    };
    for parser in [
        RemoteChecksumParser::Checksum,
        RemoteChecksumParser::Gnu {
            algorithm: manifest_parser::ChecksumAlgorithm::Sha256,
        },
        RemoteChecksumParser::Bsd {
            algorithm: manifest_parser::ChecksumAlgorithm::Sha256,
        },
        RemoteChecksumParser::GithubReleaseAssets,
        RemoteChecksumParser::Unsupported {
            parser_type: "future-checksum".into(),
        },
    ] {
        let error = parser
            .parse(&response, "selected.zip", &limits)
            .unwrap_err();
        assert_ne!(error.code(), "parser.body_too_large", "{parser:?}");
        let oversized = format!("{body} ");
        let response = RemoteResponse {
            body: &oversized,
            headers: &[],
        };
        let error = parser
            .parse(&response, "selected.zip", &limits)
            .unwrap_err();
        assert_eq!(error.code(), "parser.body_too_large", "{parser:?}");
        assert_eq!(error.path(), "response.body");
        assert_eq!(
            error.fields(),
            serde_json::json!({ "limit": limits.body_bytes(), "actual": oversized.len() })
        );
    }
}

#[test]
fn checksum_gnu_long_line() {
    let body = format!(
        "{} {}{}",
        "ab".repeat(32),
        " ".repeat(1_000_000),
        "x".repeat(1_000_000)
    );
    let limits = Limits::default();
    assert!(body.len() < limits.body_bytes());
    let response = RemoteResponse {
        body: &body,
        headers: &[],
    };
    let error = RemoteChecksumParser::Gnu {
        algorithm: manifest_parser::ChecksumAlgorithm::Sha256,
    }
    .parse(&response, "x", &limits)
    .unwrap_err();
    assert_eq!(error.code(), "parser.not_found");
    assert_eq!(error.path(), "response.body");
    assert_eq!(error.fields(), serde_json::json!({ "name": "x" }));
}
