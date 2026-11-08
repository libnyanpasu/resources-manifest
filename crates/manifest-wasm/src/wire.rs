//! Wire types that exist only at the wasm boundary.
//!
//! Core option types (`VariantSelection`, `VersionOptions`, `DownloadOptions`)
//! and parser responses borrow their inputs, and `Limits` keeps a private
//! lower-only invariant, so none of them deserialize. The boundary decodes the
//! fields declared here and then builds the borrowed core inputs.
//! `manifest-schema-generator` exports these shapes into `@manifest/runtime`'s
//! `src/types.ts`.

use manifest_parser::Limits;
use serde::{Deserialize, Serialize};
use specta::Type;
use wasm_bindgen::JsValue;

use crate::bridge::Fields;

/// Optional overrides that can only lower the default parser and render limits
/// (design §5.5).
#[derive(Debug, Default, Type)]
pub struct LimitsInput {
    #[specta(optional)]
    pub body_bytes: Option<u32>,
    #[specta(optional)]
    pub header_value_bytes: Option<u32>,
    #[specta(optional)]
    pub regex_pattern_bytes: Option<u32>,
    #[specta(optional)]
    pub regex_size_bytes: Option<u32>,
    #[specta(optional)]
    pub template_depth: Option<u32>,
    #[specta(optional)]
    pub render_output_bytes: Option<u32>,
}

impl LimitsInput {
    /// Decodes the optional `limits` argument; `null` and `undefined` keep the
    /// defaults.
    pub(crate) fn decode(value: &JsValue) -> Result<Limits, JsValue> {
        let mut fields = Fields::optional(value, "arg.limits")?;
        let input = Self {
            body_bytes: fields.take("body_bytes")?,
            header_value_bytes: fields.take("header_value_bytes")?,
            regex_pattern_bytes: fields.take("regex_pattern_bytes")?,
            regex_size_bytes: fields.take("regex_size_bytes")?,
            template_depth: fields.take("template_depth")?,
            render_output_bytes: fields.take("render_output_bytes")?,
        };
        Ok(input.into_limits())
    }

    fn into_limits(self) -> Limits {
        // `usize` is at least 32 bits on every supported target.
        let bytes = |value: u32| usize::try_from(value).expect("u32 fits in usize");
        let mut limits = Limits::default();
        if let Some(value) = self.body_bytes {
            limits = limits.with_body_bytes(bytes(value));
        }
        if let Some(value) = self.header_value_bytes {
            limits = limits.with_header_value_bytes(bytes(value));
        }
        if let Some(value) = self.regex_pattern_bytes {
            limits = limits.with_regex_pattern_bytes(bytes(value));
        }
        if let Some(value) = self.regex_size_bytes {
            limits = limits.with_regex_size_bytes(bytes(value));
        }
        if let Some(value) = self.template_depth {
            limits = limits.with_template_depth(bytes(value));
        }
        if let Some(value) = self.render_output_bytes {
            limits = limits.with_render_output_bytes(bytes(value));
        }
        limits
    }
}

/// A fetched response handed to a version or checksum parser.
///
/// Each header entry is one header line or a client-combined comma list;
/// duplicates and order are kept (design §5.3).
#[derive(Debug, Type)]
pub struct RemoteResponseInput {
    pub body: String,
    #[specta(optional)]
    pub headers: Option<Vec<(String, String)>>,
}

impl RemoteResponseInput {
    /// Decodes a `response` argument.
    pub(crate) fn decode(value: &JsValue) -> Result<Self, JsValue> {
        let mut fields = Fields::object(value, "response")?;
        Ok(Self {
            body: fields.require("body")?,
            headers: fields.take("headers")?,
        })
    }

    pub(crate) fn headers(&self) -> &[(String, String)] {
        self.headers.as_deref().unwrap_or_default()
    }
}

/// A template variable value: `authored` values expand recursively, `literal`
/// values are inserted verbatim.
#[derive(Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TemplateValue {
    Authored(String),
    Literal(String),
}

impl TemplateValue {
    pub(crate) fn as_core(&self) -> manifest_core::TemplateValue<'_> {
        match self {
            Self::Authored(value) => manifest_core::TemplateValue::Authored(value),
            Self::Literal(value) => manifest_core::TemplateValue::Literal(value),
        }
    }
}

/// A rendered artifact name and its canonical download URL.
#[derive(Debug, Serialize, Type)]
pub struct RenderedArtifact {
    pub artifact: String,
    pub canonical_url: String,
}
