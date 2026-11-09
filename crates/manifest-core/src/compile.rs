//! Compiles an authored template and a release snapshot into a manifest
//! (design §9.6).
//!
//! Compilation runs in three stages, each only when the previous one passed:
//! binding template artifacts to snapshot release sources, validating the
//! result, and preflighting every plan the result advertises. Errors carry a
//! path rooted at the document they describe: `template.`, `snapshot.`, or
//! `output.` for the compiled manifest.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use manifest_metadata::{
    AssetSnapshot, ManifestSnapshot, ManifestVersion, ReleaseSnapshot, ReleaseSourceSnapshot,
    ResourceArtifactTemplate, ResourceArtifactVersionTemplate, ResourceDefinition,
    ResourceManifest, ResourceManifestTemplate, ResourceStaticReleaseTemplate, ResourceTag,
    ResourceTemplate, ResourceVariant, SupportedPlatform, TemplateArtifactVersion,
    TemplateReleaseSource,
};
use manifest_parser::Limits;
use serde_json::{Value, json};
use snafu::Snafu;

use crate::plan::{Mirror, plan_download_with, plan_version_with};
use crate::{
    ArtifactRenderOptions, ChecksumMode, DownloadOptions, RenderError, ResolveError,
    ValidatedManifest, ValidationError, validate_manifest,
};

/// The version a remote artifact without a fallback is preflighted with.
const PLACEHOLDER_VERSION: &str = "0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReleaseSourceReason {
    /// The artifact names a release source the resource does not declare.
    Unknown,
    /// A static artifact names a tag source, or a remote one a history source.
    Kind,
}

impl ReleaseSourceReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Kind => "kind",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SnapshotReason {
    /// The snapshot has no records for a declared release source.
    Missing,
    /// The snapshot records a different kind of source than the template.
    Kind,
}

impl SnapshotReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Kind => "kind",
        }
    }
}

#[derive(Debug, Snafu)]
#[non_exhaustive]
#[must_use]
pub enum CompileError {
    #[snafu(display("artifact release source {releases:?} at {path}: {}", reason.as_str()))]
    BindReleaseSource {
        path: String,
        releases: String,
        reason: ReleaseSourceReason,
    },
    #[snafu(display("snapshot release source at {path}: {}", reason.as_str()))]
    FindSnapshot {
        path: String,
        reason: SnapshotReason,
    },
    #[snafu(display("snapshot history at {path} has no releases"))]
    ReadHistory { path: String },
    #[snafu(display("unknown tag {tag:?} at {path}"))]
    ExpandTag { path: String, tag: String },
    #[snafu(display("release {release:?} has no asset {artifact:?} for {path}"))]
    FindArtifact {
        path: String,
        release: String,
        artifact: String,
    },
    #[snafu(display("invalid output at {path}: {source}"))]
    ValidateOutput {
        path: String,
        source: Box<ValidationError>,
    },
    #[snafu(display("cannot render an artifact name at {path}: {source}"))]
    RenderName { path: String, source: RenderError },
    #[snafu(display("cannot plan the output at {path}: {source}"))]
    Preflight {
        path: String,
        source: Box<ResolveError>,
    },
}

impl CompileError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::BindReleaseSource { .. } => "compile.release_source",
            Self::FindSnapshot { .. } => "compile.snapshot_missing",
            Self::ReadHistory { .. } => "compile.history_empty",
            Self::ExpandTag { .. } => "compile.unknown_tag",
            Self::FindArtifact { .. } => "compile.artifact_missing",
            Self::ValidateOutput { source, .. } => source.code(),
            Self::RenderName { source, .. } => source.code(),
            Self::Preflight { source, .. } => source.code(),
        }
    }

    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::BindReleaseSource { path, .. }
            | Self::FindSnapshot { path, .. }
            | Self::ReadHistory { path }
            | Self::ExpandTag { path, .. }
            | Self::FindArtifact { path, .. }
            | Self::ValidateOutput { path, .. }
            | Self::RenderName { path, .. }
            | Self::Preflight { path, .. } => path,
        }
    }

    #[must_use]
    pub fn fields(&self) -> Value {
        match self {
            Self::BindReleaseSource {
                releases, reason, ..
            } => json!({ "releases": releases, "reason": reason.as_str() }),
            Self::FindSnapshot { reason, .. } => json!({ "reason": reason.as_str() }),
            Self::ReadHistory { .. } => json!({}),
            Self::ExpandTag { tag, .. } => json!({ "tag": tag }),
            Self::FindArtifact {
                release, artifact, ..
            } => json!({ "release": release, "artifact": artifact }),
            Self::ValidateOutput { source, .. } => source.fields(),
            Self::RenderName { source, .. } => source.fields(),
            Self::Preflight { source, .. } => source.fields(),
        }
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("manifest compilation failed with {} errors", errors.len()))]
#[must_use]
pub struct CompileErrors {
    errors: Vec<CompileError>,
}

