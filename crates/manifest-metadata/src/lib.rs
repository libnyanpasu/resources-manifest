//! Meta types for the variant-aware resource manifest (schema v2).
//!
//! The wire format mirrors `files/scripts/resource-template.ts`: field names
//! are snake_case, tagged enums use a `type` discriminant and checksums are
//! serialized as `<algorithm>:<hex>`. Every type is annotated with `serde`
//! (which `schemars` also reads for the JSON schema export) plus a derived
//! `specta::Type` for the TypeScript export.
//!
//! Two invariants are part of the type definitions themselves rather than a
//! separate validator: `manifest_version` only accepts the literal
//! [`RESOURCE_MANIFEST_VERSION`], and [`ResourceChecksum`] only accepts the
//! normalized `<algorithm>:<hex>` serialization. Cross-field rules from the
//! design document (tag keys, default variants, registry references, …) are
//! enforced by the compiler/validator, not by these types.
//!
//! `Option` fields use `skip_serializing_if` so they are omitted from JSON
//! when absent. `specta-serde`'s unified format rejects that attribute, so
//! `crates/manifest-schema-generator` strips it from the exported type graph
//! (marking the field optional instead) before handing the graph to the
//! serde-aware TypeScript exporter.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use specta::datatype::DataType;
use specta::{Type, Types};

/// Manifest schema version produced by the generator.
pub const RESOURCE_MANIFEST_VERSION: u32 = 2;

/// The `manifest_version` field: only [`RESOURCE_MANIFEST_VERSION`] is valid.
///
/// A unit type so an incompatible version is unrepresentable in Rust;
/// deserialization rejects any other value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ManifestVersion;

impl ManifestVersion {
    pub const VALUE: u32 = RESOURCE_MANIFEST_VERSION;
}

impl Serialize for ManifestVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(Self::VALUE)
    }
}

impl<'de> Deserialize<'de> for ManifestVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u32::deserialize(deserializer)?;
        if value != Self::VALUE {
            return Err(serde::de::Error::custom(format!(
                "unsupported manifest_version {value}, expected {}",
                Self::VALUE
            )));
        }
        Ok(ManifestVersion)
    }
}

impl JsonSchema for ManifestVersion {
    fn schema_name() -> Cow<'static, str> {
        "ManifestVersion".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "integer",
            "const": Self::VALUE,
        })
    }
}

impl Type for ManifestVersion {
    fn definition(types: &mut Types) -> DataType {
        // Placeholder shape; the schema generator rewrites this to the
        // TypeScript literal `2` during export.
        u32::definition(types)
    }
}

/// Platforms an artifact may target, serialized in `{os}-{arch}` form.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
    Type,
)]
pub enum SupportedPlatform {
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
    #[serde(rename = "windows-arm64")]
    WindowsArm64,
    #[serde(rename = "linux-aarch64")]
    LinuxAarch64,
    #[serde(rename = "linux-amd64")]
    LinuxAmd64,
    #[serde(rename = "darwin-arm64")]
    DarwinArm64,
    #[serde(rename = "darwin-x64")]
    DarwinX64,
}

/// Supported checksum algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Type)]
#[serde(rename_all = "lowercase")]
pub enum ChecksumAlgorithm {
    Md5,
    Sha256,
    Sha512,
}

impl ChecksumAlgorithm {
    pub fn as_str(self) -> &'static str {
        match self {
            ChecksumAlgorithm::Md5 => "md5",
            ChecksumAlgorithm::Sha256 => "sha256",
            ChecksumAlgorithm::Sha512 => "sha512",
        }
    }

    /// Length of the lowercase hexadecimal digest.
    pub fn hex_len(self) -> usize {
        match self {
            ChecksumAlgorithm::Md5 => 32,
            ChecksumAlgorithm::Sha256 => 64,
            ChecksumAlgorithm::Sha512 => 128,
        }
    }
}

/// Error returned when a checksum is not a valid `<algorithm>:<hex>` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseChecksumError(pub String);

impl fmt::Display for ParseChecksumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseChecksumError {}

/// Serialized as `<algorithm>:<lowercase hex>`, for example `sha512:abcd...`.
///
/// Construction and deserialization validate the algorithm and the hex length
/// (`md5` 32, `sha256` 64, `sha512` 128) and normalize both parts to
/// lowercase, mirroring `parseResourceChecksum` in the reference scripts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Type)]
pub struct ResourceChecksum(String);

