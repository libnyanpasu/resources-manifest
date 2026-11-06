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
    #[snafu(display("unsupported parser type {parser_type:?}"))]
    ResolveParser { parser_type: String },
    #[snafu(display("header {name:?} was not found"))]
    FindHeader { name: String },
    #[snafu(display("header {name:?} has conflicting values"))]
    SelectHeader { name: String },
}

impl ParseError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ValidateBodySize { .. } => "parser.body_too_large",
            Self::ValidatePatternSize { .. } | Self::CompilePattern { .. } => {
                "parser.invalid_pattern"
            }
            Self::ValidateGroup { .. } => "parser.group_out_of_range",
            Self::FindVersion { .. } | Self::FindHeader { .. } => "parser.not_found",
            Self::ValidateVersion { .. } => "parser.invalid_version",
            Self::ResolveParser { .. } => "resolve.unsupported_parser",
            Self::SelectHeader { .. } => "parser.ambiguous",
        }
    }

    #[must_use]
    pub fn path(&self) -> Cow<'_, str> {
        match self {
            Self::FindHeader { name } | Self::SelectHeader { name } => {
                Cow::Owned(format!("response.headers.{}", name.to_ascii_lowercase()))
            }
            _ => Cow::Borrowed("response.body"),
        }
    }

    #[must_use]
    pub fn fields(&self) -> Value {
        match self {
            Self::ValidateBodySize { limit, actual }
            | Self::ValidatePatternSize { limit, actual } => {
                json!({ "limit": limit, "actual": actual })
            }
            Self::ValidateGroup { group, groups } => json!({ "group": group, "groups": groups }),
            Self::ResolveParser { parser_type } => json!({ "parser_type": parser_type }),
            Self::FindHeader { name } | Self::SelectHeader { name } => json!({ "name": name }),
            Self::CompilePattern { .. } | Self::FindVersion {} | Self::ValidateVersion {} => {
                json!({})
            }
        }
    }
}
