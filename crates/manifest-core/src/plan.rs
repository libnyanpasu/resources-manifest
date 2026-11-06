use std::borrow::Cow;
use std::collections::BTreeSet;

use manifest_metadata::{
    RemoteResourceTemplate, ResourceArtifactVersionTemplate, ResourceStaticReleaseTemplate,
    SourceVariables, SupportedPlatform,
};
use manifest_parser::{
    CompiledVersionParser, Limits, ParseError, RemoteChecksumParser, RemoteResponse,
    RemoteVersionParser, ResourceChecksum,
};
use serde::Serialize;
use serde_json::{Value, json};
use snafu::Snafu;
use specta::Type;
use url::Url;

use crate::render::{render, validate_literal, validate_url};
use crate::scope::{Scope, merge, registry_scope};
use crate::{
    ArtifactRenderOptions, ChecksumKind, RenderError, SelectedArtifact, TemplateValue,
    ValidatedManifest, artifact_checksum_kind,
};

#[derive(Debug, Snafu, Type)]
#[non_exhaustive]
#[must_use]
pub enum ResolveError {
    #[snafu(display("unknown resource {resource:?}"))]
    FindResource { resource: String },
    #[snafu(display("no variant of {resource:?} supports {platform:?}"))]
    SelectPlatform {
        resource: String,
        platform: SupportedPlatform,
    },
    #[snafu(display("variant {variant:?} does not support {platform:?}"))]
    FindVariant {
        variant: String,
        platform: SupportedPlatform,
    },
    #[snafu(display("cannot select tags {tags:?}: {reason}"))]
    SelectTags { tags: Vec<String>, reason: String },
    #[snafu(display("variant ID {variant:?} and tags select different variants"))]
    MatchSelection { variant: String, tags: Vec<String> },
    #[snafu(display("default variant is missing for {platform:?}"))]
    FindDefault {
        resource: String,
        platform: SupportedPlatform,
    },
    #[snafu(display("unknown mirror {mirror:?}"))]
    ResolveMirror { mirror: String },
    #[snafu(display("mirror {mirror:?} does not support registry {registry:?}"))]
    SelectMirror { mirror: String, registry: String },
    #[snafu(display("version {version:?} has no matching fallback checksum"))]
    SelectFallback {
        version: String,
        fallback_version: Option<String>,
    },
    #[snafu(display("unsupported parser type {parser_type:?}"))]
    ResolveParser { parser_type: String },
    #[snafu(display("cannot render request: {source}"))]
    RenderRequest {
        #[specta(skip)]
        source: RenderError,
    },
    #[snafu(display("cannot compile parser: {source}"))]
    CompileParser {
        #[specta(skip)]
        source: ParseError,
    },
}