impl CompileErrors {
    #[must_use = "compilation errors must be inspected"]
    pub fn errors(&self) -> &[CompileError] {
        &self.errors
    }
}

/// Compiles `template` with the releases captured in `snapshot`; `now` is
/// written verbatim as `updated_at`.
///
/// A snapshot whose tag sources lack an observed version yields a provisional
/// manifest whose remote artifacts have no fallback; it serves only to plan the
/// version requests that observe those versions.
#[must_use = "compilation errors must be handled"]
pub fn compile(
    template: &ResourceManifestTemplate,
    snapshot: &ManifestSnapshot,
    now: &str,
) -> Result<ResourceManifest, CompileErrors> {
    let mut errors = Vec::new();
    let provisional = expand(template, snapshot, now, &mut errors);
    failed(errors)?;
    let validated = validate(provisional)?;
    let mut errors = Vec::new();
    let output = bind(template, snapshot, &validated, &mut errors);
    failed(errors)?;
    let validated = validate(output)?;
    let mut errors = Vec::new();
    preflight(&validated, &mut errors);
    failed(errors)?;
    Ok(validated.manifest().clone())
}

/// Fails with `errors` unless it is empty. Artifacts that share an input
/// report its error once.
fn failed(errors: Vec<CompileError>) -> Result<(), CompileErrors> {
    if errors.is_empty() {
        return Ok(());
    }
    let mut seen = BTreeSet::new();
    let errors = errors
        .into_iter()
        .filter(|error| {
            seen.insert((
                error.code(),
                error.path().to_owned(),
                error.fields().to_string(),
            ))
        })
        .collect();
    Err(CompileErrors { errors })
}

fn validate(manifest: ResourceManifest) -> Result<ValidatedManifest, CompileErrors> {
    validate_manifest(manifest).map_err(|source| CompileErrors {
        errors: source
            .errors()
            .iter()
            .map(|error| CompileError::ValidateOutput {
                path: format!("output.{}", error.path()),
                source: Box::new(error.clone()),
            })
            .collect(),
    })
}

/// A release source resolved against the snapshot.
enum Source<'a> {
    History(&'a [ReleaseSnapshot]),
    Tag {
        version: Option<&'a str>,
        release: &'a ReleaseSnapshot,
    },
}

/// Resolves every declared release source of `resource` once, reporting
/// snapshot errors; failed sources are absent from the result.
fn sources<'a>(
    resource_id: &str,
    resource: &ResourceTemplate,
    snapshot: &'a ManifestSnapshot,
    errors: &mut Vec<CompileError>,
) -> BTreeMap<String, Source<'a>> {
    let mut resolved = BTreeMap::new();
    let captured = snapshot.resources.get(resource_id);
    for (id, declared) in &resource.releases {
        let path = format!("snapshot.resources.{resource_id}.{id}");
        let source = match (declared, captured.and_then(|sources| sources.get(id))) {
            (_, None) => Err(SnapshotReason::Missing),
            (
                TemplateReleaseSource::History { limit },
                Some(ReleaseSourceSnapshot::History { releases }),
            ) => {
                if releases.is_empty() {
                    errors.push(CompileError::ReadHistory {
                        path: format!("{path}.releases"),
                    });
                    continue;
                }
                let limit = usize::try_from(limit.get()).expect("history limit fits usize");
                Ok(Source::History(&releases[..releases.len().min(limit)]))
            }
            (
                TemplateReleaseSource::Tag { .. },
                Some(ReleaseSourceSnapshot::Tag { version, release }),
            ) => Ok(Source::Tag {
                version: version.as_deref(),
                release,
            }),
            _ => Err(SnapshotReason::Kind),
        };
        match source {
            Ok(source) => {
                resolved.insert(id.clone(), source);
            }
            Err(reason) => errors.push(CompileError::FindSnapshot { path, reason }),
        }
    }
    resolved
}

