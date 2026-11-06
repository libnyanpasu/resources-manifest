use std::cell::Cell;
use std::collections::BTreeMap;

use manifest_metadata::{
    ResourceArtifactTemplate, ResourceArtifactVersionTemplate, ResourceDefinition, ResourceVariant,
    SourceVariables, SupportedPlatform,
};
use manifest_parser::{Limits, VersionLabel};
use serde_json::{Value, json};
use snafu::Snafu;
use url::{SyntaxViolation, Url};

use crate::ValidatedManifest;
use crate::scope::{Scope, merge, registry_scope, variables};

/// Authored values recursively expand; literals are always inserted verbatim.
/// Literal URL components need no label validation. Host values and versions
/// are checked by [`SelectedArtifact::render`] before entering the scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateValue<'a> {
    Authored(&'a str),
    Literal(&'a str),
}

impl<'a> From<&'a str> for TemplateValue<'a> {
    fn from(value: &'a str) -> Self {
        Self::Authored(value)
    }
}

#[derive(Debug, Snafu)]
#[non_exhaustive]
#[must_use]
pub enum RenderError {
    #[snafu(display("unknown template variable {variable:?} at {path}"))]
    ResolveVariable { path: String, variable: String },
    #[snafu(display("cyclic template variables {variables:?} at {path}"))]
    ExpandCycle {
        path: String,
        variables: Vec<String>,
    },
    #[snafu(display("template depth {actual} exceeds {limit} at {path}"))]
    ExpandDepth {
        path: String,
        limit: usize,
        actual: usize,
    },
    #[snafu(display("render output {actual} exceeds {limit} bytes at {path}"))]
    RenderOutput {
        path: String,
        limit: usize,
        actual: usize,
    },
    #[snafu(display("literal does not satisfy VersionLabel at {path}"))]
    ValidateLiteral { path: String },
    #[snafu(display("invalid URL at {path}: {source}"))]
    ParseUrl {
        path: String,
        source: url::ParseError,
    },
    #[snafu(display("unsupported URL scheme {scheme:?} at {path}"))]
    ValidateScheme { path: String, scheme: String },
    #[snafu(display("forbidden URL component {component} at {path}"))]
    ValidateComponent {
        path: String,
        component: &'static str,
    },
    #[snafu(display("remote version is required at {path}"))]
    ReadVersion { path: String },
    #[snafu(display("version {version:?} is not in static history at {path}"))]
    FindRelease { path: String, version: String },
}

impl RenderError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::ResolveVariable { .. } => "template.unknown_variable",
            Self::ExpandCycle { .. } => "template.cycle",
            Self::ExpandDepth { .. } => "template.depth_exceeded",
            Self::RenderOutput { .. } => "template.output_too_large",
            Self::ValidateLiteral { .. } => "template.invalid_literal",
            Self::ParseUrl { .. } => "url.invalid",
            Self::ValidateScheme { .. } => "url.unsupported_scheme",
            Self::ValidateComponent { .. } => "url.forbidden_component",
            Self::ReadVersion { .. } => "resolve.version_required",
            Self::FindRelease { .. } => "resolve.version_not_in_history",
        }
    }

    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::ResolveVariable { path, .. }
            | Self::ExpandCycle { path, .. }
            | Self::ExpandDepth { path, .. }
            | Self::RenderOutput { path, .. }
            | Self::ValidateLiteral { path }
            | Self::ParseUrl { path, .. }
            | Self::ValidateScheme { path, .. }
            | Self::ValidateComponent { path, .. }
            | Self::ReadVersion { path }
            | Self::FindRelease { path, .. } => path,
        }
    }

    #[must_use]
    pub fn fields(&self) -> Value {
        match self {
            Self::ResolveVariable { variable, .. } => json!({ "variable": variable }),
            Self::ExpandCycle { variables, .. } => json!({ "variables": variables }),
            Self::ExpandDepth { limit, actual, .. } | Self::RenderOutput { limit, actual, .. } => {
                json!({ "limit": limit, "actual": actual })
            }
            Self::ValidateScheme { scheme, .. } => json!({ "scheme": scheme }),
            Self::ValidateComponent { component, .. } => json!({ "component": component }),
            Self::FindRelease { version, .. } => json!({ "version": version }),
            Self::ValidateLiteral { .. } | Self::ParseUrl { .. } | Self::ReadVersion { .. } => {
                json!({})
            }
        }
    }
}

/// Render one template without URL normalization or percent encoding.
/// Only `{[A-Za-z0-9_]+}` placeholders expand. The root has depth zero;
/// entering an Authored value adds one, while a Literal adds no depth.
/// Errors use `path`; manifest rendering preserves each binding's own locator.
#[must_use = "template rendering errors must be handled"]
pub fn render_template(
    template: &str,
    values: &BTreeMap<&str, TemplateValue<'_>>,
    path: &str,
    limits: &Limits,
) -> Result<String, RenderError> {
    let scope = values
        .iter()
        .map(|(&key, &value)| (key, (value, path.to_owned())))
        .collect();
    render(template, &scope, path, limits, &mut Vec::new())
}

