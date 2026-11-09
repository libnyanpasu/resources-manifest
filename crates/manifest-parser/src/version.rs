use std::fmt;
use std::str::FromStr;

use regex::{Regex, RegexBuilder};
use schemars::JsonSchema;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use specta::Type;

use crate::{Limits, ParseError, RemoteResponse};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionLabel(String);

impl FromStr for VersionLabel {
    type Err = ParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let bytes = value.as_bytes();
        if !(1..=128).contains(&bytes.len())
            || !bytes[0].is_ascii_alphanumeric()
            || !bytes[1..]
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(byte))
        {
            return Err(ParseError::ValidateVersion {});
        }
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<&str> for VersionLabel {
    type Error = ParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl fmt::Display for VersionLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for VersionLabel {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct CompiledVersionParser {
    parser: VersionParser,
    body_limit: usize,
}

#[derive(Debug, Clone)]
enum VersionParser {
    Text {
        trim: bool,
    },
    Regex {
        regex: Regex,
        group: usize,
    },
    Json {
        pointer: String,
        capture: Option<(Regex, usize)>,
    },
}

impl CompiledVersionParser {
    #[must_use = "version parsing errors must be handled"]
    pub fn parse(&self, response: &RemoteResponse<'_>) -> Result<VersionLabel, ParseError> {
        if response.body.len() > self.body_limit {
            return Err(ParseError::ValidateBodySize {
                limit: self.body_limit,
                actual: response.body.len(),
            });
        }
        match &self.parser {
            VersionParser::Text { trim } => {
                if *trim {
                    response.body.trim()
                } else {
                    response.body
                }
            }
            .parse(),
            VersionParser::Regex { regex, group } => {
                capture(regex, *group, response.body)?.trim().parse()
            }
            VersionParser::Json {
                pointer,
                capture: rule,
            } => {
                let document = decode_json(response.body)?;
                let value = document
                    .pointer(pointer)
                    .and_then(Value::as_str)
                    .ok_or(ParseError::FindVersion {})?;
                match rule {
                    Some((regex, group)) => capture(regex, *group, value)?,
                    None => value,
                }
                .trim()
                .parse()
            }
        }
    }
}

fn capture<'a>(regex: &Regex, group: usize, text: &'a str) -> Result<&'a str, ParseError> {
    Ok(regex
        .captures(text)
        .and_then(|captures| captures.get(group))
        .ok_or(ParseError::FindVersion {})?
        .as_str())
}

/// Compiles a version regex and resolves its capture group, defaulting to 1.
fn compile_capture(
    pattern: &str,
    group: Option<u32>,
    limits: &Limits,
) -> Result<(Regex, usize), ParseError> {
    if pattern.len() > limits.regex_pattern_bytes() {
        return Err(ParseError::ValidatePatternSize {
            limit: limits.regex_pattern_bytes(),
            actual: pattern.len(),
        });
    }
    let regex = RegexBuilder::new(pattern)
        .size_limit(limits.regex_size_bytes())
        .build()
        .map_err(|source| ParseError::CompilePattern { source })?;
    let group = group.unwrap_or(1);
    let groups = regex.captures_len();
    let index = usize::try_from(group)
        .ok()
        .filter(|&index| index < groups)
        .ok_or(ParseError::ValidateGroup { group, groups })?;
    Ok((regex, index))
}

/// Whether `pointer` is an RFC 6901 JSON Pointer: empty, or `/`-prefixed
/// with every `~` starting a `~0` or `~1` escape.
fn is_json_pointer(pointer: &str) -> bool {
    let bytes = pointer.as_bytes();
    (bytes.is_empty() || bytes[0] == b'/')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, &byte)| byte != b'~' || matches!(bytes.get(index + 1), Some(b'0' | b'1')))
}