/// Builds the provisional manifest: tags expanded, static history listing
/// every captured release without checksums, and remote fallbacks carrying
/// only the observed version. Artifact names are rendered from it in [`bind`].
fn expand(
    template: &ResourceManifestTemplate,
    snapshot: &ManifestSnapshot,
    now: &str,
    errors: &mut Vec<CompileError>,
) -> ResourceManifest {
    let mut resources = BTreeMap::new();
    for (resource_id, resource) in &template.resources {
        let path = format!("template.resources.{resource_id}");
        let sources = sources(resource_id, resource, snapshot, errors);
        let catalog: Vec<ResourceTag> = resource
            .tags
            .iter()
            .map(|(id, tag)| ResourceTag {
                id: id.clone(),
                name: tag.name.clone(),
                group: tag.group.clone(),
                description: tag.description.clone(),
            })
            .collect();
        let mut variants = Vec::new();
        for (index, variant) in resource.variants.iter().enumerate() {
            let path = format!("{path}.variants[{index}]");
            let mut tags = Vec::new();
            for id in &variant.tags {
                match catalog.iter().find(|tag| tag.id == *id) {
                    Some(tag) => tags.push(tag.clone()),
                    None => errors.push(CompileError::ExpandTag {
                        path: format!("{path}.tags"),
                        tag: id.clone(),
                    }),
                }
            }
            let mut ids: Vec<&str> = variant.tags.iter().map(String::as_str).collect();
            ids.sort_unstable();
            let tag_key = ids.join("+");
            let mut artifacts = BTreeMap::new();
            for (platform, artifact) in &variant.artifacts {
                let path = format!("{path}.artifacts.{}.version", platform.as_str());
                let (releases, remote) = match &artifact.version {
                    TemplateArtifactVersion::Static { releases } => (releases, false),
                    TemplateArtifactVersion::Remote { releases, .. } => (releases, true),
                };
                let reason = match (resource.releases.get(releases), remote) {
                    (None, _) => Some(ReleaseSourceReason::Unknown),
                    (Some(TemplateReleaseSource::History { .. }), false)
                    | (Some(TemplateReleaseSource::Tag { .. }), true) => None,
                    _ => Some(ReleaseSourceReason::Kind),
                };
                if let Some(reason) = reason {
                    errors.push(CompileError::BindReleaseSource {
                        path: format!("{path}.releases"),
                        releases: releases.clone(),
                        reason,
                    });
                    continue;
                }
                let version = match (&artifact.version, sources.get(releases)) {
                    (TemplateArtifactVersion::Static { .. }, Some(Source::History(history))) => {
                        ResourceArtifactVersionTemplate::Static {
                            latest: history[0].tag.clone(),
                            releases: history
                                .iter()
                                .map(|release| ResourceStaticReleaseTemplate {
                                    version: release.tag.clone(),
                                    checksum: None,
                                    published_at: None,
                                    source: None,
                                })
                                .collect(),
                        }
                    }
                    (
                        TemplateArtifactVersion::Remote {
                            version, checksum, ..
                        },
                        Some(Source::Tag {
                            version: observed, ..
                        }),
                    ) => ResourceArtifactVersionTemplate::Remote {
                        fallback: observed.map(|observed| ResourceStaticReleaseTemplate {
                            version: observed.to_owned(),
                            checksum: None,
                            published_at: None,
                            source: None,
                        }),
                        version: version.clone(),
                        checksum: checksum.clone(),
                    },
                    // The snapshot error is already reported.
                    _ => continue,
                };
                artifacts.insert(
                    *platform,
                    ResourceArtifactTemplate {
                        artifact: artifact.artifact.clone(),
                        source: artifact.source.clone(),
                        version,
                    },
                );
            }
            variants.push(ResourceVariant {
                id: variant.id.clone().unwrap_or_else(|| tag_key.clone()),
                tag_key,
                name: variant.name.clone(),
                tags,
                source: variant.source.clone(),
                artifacts,
            });
        }
        resources.insert(
            resource_id.clone(),
            ResourceDefinition {
                name: resource.name.clone(),
                registry: resource.registry.clone(),
                source: resource.source.clone(),
                tags: catalog,
                default_variants: defaults(resource, &variants),
                variants,
            },
        );
    }
    ResourceManifest {
        manifest_version: ManifestVersion::current(),
        registries: template.registries.clone(),
        mirrors: template.mirrors.clone().unwrap_or_default(),
        resources,
        updated_at: Some(now.to_owned()),
    }
}