impl ResourceChecksum {
    pub fn new(checksum: impl Into<String>) -> Result<Self, ParseChecksumError> {
        let checksum = checksum.into();
        let fail = |message: String| {
            ParseChecksumError(format!("invalid checksum {checksum:?}: {message}"))
        };

        let separator = checksum.find(':');
        if separator.is_none() || separator != checksum.rfind(':') {
            return Err(fail("must use <algorithm>:<hex>".into()));
        }
        let separator = separator.expect("checked above");

        let algorithm = checksum[..separator].to_lowercase();
        let value = checksum[separator + 1..].to_lowercase();
        let algorithm = match algorithm.as_str() {
            "md5" => ChecksumAlgorithm::Md5,
            "sha256" => ChecksumAlgorithm::Sha256,
            "sha512" => ChecksumAlgorithm::Sha512,
            _ => return Err(fail(format!("unsupported algorithm {algorithm}"))),
        };

        let expected = algorithm.hex_len();
        if value.len() != expected || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(fail(format!(
                "{} must be a {expected}-character hex string",
                algorithm.as_str()
            )));
        }

        Ok(ResourceChecksum(format!("{}:{value}", algorithm.as_str())))
    }

    pub fn algorithm(&self) -> ChecksumAlgorithm {
        // Invariant upheld by `new`.
        match &self.0[..self.0.find(':').expect("separator")] {
            "md5" => ChecksumAlgorithm::Md5,
            "sha256" => ChecksumAlgorithm::Sha256,
            _ => ChecksumAlgorithm::Sha512,
        }
    }

    /// The lowercase hexadecimal digest, without the algorithm prefix.
    pub fn value(&self) -> &str {
        &self.0[self.0.find(':').expect("separator") + 1..]
    }

    /// The full `<algorithm>:<hex>` serialization.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceChecksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for ResourceChecksum {
    type Err = ParseChecksumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ResourceChecksum::new(s)
    }
}

impl Serialize for ResourceChecksum {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ResourceChecksum {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ResourceChecksum::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ResourceChecksum {
    fn schema_name() -> Cow<'static, str> {
        "ResourceChecksum".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^(md5:[a-f0-9]{32}|sha256:[a-f0-9]{64}|sha512:[a-f0-9]{128})$",
        })
    }
}

/// URL template variables, for example owner/repository identity or a release
/// tag that differs from the version.
pub type SourceVariables = BTreeMap<String, String>;

/// A static, traceable release entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceStaticReleaseTemplate {
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub checksum: Option<ResourceChecksum>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub published_at: Option<String>,
    /// Per-release variables, for example a release tag that differs from version.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub source: Option<SourceVariables>,
}

/// A remote metadata request described declaratively.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct RemoteResourceTemplate {
    /// Named registry used to create the canonical metadata URL.
    pub registry: String,
    /// Optional virtual artifact/path consumed by the registry template.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub artifact: Option<String>,
    /// Variables merged on top of resource/variant/artifact source variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub source: Option<SourceVariables>,
}

/// How to parse a remote version response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteVersionParser {
    /// Use the response body, optionally trimming whitespace.
    Text {
        /// Defaults to true.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        trim: Option<bool>,
    },
    /// Apply a regular expression and return a capture group.
    Regex {
        pattern: String,
        /// Capture group containing the version. Defaults to 1. Deserializing
        /// negative or fractional values fails, matching the reference
        /// validator's non-negative integer rule.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        group: Option<u32>,
    },
}

/// How to parse a remote checksum response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RemoteChecksumParser {
    /// The response body itself is `<algorithm>:<hex>`.
    Checksum,
    /// GNU checksum list: `<hex>  <artifact>`.
    Gnu { algorithm: ChecksumAlgorithm },
    /// BSD checksum list: `SHA512 (<artifact>) = <hex>`.
    Bsd { algorithm: ChecksumAlgorithm },
    /// GitHub `/releases/expanded_assets/<tag>` HTML with asset digests.
    GithubReleaseAssets,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct RemoteVersionResourceTemplate {
    pub resource: RemoteResourceTemplate,
    pub parser: RemoteVersionParser,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct RemoteChecksumResourceTemplate {
    pub resource: RemoteResourceTemplate,
    pub parser: RemoteChecksumParser,
}

/// How an artifact's version is resolved.
///
/// The `Remote` plan is intentionally inlined: these types describe a wire
/// format, not a hot data structure.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceArtifactVersionTemplate {
    /// The manifest contains a bounded, traceable release history.
    Static {
        latest: String,
        /// Ordered history; the latest entry need not be first, but must exist.
        releases: Vec<ResourceStaticReleaseTemplate>,
    },
    /// The manifest tells the backend which URLs to request and how to parse
    /// the latest version and checksum.
    Remote {
        /// Snapshot generated with the manifest; consumers should still query remote.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        fallback: Option<ResourceStaticReleaseTemplate>,
        version: RemoteVersionResourceTemplate,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        checksum: Option<RemoteChecksumResourceTemplate>,
    },
}

