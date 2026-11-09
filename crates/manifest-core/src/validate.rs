use std::collections::{BTreeMap, BTreeSet};

use manifest_metadata::{
    RemoteVersionTemplate, ResourceArtifactVersionTemplate, ResourceDefinition, ResourceManifest,
    ResourceTag, SupportedPlatform,
};
use manifest_parser::{Limits, RemoteChecksumParser, RemoteVersionParser};
use serde_json::json;

use crate::scope::{BUILTINS, URL_COMPONENTS, variables};
use crate::{
    ExpandTagReason, ReadHistoryReason, ResolveDefaultReason, ResolveReferenceKind,
    ValidateIdsKind, ValidateIdsReason, ValidationError, VersionPartsReason,
};

const MAX_VERSION_PARTS: usize = 4;

mod templates;

pub(super) fn collect(manifest: &ResourceManifest) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for (mirror_id, mirror) in &manifest.mirrors {
        if let Some(registries) = &mirror.registries {
            for (index, registry) in registries.iter().enumerate() {
                check_registry(
                    manifest,
                    registry,
                    format!("mirrors.{mirror_id}.registries[{index}]"),
                    &mut errors,
                );
            }
        }
    }
    for (resource_id, resource) in &manifest.resources {
        let path = format!("resources.{resource_id}");
        check_registry(
            manifest,
            &resource.registry,
            format!("{path}.registry"),
            &mut errors,
        );
        check_tags(&resource.tags, &format!("{path}.tags"), &mut errors);
        let catalog: BTreeMap<_, _> = resource.tags.iter().map(|tag| (&tag.id, tag)).collect();
        let mut ids = BTreeSet::new();
        let mut combinations = BTreeMap::new();
        for (index, variant) in resource.variants.iter().enumerate() {
            let path = format!("{path}.variants[{index}]");
            check_id(
                &variant.id,
                ValidateIdsKind::Variant,
                &format!("{path}.id"),
                &mut ids,
                &mut errors,
            );
            check_tags(&variant.tags, &format!("{path}.tags"), &mut errors);
            let tag_ids: BTreeSet<_> = variant.tags.iter().map(|tag| tag.id.as_str()).collect();
            let canonical = tag_ids.iter().copied().collect::<Vec<_>>().join("+");
            let mut sorted_ids: Vec<_> = variant.tags.iter().map(|tag| tag.id.as_str()).collect();
            sorted_ids.sort_unstable();
            let expected = sorted_ids.join("+");
            if variant.tag_key != expected {
                errors.push(ValidationError::BuildTagKey {
                    path: format!("{path}.tag_key"),
                    expected,
                    actual: variant.tag_key.clone(),
                });
            }
            if let Some(&first_variant) = combinations.get(&tag_ids) {
                errors.push(ValidationError::SelectTagCombination {
                    path: format!("{path}.tags"),
                    tag_key: canonical,
                    first_variant,
                });
            } else {
                combinations.insert(tag_ids, index);
            }
            let mut groups: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            for (index, tag) in variant.tags.iter().enumerate() {
                let reason = match catalog.get(&tag.id) {
                    None => Some(ExpandTagReason::Unknown),
                    Some(definition) if **definition != *tag => Some(ExpandTagReason::Mismatch),
                    Some(_) => None,
                };
                if let Some(reason) = reason {
                    errors.push(ValidationError::ExpandTag {
                        path: format!("{path}.tags[{index}]"),
                        id: tag.id.clone(),
                        reason,
                    });
                }
                if let Some(group) = tag.group.as_deref().filter(|group| !group.is_empty()) {
                    groups.entry(group).or_default().push(tag.id.clone());
                }
            }
            for (group, tags) in groups {
                let tags: Vec<_> = tags
                    .into_iter()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                if tags.len() > 1 {
                    errors.push(ValidationError::SelectGroup {
                        path: format!("{path}.tags"),
                        group: group.to_owned(),
                        tags,
                    });
                }
            }
            for (platform, artifact) in &variant.artifacts {
                let path = format!("{path}.artifacts.{}.version", platform_name(*platform));
                match &artifact.version {
                    ResourceArtifactVersionTemplate::Static { latest, releases } => {
                        let mut versions = BTreeSet::new();
                        for (index, release) in releases.iter().enumerate() {
                            if !versions.insert(&release.version) {
                                errors.push(ValidationError::ReadHistory {
                                    path: format!("{path}.releases[{index}].version"),
                                    version: release.version.clone(),
                                    reason: ReadHistoryReason::Duplicate,
                                });
                            }
                        }
                        if !versions.contains(latest) {
                            errors.push(ValidationError::ReadHistory {
                                path: format!("{path}.latest"),
                                version: latest.clone(),
                                reason: ReadHistoryReason::LatestMissing,
                            });
                        }
                    }
                    ResourceArtifactVersionTemplate::Remote {
                        latest, checksum, ..
                    } => {
                        if let RemoteVersionTemplate::Composite { parts, format } = latest {
                            check_version_parts(
                                parts,
                                format,
                                &format!("{path}.latest"),
                                &mut errors,
                            );
                        }
                        for (name, part) in latest.parts() {
                            let path = match name {
                                Some(name) => format!("{path}.latest.parts.{name}"),
                                None => format!("{path}.latest"),
                            };
                            check_registry(
                                manifest,
                                &part.resource.registry,
                                format!("{path}.resource.registry"),
                                &mut errors,
                            );
                            if !matches!(part.parser, RemoteVersionParser::Unsupported { .. })
                                && let Err(error) = part.parser.compile(&Limits::default())
                            {
                                errors.push(ValidationError::CompileParser {
                                    path: format!("{path}.{}", error.path()),
                                    reason: error.code(),
                                    details: error.fields(),
                                });
                            }
                        }
                        if let Some(checksum) = checksum {
                            check_registry(
                                manifest,
                                &checksum.resource.registry,
                                format!("{path}.checksum.resource.registry"),
                                &mut errors,
                            );
                            if let RemoteChecksumParser::Header { name, .. } = &checksum.parser
                                && (name.is_empty()
                                    || !name.bytes().all(|byte| {
                                        byte.is_ascii_alphanumeric()
                                            || b"!#$%&'*+-.^_`|~".contains(&byte)
                                    }))
                            {
                                errors.push(ValidationError::CompileParser {
                                    path: format!("{path}.checksum.parser.name"),
                                    reason: "invalid_header_name",
                                    details: json!({ "name": name }),
                                });
                            }
                        }
                    }
                }
            }
        }
        check_defaults(resource, &path, &mut errors);
    }
    templates::check(manifest, &mut errors);
    errors
}

