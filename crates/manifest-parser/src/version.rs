use std::fmt;

use schemars::JsonSchema;
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

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
    #[schemars(skip, !untagged)]
    #[specta(skip)]
    #[serde(untagged)]
    Unsupported {
        #[serde(rename = "type")]
        parser_type: String,
    },
}

impl<'de> Deserialize<'de> for RemoteVersionParser {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Known {
            Text { trim: Option<bool> },
            Regex { pattern: String, group: Option<u32> },
        }

        let value = deserialize_parser_object(deserializer)?;
        let parser_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("parser type must be a string"))?;
        if !matches!(parser_type, "text" | "regex") {
            return Ok(Self::Unsupported {
                parser_type: parser_type.to_owned(),
            });
        }
        match Known::deserialize(value).map_err(serde::de::Error::custom)? {
            Known::Text { trim } => Ok(Self::Text { trim }),
            Known::Regex { pattern, group } => Ok(Self::Regex { pattern, group }),
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
