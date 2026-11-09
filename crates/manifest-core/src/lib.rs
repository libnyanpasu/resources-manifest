use manifest_metadata::ResourceManifest;
use serde_json::{Value, json};
use snafu::Snafu;

mod kind;
mod plan;
mod render;
mod scope;
mod validate;
mod verify;

pub use kind::{
    ChecksumKind, VariantChecksumKind, artifact_checksum_kind, list_variant_checksum_kinds,
};
pub use plan::{
    ChecksumMode, ChecksumPlan, DownloadOptions, DownloadPlan, FinishVersionError,
    RemoteVersionPlan, RequestPlan, ResolveError, ResolvedChecksum, VariantSelection,
    VersionOptions, VersionPartPlan, VersionPlan, finish_checksum, finish_version, plan_download,
    plan_version, select_variant,
};
pub use verify::{VerifyError, VerifyPolicy, check_policy};

pub use render::{
    ArtifactRenderOptions, RenderError, RenderedArtifact, SelectedArtifact, TemplateValue,
    render_template,
};

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct RuntimeErrorShape<Fields> {
    pub code: String,
    pub path: String,
    pub message: String,
    pub fields: Fields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValidateIdsKind {
    Tag,
    Variant,
}

impl ValidateIdsKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tag => "tag",
            Self::Variant => "variant",
        }
    }
}

impl std::fmt::Display for ValidateIdsKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValidateIdsReason {
    Invalid,
    Duplicate,
}

impl ValidateIdsReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::Duplicate => "duplicate",
        }
    }
}

impl std::fmt::Display for ValidateIdsReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExpandTagReason {
    Unknown,
    Mismatch,
}

impl ExpandTagReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Mismatch => "mismatch",
        }
    }
}

impl std::fmt::Display for ExpandTagReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResolveDefaultReason {
    Missing,
    Unknown,
    UnsupportedPlatform,
}

impl ResolveDefaultReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Unknown => "unknown",
            Self::UnsupportedPlatform => "unsupported_platform",
        }
    }
}

impl std::fmt::Display for ResolveDefaultReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResolveReferenceKind {
    Registry,
}

impl ResolveReferenceKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Registry => "registry",
        }
    }
}

impl std::fmt::Display for ResolveReferenceKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReadHistoryReason {
    LatestMissing,
    Duplicate,
}

impl ReadHistoryReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LatestMissing => "latest_missing",
            Self::Duplicate => "duplicate",
        }
    }
}

impl std::fmt::Display for ReadHistoryReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VersionPartsReason {
    Count,
    Name,
    Unreferenced,
}

impl VersionPartsReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Name => "name",
            Self::Unreferenced => "unreferenced",
        }
    }
}

impl std::fmt::Display for VersionPartsReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
#[must_use]
pub struct ValidatedManifest {
    manifest: ResourceManifest,
}

impl ValidatedManifest {
    #[must_use]
    pub fn manifest(&self) -> &ResourceManifest {
        &self.manifest
    }

    #[must_use]
    pub fn newer_minor(&self) -> bool {
        self.manifest.manifest_version.is_newer_minor()
    }
}

#[derive(Debug, PartialEq, Snafu)]
#[snafu(display("manifest has {} validation violations", errors.len()))]
#[must_use]
pub struct ValidationErrors {
    errors: Vec<ValidationError>,
}

impl ValidationErrors {
    #[must_use = "validation violations must be inspected"]
    pub fn errors(&self) -> &[ValidationError] {
        &self.errors
    }
}

