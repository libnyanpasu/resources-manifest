use manifest_parser::{Limits, ParseError, RemoteResponse, RemoteVersionParser};
use serde::Deserialize;
use serde_json::Value;

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
    path: Option<String>,
    fields: Option<Value>,
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

fn compare(result: Result<Value, ParseError>, expect: &Expectation) -> Result<(), String> {
    match (result, expect) {
        (Ok(actual), Expectation::Ok { ok }) if actual == *ok => Ok(()),
        (Err(actual), Expectation::Error { error }) => {
            if actual.code() != error.code {
                return Err(format!("expected code {}, got {actual:?}", error.code));
            }
            if let Some(path) = &error.path
                && actual.path() != *path
            {
                return Err(format!("expected path {path:?}, got {:?}", actual.path()));
            }
            if let Some(fields) = &error.fields
                && actual.fields() != *fields
            {
                return Err(format!("expected fields {fields}, got {}", actual.fields()));
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
    for case in cases {
        if !names.insert(case.name.clone()) {
            failures.push(format!("{}: duplicate case name", case.name));
        }
        match compare(execute(case.input), &case.expect) {
            Ok(()) => println!("PASS {}", case.name),
            Err(error) => failures.push(format!("{}: {error}", case.name)),
        }
    }
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