/// Decodes a JSON response body, rejecting duplicate object keys so that a
/// pointer never picks one of several conflicting values.
fn decode_json(body: &str) -> Result<Value, ParseError> {
    struct Unique(Value);

    impl<'de> Deserialize<'de> for Unique {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_any(UniqueVisitor).map(Unique)
        }
    }

    struct UniqueVisitor;

    impl<'de> Visitor<'de> for UniqueVisitor {
        type Value = Value;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a JSON value")
        }

        fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
            Ok(Value::Bool(value))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
            Ok(Value::from(value))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
            Ok(Value::from(value))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
            Ok(Value::from(value))
        }

        fn visit_str<E>(self, value: &str) -> Result<Value, E> {
            Ok(Value::String(value.to_owned()))
        }

        fn visit_string<E>(self, value: String) -> Result<Value, E> {
            Ok(Value::String(value))
        }

        fn visit_unit<E>(self) -> Result<Value, E> {
            Ok(Value::Null)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
            let mut values = Vec::new();
            while let Some(Unique(value)) = seq.next_element()? {
                values.push(value);
            }
            Ok(Value::Array(values))
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
            let mut object = serde_json::Map::new();
            while let Some(key) = map.next_key::<String>()? {
                if object.contains_key(&key) {
                    return Err(serde::de::Error::custom(format!("duplicate key {key:?}")));
                }
                let Unique(value) = map.next_value()?;
                object.insert(key, value);
            }
            Ok(Value::Object(object))
        }
    }

    serde_json::from_str::<Unique>(body)
        .map(|Unique(value)| value)
        .map_err(|source| ParseError::DecodeJson { source })
}

pub(crate) fn deserialize_parser_object<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<serde_json::Value, D::Error> {
    struct ParserObjectVisitor;

    impl<'de> Visitor<'de> for ParserObjectVisitor {
        type Value = serde_json::Value;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a parser object")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut fields = serde_json::Map::new();
            while let Some(key) = map.next_key::<String>()? {
                if fields.contains_key(&key) {
                    return Err(serde::de::Error::custom(format!("duplicate field `{key}`")));
                }
                fields.insert(key, map.next_value()?);
            }
            Ok(serde_json::Value::Object(fields))
        }
    }

    deserializer.deserialize_map(ParserObjectVisitor)
}

/// How to parse a remote version response.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RemoteVersionParser {
    /// Use the response body, optionally trimming whitespace.
    Text {
        /// Defaults to true.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        trim: Option<bool>,
    },
    /// Apply a regular expression and return a capture group.
    Regex {
        pattern: String,
        /// Capture group containing the version. Defaults to 1. Deserializing
        /// negative or fractional values fails, matching the reference
        /// validator's non-negative integer rule.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        #[schemars(range(max = 4_294_967_295_u32))]
        group: Option<u32>,
    },
    /// Decode the body as JSON and read the string at an RFC 6901 pointer,
    /// optionally narrowed by a regular expression.
    Json {
        pointer: String,
        /// Applied to the string like the `regex` parser; without it the
        /// whole string is the version.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        pattern: Option<String>,
        /// Capture group of `pattern`. Defaults to 1; unused without `pattern`.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        #[schemars(range(max = 4_294_967_295_u32))]
        group: Option<u32>,
    },
    #[schemars(skip, !untagged)]
    #[specta(skip)]
    #[serde(untagged)]
    Unsupported {
        #[serde(rename = "type")]
        parser_type: String,
    },
}

impl RemoteVersionParser {
    #[must_use = "parser compilation errors must be handled"]
    pub fn compile(&self, limits: &Limits) -> Result<CompiledVersionParser, ParseError> {
        let parser = match self {
            Self::Text { trim } => VersionParser::Text {
                trim: trim.unwrap_or(true),
            },
            Self::Regex { pattern, group } => {
                let (regex, group) = compile_capture(pattern, *group, limits)?;
                VersionParser::Regex { regex, group }
            }
            Self::Json {
                pointer,
                pattern,
                group,
            } => {
                if !is_json_pointer(pointer) {
                    return Err(ParseError::ValidatePointer {});
                }
                VersionParser::Json {
                    pointer: pointer.clone(),
                    capture: pattern
                        .as_deref()
                        .map(|pattern| compile_capture(pattern, *group, limits))
                        .transpose()?,
                }
            }
            Self::Unsupported { parser_type } => {
                return Err(ParseError::ResolveParser {
                    parser_type: parser_type.clone(),
                });
            }
        };
        Ok(CompiledVersionParser {
            parser,
            body_limit: limits.body_bytes(),
        })
    }
}

impl<'de> Deserialize<'de> for RemoteVersionParser {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Known {
            Text {
                trim: Option<bool>,
            },
            Regex {
                pattern: String,
                group: Option<u32>,
            },
            Json {
                pointer: String,
                pattern: Option<String>,
                group: Option<u32>,
            },
        }