/// A platform artifact. The historical string shorthand is intentionally not
/// modeled; the compiler rejects it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceArtifactTemplate {
    pub artifact: String,
    /// Artifact-level URL variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub source: Option<SourceVariables>,
    /// Static traceable versions, or a remote version/checksum resolution plan.
    pub version: ResourceArtifactVersionTemplate,
}

/// A tag without its identifier, used by the per-resource tag catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceTagDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub description: Option<String>,
}

/// A tag with its identifier, as embedded in compiled variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceTag {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub description: Option<String>,
}

/// A variant as authored in the template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceVariantTemplate {
    /// A stable, human-readable alias. The semantic identity is still the exact
    /// set of tags, exposed as `tag_key` in the compiled manifest.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub id: Option<String>,
    pub name: String,
    pub tags: Vec<String>,
    /// Channel/release variables belong here when channels share one resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub source: Option<SourceVariables>,
    pub artifacts: BTreeMap<SupportedPlatform, ResourceArtifactTemplate>,
}

/// A variant in the compiled manifest, with expanded tags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceVariant {
    pub id: String,
    /// Canonical, order-independent key derived from the exact tag combination.
    pub tag_key: String,
    pub name: String,
    /// Expanded definitions let every variant display its own tags directly.
    pub tags: Vec<ResourceTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub source: Option<SourceVariables>,
    pub artifacts: BTreeMap<SupportedPlatform, ResourceArtifactTemplate>,
}

/// A registry produces canonical download/metadata URLs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct RegistryDefinition {
    pub name: String,
    /// Template for a canonical URL. Artifact registries normally use
    /// `{artifact}`; metadata registries may instead expose a release page/API.
    pub url_template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub variables: Option<SourceVariables>,
}

/// Error returned for NaN or infinite numbers, which JSON cannot represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonFiniteF64Error;

impl fmt::Display for NonFiniteF64Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("number must be finite")
    }
}

impl std::error::Error for NonFiniteF64Error {}

/// A finite `f64`, serialized like a JavaScript number: integral values
/// without a fraction (matching `JSON.stringify`), and NaN/infinity are
/// rejected instead of silently becoming `null`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct FiniteF64(f64);

impl FiniteF64 {
    pub fn new(value: f64) -> Result<Self, NonFiniteF64Error> {
        if value.is_finite() {
            Ok(FiniteF64(value))
        } else {
            Err(NonFiniteF64Error)
        }
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

impl From<FiniteF64> for f64 {
    fn from(value: FiniteF64) -> Self {
        value.0
    }
}

impl Serialize for FiniteF64 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_992.0; // 2^53
        if self.0.fract() == 0.0 && self.0.abs() <= MAX_SAFE_INTEGER {
            serializer.serialize_i64(self.0 as i64)
        } else {
            serializer.serialize_f64(self.0)
        }
    }
}

impl<'de> Deserialize<'de> for FiniteF64 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        FiniteF64::new(f64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for FiniteF64 {
    fn schema_name() -> Cow<'static, str> {
        "FiniteF64".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> Schema {
        schemars::json_schema!({ "type": "number" })
    }

    fn inline_schema() -> bool {
        true
    }
}

impl Type for FiniteF64 {
    fn definition(types: &mut Types) -> DataType {
        // Inline plain number, matching the reference `priority?: number`.
        f64::definition(types)
    }
}

/// A mirror transforms a canonical URL into an accelerated/alternative URL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct MirrorDefinition {
    pub name: String,
    /// Template for an accelerated/alternative URL. It may use source variables
    /// and URL components such as `{url}`, `{url_encoded}`, `{host}`, `{path}`
    /// and `{path_and_query}`.
    pub url_template: String,
    /// Limit this mirror to specific registry IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub registries: Option<Vec<String>>,
    /// Lower values are preferred when a caller builds an automatic strategy.
    /// Any finite number is accepted.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub priority: Option<FiniteF64>,
}

/// A resource as authored in the template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceTemplate {
    pub name: String,
    pub registry: String,
    /// Resource-wide variables, normally owner/repository identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub source: Option<SourceVariables>,
    /// Catalog used by variants. Compiled variants contain expanded tag data.
    pub tags: BTreeMap<String, ResourceTagDefinition>,
    /// Platform-specific defaults. A default is inferred when a platform has
    /// only one compatible variant; otherwise an explicit value is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub default_variants: Option<BTreeMap<SupportedPlatform, String>>,
    pub variants: Vec<ResourceVariantTemplate>,
}

