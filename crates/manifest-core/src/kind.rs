use manifest_metadata::{
    ResourceArtifactTemplate, ResourceArtifactVersionTemplate, SupportedPlatform,
};
use manifest_parser::{RemoteChecksumParser, RemoteVersionParser};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{ResolveError, ValidatedManifest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ChecksumKind {
    Authoritative,
    Parsed,
    Snapshot,
    None,
}

impl ChecksumKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authoritative => "authoritative",
            Self::Parsed => "parsed",
            Self::Snapshot => "snapshot",
            Self::None => "none",
        }
    }
}

impl std::fmt::Display for ChecksumKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[must_use]
pub fn artifact_checksum_kind(artifact: &ResourceArtifactTemplate) -> ChecksumKind {
    match &artifact.version {
        ResourceArtifactVersionTemplate::Static { latest, releases } => {
            if releases
                .iter()
                .any(|release| release.version == *latest && release.checksum.is_some())
            {
                ChecksumKind::Authoritative
            } else {
                ChecksumKind::None
            }
        }
        ResourceArtifactVersionTemplate::Remote {
            version, checksum, ..
        } => {
            if matches!(version.parser, RemoteVersionParser::Unsupported { .. }) {
                return ChecksumKind::None;
            }
            match checksum {
                Some(checksum)
                    if !matches!(checksum.parser, RemoteChecksumParser::Unsupported { .. }) =>
                {
                    ChecksumKind::Parsed
                }
                _ => ChecksumKind::None,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[non_exhaustive]
pub struct VariantChecksumKind {
    pub variant_id: String,
    pub kind: ChecksumKind,
}

#[must_use = "resource lookup errors must be handled"]
pub fn list_variant_checksum_kinds(
    manifest: &ValidatedManifest,
    resource_id: &str,
    platform: SupportedPlatform,
) -> Result<Vec<VariantChecksumKind>, ResolveError> {
    let resource = manifest
        .manifest()
        .resources
        .get(resource_id)
        .ok_or_else(|| ResolveError::FindResource {
            resource: resource_id.to_owned(),
        })?;
    Ok(resource
        .variants
        .iter()
        .filter_map(|variant| {
            variant
                .artifacts
                .get(&platform)
                .map(|artifact| VariantChecksumKind {
                    variant_id: variant.id.clone(),
                    kind: artifact_checksum_kind(artifact),
                })
        })
        .collect())
}