        let value = deserialize_parser_object(deserializer)?;
        let parser_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("parser type must be a string"))?;
        if !matches!(parser_type, "text" | "regex" | "json") {
            return Ok(Self::Unsupported {
                parser_type: parser_type.to_owned(),
            });
        }
        match Known::deserialize(value).map_err(serde::de::Error::custom)? {
            Known::Text { trim } => Ok(Self::Text { trim }),
            Known::Regex { pattern, group } => Ok(Self::Regex { pattern, group }),
            Known::Json {
                pointer,
                pattern,
                group,
            } => Ok(Self::Json {
                pointer,
                pattern,
                group,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RemoteChecksumParser;
    use serde_json::json;

    #[test]
    fn unknown_parser_tags_roundtrip_and_ignore_other_fields() {
        for tag in ["future-parser", "Future.Parser", "", "unsupported"] {
            let input =
                json!({"type": tag, "pattern": null, "trim": [], "algorithm": {"future": true}});
            let version: RemoteVersionParser = serde_json::from_value(input.clone()).unwrap();
            let checksum: RemoteChecksumParser = serde_json::from_value(input).unwrap();
            assert_eq!(
                version,
                RemoteVersionParser::Unsupported {
                    parser_type: tag.into()
                }
            );
            assert_eq!(
                checksum,
                RemoteChecksumParser::Unsupported {
                    parser_type: tag.into()
                }
            );
            let expected = json!({"type": tag});
            assert_eq!(serde_json::to_value(&version).unwrap(), expected);
            assert_eq!(serde_json::to_value(&checksum).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<RemoteVersionParser>(expected.clone()).unwrap(),
                version
            );
            assert_eq!(
                serde_json::from_value::<RemoteChecksumParser>(expected).unwrap(),
                checksum
            );
        }
    }

    #[test]
    fn parser_decoding_rejects_missing_tags_and_malformed_known_types() {
        for input in [
            json!({}),
            json!({"type": null}),
            json!({"type": 2}),
            json!({"type": []}),
            json!(null),
            json!([]),
        ] {
            assert!(
                serde_json::from_value::<RemoteVersionParser>(input.clone()).is_err(),
                "{input}"
            );
            assert!(
                serde_json::from_value::<RemoteChecksumParser>(input.clone()).is_err(),
                "{input}"
            );
        }
        for input in [
            json!({"type": "text", "trim": "true"}),
            json!({"type": "regex"}),
            json!({"type": "regex", "pattern": 42}),
            json!({"type": "regex", "pattern": "(.*)", "group": -1}),
            json!({"type": "regex", "pattern": "(.*)", "group": 0.5}),
            json!({"type": "regex", "pattern": "(.*)", "group": 4_294_967_296_u64}),
            json!({"type": "json"}),
            json!({"type": "json", "pointer": 1}),
            json!({"type": "json", "pointer": "/a", "pattern": 1}),
            json!({"type": "json", "pointer": "/a", "group": -1}),
        ] {
            assert!(
                serde_json::from_value::<RemoteVersionParser>(input.clone()).is_err(),
                "{input}"
            );
        }
        for input in [
            json!({"type": "gnu"}),
            json!({"type": "bsd"}),
            json!({"type": "gnu", "algorithm": "sha1"}),
            json!({"type": "bsd", "algorithm": 42}),
        ] {
            assert!(
                serde_json::from_value::<RemoteChecksumParser>(input.clone()).is_err(),
                "{input}"
            );
        }
        for input in [
            json!({"type": "text"}),
            json!({"type": "text", "trim": false}),
            json!({"type": "regex", "pattern": "(.*)", "group": 0}),
            json!({"type": "json", "pointer": ""}),
            json!({"type": "json", "pointer": "/a", "pattern": "(.*)", "group": 0}),
        ] {
            assert!(
                serde_json::from_value::<RemoteVersionParser>(input.clone()).is_ok(),
                "{input}"
            );
        }
        for input in [
            json!({"type": "checksum"}),
            json!({"type": "github-release-assets"}),
            json!({"type": "gnu", "algorithm": "sha256"}),
            json!({"type": "bsd", "algorithm": "sha512"}),
        ] {
            let parsed: RemoteChecksumParser = serde_json::from_value(input.clone()).unwrap();
            assert_eq!(serde_json::to_value(parsed).unwrap(), input);
        }
    }
}