fn append(
    output: &mut String,
    value: &str,
    path: &str,
    limits: &Limits,
) -> Result<(), RenderError> {
    let actual = output.len().saturating_add(value.len());
    if actual > limits.render_output_bytes() {
        return Err(RenderError::RenderOutput {
            path: path.to_owned(),
            limit: limits.render_output_bytes(),
            actual,
        });
    }
    output.push_str(value);
    Ok(())
}

#[must_use = "template rendering errors must be handled"]
pub(crate) fn render<'a>(
    template: &str,
    scope: &Scope<'a, TemplateValue<'a>>,
    path: &str,
    limits: &Limits,
    stack: &mut Vec<String>,
) -> Result<String, RenderError> {
    let mut output = String::new();
    let mut offset = 0;
    for variable in variables(template) {
        let start = variable.as_ptr() as usize - template.as_ptr() as usize - 1;
        append(&mut output, &template[offset..start], path, limits)?;
        let (value, value_path) =
            scope
                .get(variable)
                .ok_or_else(|| RenderError::ResolveVariable {
                    path: path.to_owned(),
                    variable: variable.to_owned(),
                })?;
        match value {
            TemplateValue::Literal(value) => append(&mut output, value, path, limits)?,
            TemplateValue::Authored(value) => {
                if stack.iter().any(|key| key == variable) {
                    let mut variables = stack.clone();
                    variables.push(variable.to_owned());
                    return Err(RenderError::ExpandCycle {
                        path: path.to_owned(),
                        variables,
                    });
                }
                let actual = stack.len() + 1;
                if actual > limits.template_depth() {
                    return Err(RenderError::ExpandDepth {
                        path: path.to_owned(),
                        limit: limits.template_depth(),
                        actual,
                    });
                }
                stack.push(variable.to_owned());
                let expanded = render(value, scope, value_path, limits, stack)?;
                stack.pop();
                append(&mut output, &expanded, path, limits)?;
            }
        }
        offset = start + variable.len() + 2;
    }
    append(&mut output, &template[offset..], path, limits)?;
    Ok(output)
}

/// A borrowed artifact from a validated manifest, with its document location.
#[derive(Debug, Clone, Copy)]
pub struct SelectedArtifact<'a> {
    pub(crate) manifest: &'a ValidatedManifest,
    pub(crate) resource_id: &'a str,
    pub(crate) resource: &'a ResourceDefinition,
    pub(crate) variant: &'a ResourceVariant,
    pub(crate) variant_index: usize,
    pub(crate) platform: SupportedPlatform,
    pub(crate) artifact: &'a ResourceArtifactTemplate,
}

impl ValidatedManifest {
    /// Look up an explicit resource/variant/platform without default or tag selection.
    #[must_use]
    pub fn artifact(
        &self,
        resource_id: &str,
        variant_id: &str,
        platform: SupportedPlatform,
    ) -> Option<SelectedArtifact<'_>> {
        let (resource_id, resource) = self.manifest().resources.get_key_value(resource_id)?;
        let (variant_index, variant) = resource
            .variants
            .iter()
            .enumerate()
            .find(|(_, variant)| variant.id == variant_id)?;
        Some(SelectedArtifact {
            manifest: self,
            resource_id,
            resource,
            variant,
            variant_index,
            platform,
            artifact: variant.artifacts.get(&platform)?,
        })
    }
}

/// Host inputs are Literal VersionLabels, including every source value.
/// Static artifacts default to latest; remote artifacts require a resolved version.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArtifactRenderOptions<'a> {
    pub version: Option<&'a str>,
    pub source: Option<&'a SourceVariables>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct RenderedArtifact {
    artifact: String,
    canonical_url: String,
}

impl RenderedArtifact {
    #[must_use]
    pub fn artifact(&self) -> &str {
        &self.artifact
    }

    #[must_use]
    pub fn canonical_url(&self) -> &str {
        &self.canonical_url
    }
}

#[must_use = "literal validation errors must be handled"]
pub(crate) fn validate_literal(value: &str, path: &str) -> Result<(), RenderError> {
    value
        .parse::<VersionLabel>()
        .map(|_| ())
        .map_err(|_| RenderError::ValidateLiteral {
            path: path.to_owned(),
        })
}