/// Authored defaults, plus the only variant of a platform that has one.
/// Platforms with several variants and no default are left for validation.
fn defaults(
    resource: &ResourceTemplate,
    variants: &[ResourceVariant],
) -> BTreeMap<SupportedPlatform, String> {
    let mut defaults = resource.default_variants.clone().unwrap_or_default();
    let mut supported: BTreeMap<SupportedPlatform, Vec<&str>> = BTreeMap::new();
    for variant in variants {
        for platform in variant.artifacts.keys() {
            supported.entry(*platform).or_default().push(&variant.id);
        }
    }
    for (platform, ids) in supported {
        if let [id] = ids.as_slice() {
            defaults.entry(platform).or_insert_with(|| (*id).to_owned());
        }
    }
    defaults
}

/// Renders every artifact name from the provisional manifest, looks the names
/// up in the captured assets, and returns the final manifest.
fn bind(
    template: &ResourceManifestTemplate,
    snapshot: &ManifestSnapshot,
    provisional: &ValidatedManifest,
    errors: &mut Vec<CompileError>,
) -> ResourceManifest {
    let limits = Limits::default();
    let mut output = provisional.manifest().clone();
    for (resource_id, resource) in &mut output.resources {
        let declared = &template.resources[resource_id];
        let sources = sources(resource_id, declared, snapshot, &mut Vec::new());
        for (index, variant) in resource.variants.iter_mut().enumerate() {
            let authored = &declared.variants[index];
            for (platform, artifact) in &mut variant.artifacts {
                let path = format!(
                    "template.resources.{resource_id}.variants[{index}].artifacts.{}",
                    platform.as_str()
                );
                let selected = provisional
                    .artifact(resource_id, &variant.id, *platform)
                    .expect("provisional artifact exists");
                let source_id = match &authored.artifacts[platform].version {
                    TemplateArtifactVersion::Static { releases }
                    | TemplateArtifactVersion::Remote { releases, .. } => releases,
                };
                let source_path = format!("snapshot.resources.{resource_id}.{source_id}");
                let render = |version: &str, tag_path: String| {
                    selected
                        .render(
                            ArtifactRenderOptions {
                                version: Some(version),
                                source: None,
                            },
                            &limits,
                        )
                        .map(|rendered| rendered.artifact().to_owned())
                        .map_err(|source| CompileError::RenderName {
                            path: rebase(source.path(), || tag_path),
                            source,
                        })
                };
                match (&mut artifact.version, &sources[source_id]) {
                    (
                        ResourceArtifactVersionTemplate::Static { releases, .. },
                        Source::History(history),
                    ) => {
                        let mut bound = Vec::new();
                        for (position, release) in history.iter().enumerate() {
                            let name = match render(
                                &release.tag,
                                format!("{source_path}.releases[{position}].tag"),
                            ) {
                                Ok(name) => name,
                                Err(error) => {
                                    errors.push(error);
                                    break;
                                }
                            };
                            match find_asset(release, &name) {
                                Some(asset) => bound.push(ResourceStaticReleaseTemplate {
                                    version: release.tag.clone(),
                                    checksum: asset.digest.clone(),
                                    published_at: release.published_at.clone(),
                                    source: None,
                                }),
                                None if position == 0 => {
                                    errors.push(CompileError::FindArtifact {
                                        path: path.clone(),
                                        release: release.tag.clone(),
                                        artifact: name,
                                    });
                                    break;
                                }
                                None => {}
                            }
                        }
                        *releases = bound;
                    }
                    (
                        ResourceArtifactVersionTemplate::Remote {
                            fallback: Some(fallback),
                            ..
                        },
                        Source::Tag { release, .. },
                    ) => match render(&fallback.version, format!("{source_path}.version")) {
                        Ok(name) => match find_asset(release, &name) {
                            Some(asset) => {
                                fallback.checksum = asset.digest.clone();
                                fallback.published_at = Some(asset.updated_at.clone());
                            }
                            None => errors.push(CompileError::FindArtifact {
                                path: path.clone(),
                                release: release.tag.clone(),
                                artifact: name,
                            }),
                        },
                        Err(error) => errors.push(error),
                    },
                    _ => {}
                }
            }
        }
    }
    output
}