impl ResolveError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::FindResource { .. } => "resolve.unknown_resource",
            Self::SelectPlatform { .. } => "resolve.unsupported_platform",
            Self::FindVariant { .. } => "resolve.variant_not_found",
            Self::SelectTags { .. } => "resolve.tags_rejected",
            Self::MatchSelection { .. } => "resolve.selection_conflict",
            Self::FindDefault { .. } => "resolve.default_variant_missing",
            Self::ResolveMirror { .. } => "resolve.unknown_mirror",
            Self::SelectMirror { .. } => "resolve.mirror_not_allowed",
            Self::SelectFallback { .. } => "resolve.snapshot_unavailable",
            Self::ResolveParser { .. } => "resolve.unsupported_parser",
            Self::RenderRequest { source } => source.code(),
            Self::CompileParser { source } => source.code(),
        }
    }

    #[must_use]
    pub fn path(&self) -> Cow<'_, str> {
        match self {
            Self::FindResource { .. } => "arg.resource".into(),
            Self::SelectPlatform { .. } => "arg.platform".into(),
            Self::FindVariant { .. } => "arg.variant_id".into(),
            Self::SelectTags { .. } => "arg.tags".into(),
            Self::MatchSelection { .. } => "arg.variant".into(),
            Self::FindDefault { resource, platform } => format!(
                "resources.{resource}.default_variants.{}",
                platform.as_str()
            )
            .into(),
            Self::ResolveMirror { .. } | Self::SelectMirror { .. } => "arg.mirror".into(),
            Self::SelectFallback { .. } => "arg.version".into(),
            Self::ResolveParser { .. } => "parser.type".into(),
            Self::RenderRequest { source } => source.path().into(),
            Self::CompileParser { source } => source.path(),
        }
    }

    #[must_use]
    pub fn fields(&self) -> Value {
        match self {
            Self::FindResource { resource } => json!({ "resource": resource }),
            Self::SelectPlatform { resource, platform }
            | Self::FindDefault { resource, platform } => {
                json!({ "resource": resource, "platform": platform })
            }
            Self::FindVariant { variant, platform } => {
                json!({ "variant": variant, "platform": platform })
            }
            Self::SelectTags { tags, reason } => json!({ "tags": tags, "reason": reason }),
            Self::MatchSelection { variant, tags } => json!({ "variant": variant, "tags": tags }),
            Self::ResolveMirror { mirror } => json!({ "mirror": mirror }),
            Self::SelectMirror { mirror, registry } => {
                json!({ "mirror": mirror, "registry": registry })
            }
            Self::SelectFallback {
                version,
                fallback_version,
            } => json!({ "version": version, "fallback_version": fallback_version }),
            Self::ResolveParser { parser_type } => json!({ "parser_type": parser_type }),
            Self::RenderRequest { source } => source.fields(),
            Self::CompileParser { source } => source.fields(),
        }
    }
}