/// A resource in the compiled manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceDefinition {
    pub name: String,
    pub registry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub source: Option<SourceVariables>,
    /// All tags available to this resource, useful for filters.
    pub tags: Vec<ResourceTag>,
    pub default_variants: BTreeMap<SupportedPlatform, String>,
    pub variants: Vec<ResourceVariant>,
}

/// The authored resource manifest template, input of the compiler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceManifestTemplate {
    pub manifest_version: ManifestVersion,
    pub registries: BTreeMap<String, RegistryDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub mirrors: Option<BTreeMap<String, MirrorDefinition>>,
    pub resources: BTreeMap<String, ResourceTemplate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub updated_at: Option<String>,
}

/// The compiled resource manifest consumed by downloaders and UIs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
pub struct ResourceManifest {
    pub manifest_version: ManifestVersion,
    pub registries: BTreeMap<String, RegistryDefinition>,
    pub mirrors: BTreeMap<String, MirrorDefinition>,
    pub resources: BTreeMap<String, ResourceDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub updated_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn sample_manifest_json() -> Value {
        json!({
            "manifest_version": 2,
            "registries": {
                "github-release": {
                    "name": "GitHub Releases",
                    "url_template": "https://github.com/{owner}/{repo}/releases/download/{release}/{artifact}"
                }
            },
            "mirrors": {},
            "resources": {
                "mihomo": {
                    "name": "Mihomo",
                    "registry": "github-release",
                    "source": { "owner": "MetaCubeX", "repo": "mihomo" },
                    "tags": [
                        { "id": "channel:alpha", "name": "Alpha", "group": "channel" },
                        { "id": "channel:stable", "name": "Stable", "group": "channel" }
                    ],
                    "default_variants": { "windows-x86_64": "alpha-goamd64-v2" },
                    "variants": [
                        {
                            "id": "alpha-goamd64-v2",
                            "tag_key": "channel:alpha",
                            "name": "Alpha · GOAMD64 v2",
                            "tags": [
                                { "id": "channel:alpha", "name": "Alpha", "group": "channel" }
                            ],
                            "source": { "release": "Prerelease-Alpha" },
                            "artifacts": {
                                "windows-x86_64": {
                                    "artifact": "mihomo-windows-amd64-v2-{version}.zip",
                                    "version": {
                                        "type": "remote",
                                        "fallback": { "version": "alpha-e911985" },
                                        "version": {
                                            "resource": {
                                                "registry": "github-release",
                                                "artifact": "version.txt"
                                            },
                                            "parser": { "type": "text", "trim": true }
                                        },
                                        "checksum": {
                                            "resource": { "registry": "github-release" },
                                            "parser": { "type": "github-release-assets" }
                                        }
                                    }
                                }
                            }
                        },
                        {
                            "id": "stable-goamd64-v2",
                            "tag_key": "channel:stable",
                            "name": "Stable · GOAMD64 v2",
                            "tags": [
                                { "id": "channel:stable", "name": "Stable", "group": "channel" }
                            ],
                            "source": { "release": "{version}" },
                            "artifacts": {
                                "windows-x86_64": {
                                    "artifact": "mihomo-windows-amd64-v2-{version}.zip",
                                    "version": {
                                        "type": "static",
                                        "latest": "v1.19.28",
                                        "releases": [
                                            {
                                                "version": "v1.19.28",
                                                "checksum": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                                                "published_at": "2026-07-08T00:34:23Z"
                                            }
                                        ]
                                    }
                                }
                            }
                        }
                    ]
                }
            },
            "updated_at": "2026-07-19T00:00:00Z"
        })
    }

    #[test]
    fn manifest_roundtrips_reference_shape() {
        let expected = sample_manifest_json();
        let manifest: ResourceManifest =
            serde_json::from_value(expected.clone()).expect("deserialize manifest");
        assert_eq!(manifest.manifest_version, ManifestVersion);
        let serialized = serde_json::to_value(&manifest).expect("serialize manifest");
        assert_eq!(serialized, expected);
    }

