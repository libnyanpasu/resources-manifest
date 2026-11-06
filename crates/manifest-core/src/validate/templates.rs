use std::collections::{BTreeMap, BTreeSet};

use manifest_metadata::{
    RemoteResourceTemplate, ResourceArtifactVersionTemplate, ResourceManifest, SourceVariables,
};

use crate::ValidationError;

type Scope<'a> = BTreeMap<&'a str, (&'a str, String)>;

const BUILTINS: &[&str] = &[
    "resource",
    "resource_id",
    "variant",
    "variant_id",
    "tag_key",
    "platform",
    "version",
];
const URL_COMPONENTS: &[&str] = &[
    "url",
    "url_encoded",
    "scheme",
    "host",
    "hostname",
    "port",
    "path",
    "pathname",
    "query",
    "path_and_query",
];

pub(super) fn check(manifest: &ResourceManifest, errors: &mut Vec<ValidationError>) {
    for (resource_id, resource) in &manifest.resources {
        let resource_path = format!("resources.{resource_id}");
        for (index, variant) in resource.variants.iter().enumerate() {
            let variant_path = format!("{resource_path}.variants[{index}]");
            for (platform, artifact) in &variant.artifacts {
                let platform = super::platform_name(*platform);
                let artifact_path = format!("{variant_path}.artifacts.{platform}");
                let mut base = Scope::new();
                merge(
                    &mut base,
                    resource.source.as_ref(),
                    &format!("{resource_path}.source"),
                );
                merge(
                    &mut base,
                    variant.source.as_ref(),
                    &format!("{variant_path}.source"),
                );
                merge(
                    &mut base,
                    artifact.source.as_ref(),
                    &format!("{artifact_path}.source"),
                );
                let mut releases = Vec::new();
                match &artifact.version {
                    ResourceArtifactVersionTemplate::Static {
                        releases: history, ..
                    } => {
                        for (index, release) in history.iter().enumerate() {
                            releases.push((
                                Some(release),
                                format!("{artifact_path}.version.releases[{index}]"),
                            ));
                        }
                    }
                    ResourceArtifactVersionTemplate::Remote {
                        version, fallback, ..
                    } => {
                        remote(
                            manifest,
                            &version.resource,
                            &base,
                            &format!("{artifact_path}.version.version.resource"),
                            false,
                            errors,
                        );
                        if let Some(fallback) = fallback {
                            releases.push((
                                Some(fallback),
                                format!("{artifact_path}.version.fallback"),
                            ));
                        }
                        releases.push((None, String::new()));
                    }
                }
                if releases.is_empty() {
                    releases.push((None, String::new()));
                }
                for (release, release_path) in releases {
                    let mut values = base.clone();
                    if let Some(release) = release {
                        merge(
                            &mut values,
                            release.source.as_ref(),
                            &format!("{release_path}.source"),
                        );
                    }
                    let mut scope = registry_scope(manifest, &resource.registry);
                    scope.extend(values.clone());
                    let builtins: BTreeSet<_> = BUILTINS.iter().copied().collect();
                    inspect_dependencies(
                        &artifact.artifact,
                        &format!("{artifact_path}.artifact"),
                        &scope,
                        &builtins,
                        errors,
                    );
                    let mut canonical_builtins = builtins;
                    canonical_builtins.insert("artifact");
                    inspect_scope(&scope, &canonical_builtins, errors);
                    canonical(
                        manifest,
                        &resource.registry,
                        &scope,
                        &canonical_builtins,
                        errors,
                    );
                    if let ResourceArtifactVersionTemplate::Remote {
                        checksum: Some(checksum),
                        ..
                    } = &artifact.version
                    {
                        remote(
                            manifest,
                            &checksum.resource,
                            &values,
                            &format!("{artifact_path}.version.checksum.resource"),
                            true,
                            errors,
                        );
                    }
                }
            }
        }
    }
}

fn merge<'a>(scope: &mut Scope<'a>, source: Option<&'a SourceVariables>, path: &str) {
    if let Some(source) = source {
        for (key, value) in source {
            scope.insert(key, (value, format!("{path}.{key}")));
        }
    }
}