#[derive(Debug, Clone, PartialEq, Snafu)]
#[non_exhaustive]
#[must_use]
pub enum ValidationError {
    #[snafu(display("invalid {kind} ID {id:?} at {path}: {reason}"))]
    ValidateIds {
        path: String,
        kind: ValidateIdsKind,
        id: String,
        reason: ValidateIdsReason,
    },
    #[snafu(display("noncanonical tag key at {path}: expected {expected:?}, got {actual:?}"))]
    BuildTagKey {
        path: String,
        expected: String,
        actual: String,
    },
    #[snafu(display("inconsistent expanded tag {id:?} at {path}: {reason}"))]
    ExpandTag {
        path: String,
        id: String,
        reason: ExpandTagReason,
    },
    #[snafu(display("conflicting tags in group {group:?} at {path}"))]
    SelectGroup {
        path: String,
        group: String,
        tags: Vec<String>,
    },
    #[snafu(display("duplicate tag combination {tag_key:?} at {path}"))]
    SelectTagCombination {
        path: String,
        tag_key: String,
        first_variant: usize,
    },
    #[snafu(display("invalid default for {platform} at {path}: {reason}"))]
    ResolveDefault {
        path: String,
        platform: String,
        variant: Option<String>,
        reason: ResolveDefaultReason,
    },
    #[snafu(display("unknown {kind} reference {id:?} at {path}"))]
    ResolveReference {
        path: String,
        kind: ResolveReferenceKind,
        id: String,
    },
    #[snafu(display("invalid static history version {version:?} at {path}: {reason}"))]
    ReadHistory {
        path: String,
        version: String,
        reason: ReadHistoryReason,
    },
    #[snafu(display("cannot compile parser at {path}: {reason}"))]
    CompileParser {
        path: String,
        reason: &'static str,
        details: Value,
    },
    #[snafu(display("unknown template variable {variable:?} at {path}"))]
    ResolveVariable { path: String, variable: String },
    #[snafu(display("invalid composite version parts at {path}: {reason}"))]
    ValidateVersionParts {
        path: String,
        reason: VersionPartsReason,
        part: Option<String>,
    },
}

impl ValidationError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ValidateIds { .. } => "validate.ids",
            Self::BuildTagKey { .. } => "validate.tag_key",
            Self::ExpandTag { .. } => "validate.expanded_tag",
            Self::SelectGroup { .. } => "validate.tag_group",
            Self::SelectTagCombination { .. } => "validate.tag_combination",
            Self::ResolveDefault { .. } => "validate.default_variant",
            Self::ResolveReference { .. } => "validate.reference",
            Self::ReadHistory { .. } => "validate.static_history",
            Self::CompileParser { .. } => "validate.parser_compile",
            Self::ResolveVariable { .. } => "validate.template_variable",
            Self::ValidateVersionParts { .. } => "validate.version_parts",
        }
    }

    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::ValidateIds { path, .. }
            | Self::BuildTagKey { path, .. }
            | Self::ExpandTag { path, .. }
            | Self::SelectGroup { path, .. }
            | Self::SelectTagCombination { path, .. }
            | Self::ResolveDefault { path, .. }
            | Self::ResolveReference { path, .. }
            | Self::ReadHistory { path, .. }
            | Self::CompileParser { path, .. }
            | Self::ResolveVariable { path, .. }
            | Self::ValidateVersionParts { path, .. } => path,
        }
    }

    #[must_use]
    pub fn fields(&self) -> Value {
        match self {
            Self::ValidateIds {
                kind, id, reason, ..
            } => {
                json!({ "kind": kind.as_str(), "id": id, "reason": reason.as_str() })
            }
            Self::BuildTagKey {
                expected, actual, ..
            } => {
                json!({ "expected": expected, "actual": actual })
            }
            Self::ExpandTag { id, reason, .. } => json!({ "id": id, "reason": reason.as_str() }),
            Self::SelectGroup { group, tags, .. } => json!({ "group": group, "tags": tags }),
            Self::SelectTagCombination {
                tag_key,
                first_variant,
                ..
            } => {
                json!({ "tag_key": tag_key, "first_variant": first_variant })
            }
            Self::ResolveDefault {
                platform,
                variant,
                reason,
                ..
            } => {
                json!({ "platform": platform, "variant": variant, "reason": reason.as_str() })
            }
            Self::ResolveReference { kind, id, .. } => json!({ "kind": kind.as_str(), "id": id }),
            Self::ReadHistory {
                version, reason, ..
            } => {
                json!({ "version": version, "reason": reason.as_str() })
            }
            Self::CompileParser {
                reason, details, ..
            } => {
                json!({ "reason": reason, "details": details })
            }
            Self::ResolveVariable { variable, .. } => json!({ "variable": variable }),
            Self::ValidateVersionParts { reason, part, .. } => {
                json!({ "reason": reason.as_str(), "part": part })
            }
        }
    }
}

#[must_use = "manifest validation errors must be handled"]
pub fn validate_manifest(raw: ResourceManifest) -> Result<ValidatedManifest, ValidationErrors> {
    let errors = validate::collect(&raw);
    if errors.is_empty() {
        Ok(ValidatedManifest { manifest: raw })
    } else {
        Err(ValidationErrors { errors })
    }
}