    #[test]
    fn optional_fields_are_omitted_from_json() {
        // `skip_serializing_if` means `None` never appears as a `null` value.
        let release = ResourceStaticReleaseTemplate {
            version: "v1.0.0".into(),
            checksum: None,
            published_at: None,
            source: None,
        };
        let serialized = serde_json::to_value(&release).unwrap();
        assert_eq!(serialized, json!({ "version": "v1.0.0" }));

        let mirror = MirrorDefinition {
            name: "m".into(),
            url_template: "{url}".into(),
            registries: None,
            priority: None,
        };
        let serialized = serde_json::to_value(&mirror).unwrap();
        assert_eq!(serialized, json!({ "name": "m", "url_template": "{url}" }));
        // Missing fields deserialize back to `None`.
        let parsed: MirrorDefinition = serde_json::from_value(serialized).unwrap();
        assert_eq!(parsed, mirror);
    }

    #[test]
    fn template_roundtrips_reference_shape() {
        let template_json = json!({
            "manifest_version": 2,
            "registries": {},
            "resources": {},
            "mirrors": {
                "nyanpasu": {
                    "name": "Nyanpasu GitHub mirror",
                    "url_template": "https://example.com{path_and_query}",
                    "registries": ["github-release"],
                    "priority": 10
                }
            }
        });
        let template: ResourceManifestTemplate =
            serde_json::from_value(template_json.clone()).expect("deserialize template");
        let serialized = serde_json::to_value(&template).expect("serialize template");
        assert_eq!(serialized, template_json);
    }

    #[test]
    fn manifest_version_only_accepts_the_literal() {
        assert_eq!(
            serde_json::from_value::<ManifestVersion>(json!(2)).unwrap(),
            ManifestVersion
        );
        for version in [0, 1, 3, 42] {
            assert!(
                serde_json::from_value::<ManifestVersion>(json!(version)).is_err(),
                "version {version} must be rejected"
            );
        }
        assert_eq!(serde_json::to_value(ManifestVersion).unwrap(), json!(2));
    }

    #[test]
    fn checksum_validates_and_normalizes() {
        let checksum = ResourceChecksum::new(
            "SHA256:0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF",
        )
        .expect("valid checksum");
        assert_eq!(checksum.algorithm(), ChecksumAlgorithm::Sha256);
        assert_eq!(checksum.value().len(), 64);
        assert_eq!(
            checksum.as_str(),
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            serde_json::to_value(&checksum).unwrap(),
            json!(checksum.as_str())
        );
    }

    #[test]
    fn checksum_rejects_malformed_values() {
        for invalid in [
            "sha256:abcd",                                                      // wrong hex length
            "sha1:0123456789abcdef0123456789abcdef01234567", // unsupported algorithm
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", // missing algorithm
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcg", // non-hex
            "sha256:0123456789abcdef:extra",                 // two separators
        ] {
            assert!(
                ResourceChecksum::new(invalid).is_err(),
                "{invalid} must be rejected"
            );
            assert!(
                serde_json::from_value::<ResourceChecksum>(json!(invalid)).is_err(),
                "{invalid} must fail deserialization"
            );
        }
    }

    #[test]
    fn priority_behaves_like_a_javascript_number() {
        let mirror = |priority: Option<FiniteF64>| MirrorDefinition {
            name: "m".into(),
            url_template: "{url}".into(),
            registries: None,
            priority,
        };

        // Integral values serialize without a fraction, like JSON.stringify.
        assert_eq!(
            serde_json::to_value(mirror(Some(FiniteF64::new(10.0).unwrap()))).unwrap()["priority"],
            json!(10)
        );
        assert_eq!(
            serde_json::to_value(mirror(Some(FiniteF64::new(0.5).unwrap()))).unwrap()["priority"],
            json!(0.5)
        );
        // Fractional and large finite values roundtrip.
        for value in [0.5, -3.25, 1e15] {
            let parsed: MirrorDefinition = serde_json::from_value(
                json!({"name": "m", "url_template": "{url}", "priority": value}),
            )
            .unwrap();
            assert_eq!(parsed.priority.map(FiniteF64::get), Some(value));
        }
        // Non-finite values are rejected, never silently serialized as null.
        assert!(FiniteF64::new(f64::NAN).is_err());
        assert!(FiniteF64::new(f64::INFINITY).is_err());
        assert!(serde_json::from_str::<FiniteF64>("1e400").is_err());
    }

    #[test]
    fn parses_the_reference_compiled_manifest() {
        // The compiled v2 manifest produced by the reference generator in
        // `files/`; guards against drift between the meta types and reality.
        let manifest: ResourceManifest =
            serde_json::from_str(include_str!("../../../files/manifest/resources.json"))
                .expect("deserialize reference manifest");
        assert_eq!(manifest.manifest_version, ManifestVersion);
        assert!(!manifest.resources.is_empty());
    }
}