/// Checks the part count, part names, and `format` references of a composite
/// version (design §9.2). `path` locates the version object.
fn check_version_parts<V>(
    parts: &BTreeMap<String, V>,
    format: &str,
    path: &str,
    errors: &mut Vec<ValidationError>,
) {
    if parts.is_empty() || parts.len() > MAX_VERSION_PARTS {
        errors.push(ValidationError::ValidateVersionParts {
            path: format!("{path}.parts"),
            reason: VersionPartsReason::Count,
            part: None,
        });
    }
    let referenced: BTreeSet<&str> = variables(format).collect();
    for name in parts.keys() {
        let reserved = BUILTINS
            .iter()
            .chain(URL_COMPONENTS)
            .chain(&["artifact"])
            .any(|builtin| builtin == name);
        if reserved
            || name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            errors.push(ValidationError::ValidateVersionParts {
                path: format!("{path}.parts.{name}"),
                reason: VersionPartsReason::Name,
                part: Some(name.clone()),
            });
        }
        if !referenced.contains(name.as_str()) {
            errors.push(ValidationError::ValidateVersionParts {
                path: format!("{path}.format"),
                reason: VersionPartsReason::Unreferenced,
                part: Some(name.clone()),
            });
        }
    }
    for variable in referenced {
        if !parts.contains_key(variable) {
            errors.push(ValidationError::ResolveVariable {
                path: format!("{path}.format"),
                variable: variable.to_owned(),
            });
        }
    }
}

fn check_registry(
    manifest: &ResourceManifest,
    id: &str,
    path: String,
    errors: &mut Vec<ValidationError>,
) {
    if !manifest.registries.contains_key(id) {
        errors.push(ValidationError::ResolveReference {
            path,
            kind: ResolveReferenceKind::Registry,
            id: id.to_owned(),
        });
    }
}

fn check_id<'a>(
    id: &'a str,
    kind: ValidateIdsKind,
    path: &str,
    seen: &mut BTreeSet<&'a str>,
    errors: &mut Vec<ValidationError>,
) {
    let valid = id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || b"_.:-".contains(&byte)
                || (kind == ValidateIdsKind::Variant && byte == b'+')
        });
    if !valid {
        errors.push(ValidationError::ValidateIds {
            path: path.to_owned(),
            kind,
            id: id.to_owned(),
            reason: ValidateIdsReason::Invalid,
        });
    }
    if !seen.insert(id) {
        errors.push(ValidationError::ValidateIds {
            path: path.to_owned(),
            kind,
            id: id.to_owned(),
            reason: ValidateIdsReason::Duplicate,
        });
    }
}

fn check_tags(tags: &[ResourceTag], path: &str, errors: &mut Vec<ValidationError>) {
    let mut seen = BTreeSet::new();
    for (index, tag) in tags.iter().enumerate() {
        check_id(
            &tag.id,
            ValidateIdsKind::Tag,
            &format!("{path}[{index}].id"),
            &mut seen,
            errors,
        );
    }
}

fn check_defaults(resource: &ResourceDefinition, path: &str, errors: &mut Vec<ValidationError>) {
    let mut counts = BTreeMap::<_, usize>::new();
    for variant in &resource.variants {
        for platform in variant.artifacts.keys() {
            *counts.entry(*platform).or_default() += 1;
        }
    }
    for (platform, count) in counts {
        if count > 1 && !resource.default_variants.contains_key(&platform) {
            errors.push(ValidationError::ResolveDefault {
                path: format!("{path}.default_variants.{}", platform_name(platform)),
                platform: platform_name(platform),
                variant: None,
                reason: ResolveDefaultReason::Missing,
            });
        }
    }
    for (platform, id) in &resource.default_variants {
        let matching: Vec<_> = resource
            .variants
            .iter()
            .filter(|variant| variant.id == *id)
            .collect();
        let reason = if matching.is_empty() {
            Some(ResolveDefaultReason::Unknown)
        } else if !matching
            .iter()
            .any(|variant| variant.artifacts.contains_key(platform))
        {
            Some(ResolveDefaultReason::UnsupportedPlatform)
        } else {
            None
        };
        if let Some(reason) = reason {
            errors.push(ValidationError::ResolveDefault {
                path: format!("{path}.default_variants.{}", platform_name(*platform)),
                platform: platform_name(*platform),
                variant: Some(id.clone()),
                reason,
            });
        }
    }
}

#[must_use]
fn platform_name(platform: SupportedPlatform) -> String {
    platform.as_str().to_owned()
}