#[must_use]
fn registry_scope<'a>(manifest: &'a ResourceManifest, registry: &str) -> Scope<'a> {
    let mut scope = Scope::new();
    if let Some(definition) = manifest.registries.get(registry) {
        merge(
            &mut scope,
            definition.variables.as_ref(),
            &format!("registries.{registry}.variables"),
        );
    }
    scope
}

fn remote<'a>(
    manifest: &'a ResourceManifest,
    remote: &'a RemoteResourceTemplate,
    base: &Scope<'a>,
    path: &str,
    has_artifact: bool,
    errors: &mut Vec<ValidationError>,
) {
    let mut scope = registry_scope(manifest, &remote.registry);
    scope.extend(base.clone());
    merge(
        &mut scope,
        remote.source.as_ref(),
        &format!("{path}.source"),
    );
    let mut builtins: BTreeSet<_> = BUILTINS.iter().copied().collect();
    if has_artifact {
        builtins.insert("artifact");
    }
    if let Some(artifact) = &remote.artifact {
        inspect_dependencies(
            artifact,
            &format!("{path}.artifact"),
            &scope,
            &builtins,
            errors,
        );
        builtins.insert("artifact");
    }
    inspect_scope(&scope, &builtins, errors);
    canonical(manifest, &remote.registry, &scope, &builtins, errors);
}

fn canonical(
    manifest: &ResourceManifest,
    registry: &str,
    scope: &Scope<'_>,
    builtins: &BTreeSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(definition) = manifest.registries.get(registry) {
        inspect(
            &definition.url_template,
            &format!("registries.{registry}.url_template"),
            scope,
            builtins,
            errors,
        );
    }
    let mut mirror_builtins = builtins.clone();
    mirror_builtins.extend(URL_COMPONENTS.iter().copied());
    for (id, mirror) in &manifest.mirrors {
        if mirror
            .registries
            .as_ref()
            .is_none_or(|allowed| allowed.is_empty() || allowed.iter().any(|id| id == registry))
        {
            inspect(
                &mirror.url_template,
                &format!("mirrors.{id}.url_template"),
                scope,
                &mirror_builtins,
                errors,
            );
        }
    }
}

fn inspect_scope(scope: &Scope<'_>, builtins: &BTreeSet<&str>, errors: &mut Vec<ValidationError>) {
    for (key, (value, path)) in scope {
        if !builtins.contains(key) {
            inspect(value, path, scope, builtins, errors);
        }
    }
}

fn inspect(
    template: &str,
    path: &str,
    scope: &Scope<'_>,
    builtins: &BTreeSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    for variable in variables(template) {
        if !scope.contains_key(variable) && !builtins.contains(variable) {
            let error = ValidationError::ResolveVariable {
                path: path.to_owned(),
                variable: variable.to_owned(),
            };
            if !errors.contains(&error) {
                errors.push(error);
            }
        }
    }
}

fn inspect_dependencies(
    template: &str,
    path: &str,
    scope: &Scope<'_>,
    builtins: &BTreeSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    let mut pending = vec![(template, path)];
    let mut seen = BTreeSet::new();
    while let Some((template, path)) = pending.pop() {
        inspect(template, path, scope, builtins, errors);
        for variable in variables(template) {
            if !builtins.contains(variable)
                && seen.insert(variable)
                && let Some((value, path)) = scope.get(variable)
            {
                pending.push((value, path));
            }
        }
    }
}

#[must_use = "template variable references must be inspected"]
fn variables(template: &str) -> impl Iterator<Item = &str> {
    template.match_indices('{').filter_map(|(index, _)| {
        let (variable, _) = template[index + 1..].split_once('}')?;
        (!variable.is_empty()
            && variable
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
        .then_some(variable)
    })
}