fn find_asset<'a>(release: &'a ReleaseSnapshot, name: &str) -> Option<&'a AssetSnapshot> {
    release.assets.iter().find(|asset| asset.name == name)
}

/// Prefixes a compiled-manifest path with `output.`; paths of host arguments
/// (`arg.*`) are replaced by `replacement`, the input that supplied them.
fn rebase(path: &str, replacement: impl FnOnce() -> String) -> String {
    if path.starts_with("arg.") {
        replacement()
    } else {
        format!("output.{path}")
    }
}

/// Plans every artifact the way a host would, canonically and through each
/// mirror, so templates that cannot render fail at compile time (V-7). A mirror
/// is applied to every request whose registry it serves; skipping the others
/// keeps one unserved registry from hiding errors in the rest of a plan.
fn preflight(manifest: &ValidatedManifest, errors: &mut Vec<CompileError>) {
    let limits = Limits::default();
    let mut report = |error: ResolveError, artifact_path: &str| {
        let path = rebase(&error.path(), || format!("output.{artifact_path}.version"));
        errors.push(CompileError::Preflight {
            path,
            source: Box::new(error),
        });
    };
    let mirrors: Vec<Option<&str>> = std::iter::once(None)
        .chain(
            manifest
                .manifest()
                .mirrors
                .keys()
                .map(|id| Some(id.as_str())),
        )
        .collect();
    for (resource_id, resource) in &manifest.manifest().resources {
        for (index, variant) in resource.variants.iter().enumerate() {
            for (platform, artifact) in &variant.artifacts {
                let artifact_path = format!(
                    "resources.{resource_id}.variants[{index}].artifacts.{}",
                    platform.as_str()
                );
                let selected = manifest
                    .artifact(resource_id, &variant.id, *platform)
                    .expect("validated artifact exists");
                let version: Cow<'_, str> = match &artifact.version {
                    ResourceArtifactVersionTemplate::Static { latest, .. } => latest.into(),
                    ResourceArtifactVersionTemplate::Remote { fallback, .. } => fallback
                        .as_ref()
                        .map_or(PLACEHOLDER_VERSION.into(), |fallback| {
                            fallback.version.as_str().into()
                        }),
                };
                for id in &mirrors {
                    let mirror = Mirror {
                        id: *id,
                        eligible_only: true,
                    };
                    if let Err(error) = plan_version_with(&selected, None, mirror, &limits) {
                        report(error, &artifact_path);
                    }
                    let options = DownloadOptions {
                        version: Some(&version),
                        source: None,
                        mirror: *id,
                        checksum_mode: ChecksumMode::Remote,
                    };
                    if let Err(error) = plan_download_with(&selected, options, mirror, &limits) {
                        report(error, &artifact_path);
                    }
                }
            }
        }
    }
}