impl From<RenderError> for ResolveError {
    fn from(source: RenderError) -> Self {
        Self::RenderRequest { source }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VariantSelection<'a> {
    pub id: Option<&'a str>,
    pub tags: Option<&'a [String]>,
}

#[must_use = "variant selection errors must be handled"]
pub fn select_variant<'a>(
    manifest: &'a ValidatedManifest,
    resource_id: &str,
    platform: SupportedPlatform,
    selection: VariantSelection<'_>,
) -> Result<SelectedArtifact<'a>, ResolveError> {
    let resource = manifest
        .manifest()
        .resources
        .get(resource_id)
        .ok_or_else(|| ResolveError::FindResource {
            resource: resource_id.to_owned(),
        })?;
    let available: Vec<_> = resource
        .variants
        .iter()
        .filter(|variant| variant.artifacts.contains_key(&platform))
        .collect();
    if available.is_empty() {
        return Err(ResolveError::SelectPlatform {
            resource: resource_id.to_owned(),
            platform,
        });
    }
    let by_id = selection
        .id
        .map(|id| {
            available
                .iter()
                .copied()
                .find(|variant| variant.id == id)
                .ok_or_else(|| ResolveError::FindVariant {
                    variant: id.to_owned(),
                    platform,
                })
        })
        .transpose()?;
    let by_tags = selection
        .tags
        .map(|tags| {
            let unique: BTreeSet<_> = tags.iter().map(String::as_str).collect();
            let reason = if tags.is_empty() {
                Some("empty")
            } else if unique.len() != tags.len() {
                Some("duplicate")
            } else {
                None
            };
            if let Some(reason) = reason {
                return Err(ResolveError::SelectTags {
                    tags: tags.to_vec(),
                    reason: reason.to_owned(),
                });
            }
            available
                .iter()
                .copied()
                .find(|variant| {
                    variant.tags.len() == unique.len()
                        && variant
                            .tags
                            .iter()
                            .all(|tag| unique.contains(tag.id.as_str()))
                })
                .ok_or_else(|| ResolveError::SelectTags {
                    tags: tags.to_vec(),
                    reason: "not_found".to_owned(),
                })
        })
        .transpose()?;
    if let (Some(by_id), Some(by_tags)) = (by_id, by_tags)
        && by_id.id != by_tags.id
    {
        return Err(ResolveError::MatchSelection {
            variant: by_id.id.clone(),
            tags: selection.tags.unwrap().to_vec(),
        });
    }
    let selected = by_id
        .or(by_tags)
        .or_else(|| {
            let id = resource.default_variants.get(&platform)?;
            available.iter().copied().find(|variant| variant.id == *id)
        })
        .or_else(|| (available.len() == 1).then(|| available[0]))
        .ok_or_else(|| ResolveError::FindDefault {
            resource: resource_id.to_owned(),
            platform,
        })?;
    Ok(manifest
        .artifact(resource_id, &selected.id, platform)
        .expect("selected available artifact"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[must_use]
pub struct RequestPlan {
    canonical_url: String,
    mirror_url: Option<String>,
    kind: ChecksumKind,
}

impl RequestPlan {
    #[must_use]
    pub fn canonical_url(&self) -> &str {
        &self.canonical_url
    }
    #[must_use]
    pub fn mirror_url(&self) -> Option<&str> {
        self.mirror_url.as_deref()
    }
    #[must_use]
    pub fn kind(&self) -> ChecksumKind {
        self.kind
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[must_use]
pub struct RemoteVersionPlan {
    request: RequestPlan,
    parser: RemoteVersionParser,
    #[serde(skip)]
    #[specta(skip)]
    compiled: CompiledVersionParser,
}

impl RemoteVersionPlan {
    #[must_use = "request plans must be inspected"]
    pub fn request(&self) -> &RequestPlan {
        &self.request
    }
    #[must_use]
    pub fn parser(&self) -> &CompiledVersionParser {
        &self.compiled
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[must_use]
pub struct VersionPlan {
    kind: ChecksumKind,
    latest: Option<String>,
    releases: Vec<ResourceStaticReleaseTemplate>,
    remote: Option<RemoteVersionPlan>,
}

impl VersionPlan {
    #[must_use]
    pub fn kind(&self) -> ChecksumKind {
        self.kind
    }
    #[must_use]
    pub fn latest(&self) -> Option<&str> {
        self.latest.as_deref()
    }
    #[must_use]
    pub fn releases(&self) -> &[ResourceStaticReleaseTemplate] {
        &self.releases
    }
    #[must_use]
    pub fn remote(&self) -> Option<&RemoteVersionPlan> {
        self.remote.as_ref()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VersionOptions<'a> {
    pub source: Option<&'a SourceVariables>,
    pub mirror: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChecksumMode {
    #[default]
    Remote,
    /// Use a remote artifact's fallback checksum for its exact fallback version.
    /// Static artifacts have no fallback snapshot; this mode leaves their normal plan unchanged.
    Snapshot,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DownloadOptions<'a> {
    pub version: Option<&'a str>,
    pub source: Option<&'a SourceVariables>,
    pub mirror: Option<&'a str>,
    pub checksum_mode: ChecksumMode,
}

#[derive(Debug, Clone, Serialize, Type)]
#[must_use]
pub struct ChecksumPlan {
    request: RequestPlan,
    parser: RemoteChecksumParser,
    artifact: String,
    #[serde(skip)]
    #[specta(skip)]
    limits: Limits,
}

impl ChecksumPlan {
    #[must_use = "request plans must be inspected"]
    pub fn request(&self) -> &RequestPlan {
        &self.request
    }
    #[must_use]
    pub fn parser(&self) -> &RemoteChecksumParser {
        &self.parser
    }
    #[must_use]
    pub fn artifact(&self) -> &str {
        &self.artifact
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[must_use]
pub struct ResolvedChecksum {
    checksum: ResourceChecksum,
    kind: ChecksumKind,
}

impl ResolvedChecksum {
    #[must_use]
    pub fn checksum(&self) -> &ResourceChecksum {
        &self.checksum
    }
    #[must_use]
    pub fn kind(&self) -> ChecksumKind {
        self.kind
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[must_use]
pub struct DownloadPlan {
    artifact: String,
    version: String,
    request: RequestPlan,
    checksum: Option<ResolvedChecksum>,
    checksum_request: Option<ChecksumPlan>,
}

impl DownloadPlan {
    #[must_use]
    pub fn artifact(&self) -> &str {
        &self.artifact
    }
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }
    #[must_use = "request plans must be inspected"]
    pub fn request(&self) -> &RequestPlan {
        &self.request
    }
    #[must_use]
    pub fn kind(&self) -> ChecksumKind {
        self.request.kind
    }
    #[must_use]
    pub fn checksum(&self) -> Option<&ResolvedChecksum> {
        self.checksum.as_ref()
    }
    #[must_use]
    pub fn checksum_request(&self) -> Option<&ChecksumPlan> {
        self.checksum_request.as_ref()
    }
}

#[must_use = "unsupported parsers must be handled"]
fn check_support(selected: &SelectedArtifact<'_>) -> Result<(), ResolveError> {
    if let ResourceArtifactVersionTemplate::Remote {
        version, checksum, ..
    } = &selected.artifact.version
    {
        let unsupported = if let RemoteVersionParser::Unsupported { parser_type } = &version.parser
        {
            Some(parser_type)
        } else if let Some(checksum) = checksum
            && let RemoteChecksumParser::Unsupported { parser_type } = &checksum.parser
        {
            Some(parser_type)
        } else {
            None
        };
        if let Some(parser_type) = unsupported {
            return Err(ResolveError::ResolveParser {
                parser_type: parser_type.clone(),
            });
        }
    }
    Ok(())
}

#[must_use = "version planning errors must be handled"]
pub fn plan_version(
    selected: &SelectedArtifact<'_>,
    options: VersionOptions<'_>,
    limits: &Limits,
) -> Result<VersionPlan, ResolveError> {
    check_support(selected)?;
    let kind = artifact_checksum_kind(selected.artifact);
    match &selected.artifact.version {
        ResourceArtifactVersionTemplate::Static { latest, releases } => Ok(VersionPlan {
            kind,
            latest: Some(latest.clone()),
            releases: releases.clone(),
            remote: None,
        }),
        ResourceArtifactVersionTemplate::Remote { version, .. } => {
            let compiled = version
                .parser
                .compile(limits)
                .map_err(|source| ResolveError::CompileParser { source })?;
            let values = version_values(selected, options.source)?;
            let request = remote_request(
                selected,
                &version.resource,
                &values,
                &format!("{}.version.version.resource", selected.path()),
                options.mirror,
                kind,
                limits,
            )?;
            Ok(VersionPlan {
                kind,
                latest: None,
                releases: Vec::new(),
                remote: Some(RemoteVersionPlan {
                    request,
                    parser: version.parser.clone(),
                    compiled,
                }),
            })
        }
    }
}

#[must_use = "source validation errors must be handled"]
fn version_values<'a>(
    selected: &'a SelectedArtifact<'_>,
    source: Option<&'a SourceVariables>,
) -> Result<Scope<'a, TemplateValue<'a>>, RenderError> {
    let mut values = Scope::new();
    merge(
        &mut values,
        selected.resource.source.as_ref(),
        &format!("resources.{}.source", selected.resource_id),
    );
    merge(
        &mut values,
        selected.variant.source.as_ref(),
        &format!(
            "resources.{}.variants[{}].source",
            selected.resource_id, selected.variant_index
        ),
    );
    merge(
        &mut values,
        selected.artifact.source.as_ref(),
        &format!("{}.source", selected.path()),
    );
    if let Some(source) = source {
        for (key, value) in source {
            validate_literal(value, &format!("arg.source.{key}"))?;
            values.insert(
                key.as_str(),
                (TemplateValue::Literal(value), format!("arg.source.{key}")),
            );
        }
    }
    insert_builtins(selected, &mut values);
    Ok(values)
}

fn insert_builtins<'a>(
    selected: &'a SelectedArtifact<'_>,
    values: &mut Scope<'a, TemplateValue<'a>>,
) {
    for (key, value) in [
        ("resource", selected.resource_id),
        ("resource_id", selected.resource_id),
        ("variant", selected.variant.id.as_str()),
        ("variant_id", selected.variant.id.as_str()),
        ("tag_key", selected.variant.tag_key.as_str()),
        ("platform", selected.platform.as_str()),
    ] {
        values.insert(key, (TemplateValue::Literal(value), format!("arg.{key}")));
    }
}

#[must_use = "download planning errors must be handled"]
pub fn plan_download(
    selected: &SelectedArtifact<'_>,
    options: DownloadOptions<'_>,
    limits: &Limits,
) -> Result<DownloadPlan, ResolveError> {
    check_support(selected)?;
    let render_options = ArtifactRenderOptions {
        version: options.version,
        source: options.source,
    };
    let rendered = selected.render(render_options, limits)?;
    let mut values = selected.values(render_options)?;
    let version = match &selected.artifact.version {
        ResourceArtifactVersionTemplate::Static { latest, .. } => options.version.unwrap_or(latest),
        ResourceArtifactVersionTemplate::Remote { .. } => {
            options.version.expect("render requires remote version")
        }
    };
    let (kind, checksum, remote) = match &selected.artifact.version {
        ResourceArtifactVersionTemplate::Static { releases, .. } => {
            let checksum = releases
                .iter()
                .find(|release| release.version == version)
                .and_then(|release| release.checksum.clone());
            (
                if checksum.is_some() {
                    ChecksumKind::Authoritative
                } else {
                    ChecksumKind::None
                },
                checksum,
                None,
            )
        }
        ResourceArtifactVersionTemplate::Remote {
            fallback, checksum, ..
        } => {
            if options.checksum_mode == ChecksumMode::Snapshot {
                let digest = fallback
                    .as_ref()
                    .filter(|release| release.version == version)
                    .and_then(|release| release.checksum.clone())
                    .ok_or_else(|| ResolveError::SelectFallback {
                        version: version.to_owned(),
                        fallback_version: fallback.as_ref().map(|release| release.version.clone()),
                    })?;
                (ChecksumKind::Snapshot, Some(digest), None)
            } else {
                (
                    if checksum.is_some() {
                        ChecksumKind::Parsed
                    } else {
                        ChecksumKind::None
                    },
                    None,
                    checksum.as_ref(),
                )
            }
        }
    };
    values.insert(
        "artifact",
        (
            TemplateValue::Literal(rendered.artifact()),
            format!("{}.artifact", selected.path()),
        ),
    );
    let mut canonical = registry_scope(selected.manifest.manifest(), &selected.resource.registry);
    canonical.extend(values.clone());
    let request = request_plan(
        selected.manifest,
        &selected.resource.registry,
        rendered.canonical_url(),
        &canonical,
        options.mirror,
        kind,
        limits,
    )?;
    let checksum_request = remote
        .map(|remote| {
            remote_request(
                selected,
                &remote.resource,
                &values,
                &format!("{}.version.checksum.resource", selected.path()),
                options.mirror,
                kind,
                limits,
            )
            .map(|request| ChecksumPlan {
                request,
                parser: remote.parser.clone(),
                artifact: rendered.artifact().to_owned(),
                limits: *limits,
            })
        })
        .transpose()?;
    Ok(DownloadPlan {
        artifact: rendered.artifact().to_owned(),
        version: version.to_owned(),
        request,
        checksum: checksum.map(|checksum| ResolvedChecksum { checksum, kind }),
        checksum_request,
    })
}

#[must_use = "remote planning errors must be handled"]
fn remote_request(
    selected: &SelectedArtifact<'_>,
    remote: &RemoteResourceTemplate,
    base: &Scope<'_, TemplateValue<'_>>,
    path: &str,
    mirror: Option<&str>,
    kind: ChecksumKind,
    limits: &Limits,
) -> Result<RequestPlan, ResolveError> {
    let mut values = registry_scope(selected.manifest.manifest(), &remote.registry);
    values.extend(base.clone());
    merge(
        &mut values,
        remote.source.as_ref(),
        &format!("{path}.source"),
    );
    insert_builtins(selected, &mut values);
    if let Some(version) = base.get("version") {
        values.insert("version", version.clone());
    }
    let artifact = remote
        .artifact
        .as_ref()
        .map(|artifact| {
            render(
                artifact,
                &values,
                &format!("{path}.artifact"),
                limits,
                &mut Vec::new(),
            )
        })
        .transpose()?;
    if let Some(artifact) = &artifact {
        values.insert(
            "artifact",
            (TemplateValue::Literal(artifact), format!("{path}.artifact")),
        );
    }
    let registry = &selected.manifest.manifest().registries[&remote.registry];
    let registry_path = format!("registries.{}.url_template", remote.registry);
    let text = render(
        &registry.url_template,
        &values,
        &registry_path,
        limits,
        &mut Vec::new(),
    )?;
    let url = validate_url(&text, &registry_path)?;
    request_plan(
        selected.manifest,
        &remote.registry,
        url.as_str(),
        &values,
        mirror,
        kind,
        limits,
    )
}

#[must_use = "mirror planning errors must be handled"]
fn request_plan(
    manifest: &ValidatedManifest,
    registry: &str,
    canonical_url: &str,
    values: &Scope<'_, TemplateValue<'_>>,
    mirror: Option<&str>,
    kind: ChecksumKind,
    limits: &Limits,
) -> Result<RequestPlan, ResolveError> {
    let mirror_url =
        mirror
            .map(|id| {
                let mirror = manifest.manifest().mirrors.get(id).ok_or_else(|| {
                    ResolveError::ResolveMirror {
                        mirror: id.to_owned(),
                    }
                })?;
                if !crate::scope::mirror_allows(mirror, registry) {
                    return Err(ResolveError::SelectMirror {
                        mirror: id.to_owned(),
                        registry: registry.to_owned(),
                    });
                }
                let url = Url::parse(canonical_url).expect("validated canonical URL");
                let encoded = encode_uri_component(canonical_url);
                let port = url.port().map(|port| port.to_string()).unwrap_or_default();
                let hostname = url.host_str().unwrap_or_default();
                let host = if port.is_empty() {
                    hostname.to_owned()
                } else {
                    format!("{hostname}:{port}")
                };
                let query = url
                    .query()
                    .filter(|query| !query.is_empty())
                    .map(|query| format!("?{query}"))
                    .unwrap_or_default();
                let path_and_query = format!("{}{query}", url.path());
                let mut values = values.clone();
                for (key, value) in [
                    ("url", canonical_url),
                    ("url_encoded", encoded.as_str()),
                    ("scheme", url.scheme()),
                    ("host", host.as_str()),
                    ("hostname", hostname),
                    ("port", port.as_str()),
                    ("path", url.path()),
                    ("pathname", url.path()),
                    ("query", query.as_str()),
                    ("path_and_query", path_and_query.as_str()),
                ] {
                    values.insert(key, (TemplateValue::Literal(value), format!("arg.{key}")));
                }
                let path = format!("mirrors.{id}.url_template");
                let text = render(
                    &mirror.url_template,
                    &values,
                    &path,
                    limits,
                    &mut Vec::new(),
                )?;
                Ok(validate_url(&text, &path)?.into())
            })
            .transpose()?;
    Ok(RequestPlan {
        canonical_url: canonical_url.to_owned(),
        mirror_url,
        kind,
    })
}

#[must_use]
fn encode_uri_component(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("write to String");
        }
    }
    encoded
}

#[must_use = "checksum parsing errors must be handled"]
pub fn finish_checksum(
    plan: &ChecksumPlan,
    response: &RemoteResponse<'_>,
) -> Result<ResolvedChecksum, ParseError> {
    plan.parser
        .parse(response, &plan.artifact, &plan.limits)
        .map(|checksum| ResolvedChecksum {
            checksum,
            kind: ChecksumKind::Parsed,
        })
}
