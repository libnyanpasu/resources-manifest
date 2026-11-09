use std::borrow::Cow;

use serde_json::{Value, json};
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum ParseError {
    #[snafu(display("response body exceeds {limit} bytes: {actual}"))]
    ValidateBodySize { limit: usize, actual: usize },
    #[snafu(display("regex pattern exceeds {limit} bytes: {actual}"))]
    ValidatePatternSize { limit: usize, actual: usize },
    #[snafu(display("cannot compile regex pattern: {source}"))]
    CompilePattern { source: regex::Error },
    #[snafu(display("capture group {group} is outside {groups} groups"))]
    ValidateGroup { group: u32, groups: usize },
    #[snafu(display("version was not found"))]
    FindVersion {},
    #[snafu(display("version does not satisfy the version label charset"))]
    ValidateVersion {},
    #[snafu(display("checksum is invalid"))]
    ValidateChecksum {},
    #[snafu(display("checksum for {name:?} was not found"))]
    FindChecksum { name: String },
    #[snafu(display("artifact {name:?} has no digest"))]
    ReadDigest { name: String },
    #[snafu(display("artifact {name:?} has conflicting digests"))]
    SelectChecksum { name: String },
    #[snafu(display("unsupported parser type {parser_type:?}"))]
    ResolveParser { parser_type: String },
    #[snafu(display("header {name:?} was not found"))]
    FindHeader { name: String },
    #[snafu(display("header {name:?} has conflicting values"))]
    SelectHeader { name: String },
    #[snafu(display("header {name:?} exceeds {limit} bytes: {actual}"))]
    ValidateHeaderSize {
        name: String,
        limit: usize,
        actual: usize,
    },
    #[snafu(display("cannot decode header {name:?}: {source}"))]
    DecodeHeader {
        name: String,
        source: base64::DecodeError,
    },
    #[snafu(display("checksum in header {name:?} is invalid"))]
    ValidateHeaderChecksum { name: String },
    #[snafu(display("response body is not valid JSON: {source}"))]
    DecodeJson { source: serde_json::Error },
    #[snafu(display("JSON pointer is invalid"))]
    ValidatePointer {},
}

impl ParseError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ValidateBodySize { .. } | Self::ValidateHeaderSize { .. } => {
                "parser.body_too_large"
            }
            Self::ValidatePatternSize { .. } | Self::CompilePattern { .. } => {
                "parser.invalid_pattern"
            }
            Self::ValidateGroup { .. } => "parser.group_out_of_range",
            Self::FindVersion { .. } | Self::FindHeader { .. } | Self::FindChecksum { .. } => {
                "parser.not_found"
            }
            Self::ValidateVersion { .. } => "parser.invalid_version",
            Self::ValidateChecksum { .. } | Self::ValidateHeaderChecksum { .. } => {
                "parser.invalid_checksum"
            }
            Self::DecodeHeader { .. } => "parser.invalid_encoding",
            Self::DecodeJson { .. } => "parser.invalid_json",
            Self::ValidatePointer { .. } => "parser.invalid_pointer",
            Self::ReadDigest { .. } => "parser.digest_missing",
            Self::ResolveParser { .. } => "resolve.unsupported_parser",
            Self::SelectHeader { .. } | Self::SelectChecksum { .. } => "parser.ambiguous",
        }
    }

    #[must_use]
    pub fn path(&self) -> Cow<'_, str> {
        match self {
            Self::ValidatePatternSize { .. } | Self::CompilePattern { .. } => {
                Cow::Borrowed("parser.pattern")
            }
            Self::ValidateGroup { .. } => Cow::Borrowed("parser.group"),
            Self::ValidatePointer { .. } => Cow::Borrowed("parser.pointer"),
            Self::ResolveParser { .. } => Cow::Borrowed("parser.type"),
            Self::FindHeader { name }
            | Self::SelectHeader { name }
            | Self::ValidateHeaderSize { name, .. }
            | Self::DecodeHeader { name, .. }
            | Self::ValidateHeaderChecksum { name } => {
                Cow::Owned(format!("response.headers.{}", name.to_ascii_lowercase()))
            }
            _ => Cow::Borrowed("response.body"),
        }
    }

    #[must_use]
    pub fn fields(&self) -> Value {
        match self {
            Self::ValidateBodySize { limit, actual }
            | Self::ValidateHeaderSize { limit, actual, .. }
            | Self::ValidatePatternSize { limit, actual } => {
                json!({ "limit": limit, "actual": actual })
            }
            Self::ValidateGroup { group, groups } => json!({ "group": group, "groups": groups }),
            Self::ResolveParser { parser_type } => json!({ "parser_type": parser_type }),
            Self::FindHeader { name }
            | Self::SelectHeader { name }
            | Self::FindChecksum { name }
            | Self::ReadDigest { name }
            | Self::SelectChecksum { name } => json!({ "name": name }),
            Self::CompilePattern { .. }
            | Self::FindVersion {}
            | Self::ValidateVersion {}
            | Self::ValidateChecksum {}
            | Self::DecodeHeader { .. }
            | Self::ValidateHeaderChecksum { .. }
            | Self::DecodeJson { .. }
            | Self::ValidatePointer {} => json!({}),
        }
    }
}