impl SelectedArtifact<'_> {
    #[must_use]
    pub fn variant(&self) -> &ResourceVariant {
        self.variant
    }

    #[must_use]
    pub fn template(&self) -> &ResourceArtifactTemplate {
        self.artifact
    }

    /// Render a download identity only; no mirror, parser, checksum policy, or IO.
    /// Matching static history or remote fallback supplies release source overrides.
    #[must_use = "artifact rendering errors must be handled"]
    pub fn render(
        &self,
        options: ArtifactRenderOptions<'_>,
        limits: &Limits,
    ) -> Result<RenderedArtifact, RenderError> {
        let values = self.values(options)?;
        let artifact_path = self.path();
        let artifact = render(
            &self.artifact.artifact,
            &values,
            &format!("{artifact_path}.artifact"),
            limits,
            &mut Vec::new(),
        )?;
        let mut canonical = registry_scope(self.manifest.manifest(), &self.resource.registry);
        canonical.extend(values);
        canonical.insert(
            "artifact",
            (
                TemplateValue::Literal(&artifact),
                format!("{artifact_path}.artifact"),
            ),
        );
        let registry = &self.manifest.manifest().registries[&self.resource.registry];
        let path = format!("registries.{}.url_template", self.resource.registry);
        let text = render(
            &registry.url_template,
            &canonical,
            &path,
            limits,
            &mut Vec::new(),
        )?;
        let url = validate_url(&text, &path)?;
        Ok(RenderedArtifact {
            artifact,
            canonical_url: url.into(),
        })
    }
}

#[must_use = "URL validation errors must be handled"]
pub(crate) fn validate_url(text: &str, path: &str) -> Result<Url, RenderError> {
    let has_userinfo = Cell::new(false);
    let url = Url::options()
        .syntax_violation_callback(Some(&|violation| {
            if violation == SyntaxViolation::EmbeddedCredentials {
                has_userinfo.set(true);
            }
        }))
        .parse(text)
        .map_err(|source| RenderError::ParseUrl {
            path: path.to_owned(),
            source,
        })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(RenderError::ValidateScheme {
            path: path.to_owned(),
            scheme: url.scheme().to_owned(),
        });
    }
    if has_userinfo.get() || !url.username().is_empty() || url.password().is_some() {
        return Err(RenderError::ValidateComponent {
            path: path.to_owned(),
            component: "userinfo",
        });
    }
    if url.fragment().is_some() {
        return Err(RenderError::ValidateComponent {
            path: path.to_owned(),
            component: "fragment",
        });
    }
    Ok(url)
}

impl SelectedArtifact<'_> {
    #[must_use]
    pub(crate) fn path(&self) -> String {
        format!(
            "resources.{}.variants[{}].artifacts.{}",
            self.resource_id,
            self.variant_index,
            self.platform.as_str()
        )
    }

    #[must_use = "artifact scope errors must be handled"]
    pub(crate) fn values<'a>(
        &'a self,
        options: ArtifactRenderOptions<'a>,
    ) -> Result<Scope<'a, TemplateValue<'a>>, RenderError> {
        if let Some(version) = options.version {
            validate_literal(version, "arg.version")?;
        }
        if let Some(source) = options.source {
            for (key, value) in source {
                validate_literal(value, &format!("arg.source.{key}"))?;
            }
        }
        let resource_path = format!("resources.{}", self.resource_id);
        let variant_path = format!("{resource_path}.variants[{}]", self.variant_index);
        let artifact_path = format!("{variant_path}.artifacts.{}", self.platform.as_str());
        let (version, release, release_path) = match &self.artifact.version {
            ResourceArtifactVersionTemplate::Static { latest, releases } => {
                let version = options.version.unwrap_or(latest);
                let (index, release) = releases
                    .iter()
                    .enumerate()
                    .find(|(_, release)| release.version == version)
                    .ok_or_else(|| RenderError::FindRelease {
                        path: "arg.version".to_owned(),
                        version: version.to_owned(),
                    })?;
                let path = format!("{artifact_path}.version.releases[{index}]");
                if options.version.is_none() {
                    validate_literal(version, &format!("{path}.version"))?;
                }
                (version, Some(release), path)
            }
            ResourceArtifactVersionTemplate::Remote { fallback, .. } => {
                let version = options.version.ok_or_else(|| RenderError::ReadVersion {
                    path: "arg.version".to_owned(),
                })?;
                (
                    version,
                    fallback
                        .as_ref()
                        .filter(|release| release.version == version),
                    format!("{artifact_path}.version.fallback"),
                )
            }
        };
        let mut values = Scope::<TemplateValue<'_>>::new();
        merge(
            &mut values,
            self.resource.source.as_ref(),
            &format!("{resource_path}.source"),
        );
        merge(
            &mut values,
            self.variant.source.as_ref(),
            &format!("{variant_path}.source"),
        );
        merge(
            &mut values,
            self.artifact.source.as_ref(),
            &format!("{artifact_path}.source"),
        );
        if let Some(release) = release {
            merge(
                &mut values,
                release.source.as_ref(),
                &format!("{release_path}.source"),
            );
        }
        if let Some(source) = options.source {
            for (key, value) in source {
                values.insert(
                    key,
                    (TemplateValue::Literal(value), format!("arg.source.{key}")),
                );
            }
        }
        for (key, value) in [
            ("resource", self.resource_id),
            ("resource_id", self.resource_id),
            ("variant", self.variant.id.as_str()),
            ("variant_id", self.variant.id.as_str()),
            ("tag_key", self.variant.tag_key.as_str()),
            ("platform", self.platform.as_str()),
            ("version", version),
        ] {
            values.insert(key, (TemplateValue::Literal(value), format!("arg.{key}")));
        }
        Ok(values)
    }
}
