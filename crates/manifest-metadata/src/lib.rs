//! Meta types for the variant-aware resource manifest (schema v2).
//!
//! The wire format mirrors `files/scripts/resource-template.ts`: field names
//! are snake_case, tagged enums use a `type` discriminant and checksums are
//! serialized as `<algorithm>:<hex>`. Every type is annotated with `serde`
//! (which `schemars` also reads for the JSON schema export) plus a derived
//! `specta::Type` for the TypeScript export.
//!
//! Two invariants are part of the type definitions themselves rather than a
//! separate validator: `manifest_version` only accepts canonical versions within
//! [`MANIFEST_MAJOR`], and [`ResourceChecksum`] only accepts the
//! normalized `<algorithm>:<hex>` serialization. Cross-field rules from the
//! design document (tag keys, default variants, registry references, …) are
//! enforced by the compiler/validator, not by these types.
//!
//! `Option` fields use `skip_serializing_if` so they are omitted from JSON
//! when absent. `specta-serde`'s unified format rejects that attribute, so
//! `crates/manifest-schema-generator` strips it from the exported type graph
//! (marking the field optional instead) before handing the graph to the
//! serde-aware TypeScript exporter.
//!
//! On input, these Rust types deliberately stay lenient: serde maps an
//! explicit `null` for an `Option` field to `None`, even though the published
//! TypeScript and JSON schema contracts only accept "absent or concrete
//! value". Serialization never produces `null`. Consumers that must reject
//! `null` should validate against the JSON schema (or a future validator);
//! the types here prioritize ergonomic parsing.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use schemars::{JsonSchema, Schema};
use serde::de::{IgnoredAny, MapAccess, Visitor, value::StringDeserializer};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use snafu::Snafu;
use specta::datatype::DataType;
use specta::{Type, Types};

/// Manifest schema version produced by the generator.
pub const RESOURCE_MANIFEST_VERSION: &str = "2.0";

pub const MANIFEST_MAJOR: u32 = 2;
pub const CURRENT_MINOR: u32 = 0;

/// The `manifest_version` field accepts canonical versions within [`MANIFEST_MAJOR`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ManifestVersion {
    minor: u32,
}

impl ManifestVersion {
    #[must_use]
    pub const fn current() -> Self {
        Self {
            minor: CURRENT_MINOR,
        }
    }

    #[must_use]
    pub const fn minor(&self) -> u32 {
        self.minor
    }

    #[must_use]
    pub const fn is_newer_minor(&self) -> bool {
        self.minor > CURRENT_MINOR
    }
}

impl Default for ManifestVersion {
    fn default() -> Self {
        Self::current()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Snafu)]
#[non_exhaustive]
pub enum ParseManifestVersionError {
    #[snafu(display("manifest.invalid_version_format at manifest_version"))]
    InvalidVersionFormat {},
    #[snafu(display("manifest.unsupported_major at manifest_version: {major}"))]
    UnsupportedMajor { major: u32 },
}

impl ParseManifestVersionError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidVersionFormat {} => "manifest.invalid_version_format",
            Self::UnsupportedMajor { .. } => "manifest.unsupported_major",
        }
    }

    #[must_use]
    pub const fn path(&self) -> &'static str {
        "manifest_version"
    }
}

impl FromStr for ManifestVersion {
    type Err = ParseManifestVersionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (major, minor) = value
            .split_once('.')
            .ok_or(ParseManifestVersionError::InvalidVersionFormat {})?;
        for part in [major, minor] {
            if part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(ParseManifestVersionError::InvalidVersionFormat {});
            }
        }
        let major = major
            .parse::<u32>()
            .map_err(|_| ParseManifestVersionError::InvalidVersionFormat {})?;
        let minor = minor
            .parse::<u32>()
            .map_err(|_| ParseManifestVersionError::InvalidVersionFormat {})?;
        if major != MANIFEST_MAJOR {
            return Err(ParseManifestVersionError::UnsupportedMajor { major });
        }
        Ok(Self { minor })
    }
}

impl TryFrom<&str> for ManifestVersion {
    type Error = ParseManifestVersionError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl Serialize for ManifestVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("{MANIFEST_MAJOR}.{}", self.minor))
    }
}

impl<'de> Deserialize<'de> for ManifestVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer).map_err(|_| {
            serde::de::Error::custom(ParseManifestVersionError::InvalidVersionFormat {})
        })?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for ManifestVersion {
    fn schema_name() -> Cow<'static, str> {
        "ManifestVersion".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": r"^2\.(0|[1-9][0-9]*)$",
        })
    }
}

impl Type for ManifestVersion {
    fn definition(types: &mut Types) -> DataType {
        // Placeholder shape; the schema generator rewrites this to the
        // TypeScript template literal `2.${number}` during export.
        String::definition(types)
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
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Type)]
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
        #[schemars(range(max = 4_294_967_295_u32))]
        group: Option<u32>,
    },
    #[schemars(skip, !untagged)]
    #[specta(skip)]
    #[serde(untagged)]
    Unsupported {
        #[serde(rename = "type")]
        parser_type: String,
    },
}

impl<'de> Deserialize<'de> for RemoteVersionParser {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Known {
            Text { trim: Option<bool> },
            Regex { pattern: String, group: Option<u32> },
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        let parser_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("parser type must be a string"))?;
        if !matches!(parser_type, "text" | "regex") {
            return Ok(Self::Unsupported {
                parser_type: parser_type.to_owned(),
            });
        }
        match Known::deserialize(value).map_err(serde::de::Error::custom)? {
            Known::Text { trim } => Ok(Self::Text { trim }),
            Known::Regex { pattern, group } => Ok(Self::Regex { pattern, group }),
        }
    }
}

/// How to parse a remote checksum response.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Type)]
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
    #[schemars(skip, !untagged)]
    #[specta(skip)]
    #[serde(untagged)]
    Unsupported {
        #[serde(rename = "type")]
        parser_type: String,
    },
}

impl<'de> Deserialize<'de> for RemoteChecksumParser {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "kebab-case")]
        enum Known {
            Checksum,
            Gnu { algorithm: ChecksumAlgorithm },
            Bsd { algorithm: ChecksumAlgorithm },
            GithubReleaseAssets,
        }

        let value = serde_json::Value::deserialize(deserializer)?;
        let parser_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("parser type must be a string"))?;
        if !matches!(
            parser_type,
            "checksum" | "gnu" | "bsd" | "github-release-assets"
        ) {
            return Ok(Self::Unsupported {
                parser_type: parser_type.to_owned(),
            });
        }
        match Known::deserialize(value).map_err(serde::de::Error::custom)? {
            Known::Checksum => Ok(Self::Checksum),
            Known::Gnu { algorithm } => Ok(Self::Gnu { algorithm }),
            Known::Bsd { algorithm } => Ok(Self::Bsd { algorithm }),
            Known::GithubReleaseAssets => Ok(Self::GithubReleaseAssets),
        }
    }
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
#[serde(deny_unknown_fields)]
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

fn deserialize_known_platforms<'de, D, V>(
    deserializer: D,
) -> Result<BTreeMap<SupportedPlatform, V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct PlatformMapVisitor<V>(PhantomData<V>);

    impl<'de, V: Deserialize<'de>> Visitor<'de> for PlatformMapVisitor<V> {
        type Value = BTreeMap<SupportedPlatform, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a map of platform keys to values")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut platforms = BTreeMap::new();
            while let Some(key) = map.next_key::<String>()? {
                match SupportedPlatform::deserialize(StringDeserializer::<A::Error>::new(key)) {
                    Ok(platform) => {
                        platforms.insert(platform, map.next_value()?);
                    }
                    Err(_) => {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
            }
            Ok(platforms)
        }
    }

    deserializer.deserialize_map(PlatformMapVisitor(PhantomData))
}

fn deserialize_template_artifacts<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<SupportedPlatform, ResourceArtifactTemplate>, D::Error> {
    let artifacts =
        BTreeMap::<SupportedPlatform, ResourceArtifactTemplate>::deserialize(deserializer)?;
    for artifact in artifacts.values() {
        if let ResourceArtifactVersionTemplate::Remote {
            version, checksum, ..
        } = &artifact.version
        {
            if let RemoteVersionParser::Unsupported { parser_type } = &version.parser {
                return Err(serde::de::Error::custom(format!(
                    "unsupported version parser type {parser_type:?} in template"
                )));
            }
            if let Some(checksum) = checksum
                && let RemoteChecksumParser::Unsupported { parser_type } = &checksum.parser
            {
                return Err(serde::de::Error::custom(format!(
                    "unsupported checksum parser type {parser_type:?} in template"
                )));
            }
        }
    }
    Ok(artifacts)
}

/// A variant as authored in the template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(deny_unknown_fields)]
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
    #[serde(deserialize_with = "deserialize_template_artifacts")]
    #[specta(type = BTreeMap<SupportedPlatform, ResourceArtifactTemplate>)]
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
    #[serde(deserialize_with = "deserialize_known_platforms")]
    #[specta(type = BTreeMap<SupportedPlatform, ResourceArtifactTemplate>)]
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
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Type)]
#[specta(inline)]
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
#[serde(deny_unknown_fields)]
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
    #[serde(deserialize_with = "deserialize_known_platforms")]
    #[specta(type = BTreeMap<SupportedPlatform, String>)]
    pub default_variants: BTreeMap<SupportedPlatform, String>,
    pub variants: Vec<ResourceVariant>,
}

/// The authored resource manifest template, input of the compiler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Type)]
#[serde(deny_unknown_fields)]
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
            "manifest_version": "2.0",
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
        assert_eq!(manifest.manifest_version, ManifestVersion::current());
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
    fn explicit_null_is_lenient_input() {
        // Deliberate (see module docs): explicit `null` parses as `None` even
        // though the TypeScript and JSON schema contracts reject it. Strict
        // consumers must validate against the schema, not just these types.
        let parsed: MirrorDefinition = serde_json::from_value(
            json!({ "name": "m", "url_template": "{url}", "priority": null }),
        )
        .unwrap();
        assert_eq!(parsed.priority, None);

        let parsed: ResourceStaticReleaseTemplate = serde_json::from_value(
            json!({ "version": "v1.0.0", "checksum": null, "source": null }),
        )
        .unwrap();
        assert_eq!(parsed.checksum, None);
        assert_eq!(parsed.source, None);
    }

    #[test]
    fn template_roundtrips_reference_shape() {
        let template_json = json!({
            "manifest_version": "2.0",
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
    fn template_rejects_unknown_fields() {
        let template_json = json!({
            "manifest_version": "2.0",
            "registries": {},
            "resources": {
                "example": {
                    "name": "Example",
                    "registry": "example",
                    "tags": { "stable": { "name": "Stable" } },
                    "variants": [{
                        "name": "Stable",
                        "tags": ["stable"],
                        "artifacts": {}
                    }]
                }
            }
        });
        serde_json::from_value::<ResourceManifestTemplate>(template_json.clone())
            .expect("deserialize valid template");

        for path in [
            "",
            "/resources/example",
            "/resources/example/variants/0",
            "/resources/example/tags/stable",
        ] {
            let mut invalid = template_json.clone();
            invalid
                .pointer_mut(path)
                .expect("template object exists")
                .as_object_mut()
                .expect("template node is an object")
                .insert("unknown_field".into(), json!(true));
            let error = serde_json::from_value::<ResourceManifestTemplate>(invalid)
                .expect_err("unknown template field must be rejected");
            assert!(
                error.to_string().contains("unknown field `unknown_field`"),
                "unexpected error at {path}: {error}"
            );
        }
    }

    #[test]
    fn manifest_version_accepts_supported_major_and_preserves_minor() {
        assert_eq!(
            RESOURCE_MANIFEST_VERSION,
            format!("{MANIFEST_MAJOR}.{CURRENT_MINOR}")
        );
        assert_eq!(ManifestVersion::default(), ManifestVersion::current());
        for (version, minor) in [("2.0", 0), ("2.7", 7), ("2.4294967295", u32::MAX)] {
            let parsed = ManifestVersion::try_from(version).expect("valid version");
            assert_eq!(parsed.minor(), minor);
            assert_eq!(parsed.is_newer_minor(), minor > CURRENT_MINOR);
            assert_eq!(serde_json::to_value(parsed).unwrap(), json!(version));
            assert_eq!(
                serde_json::from_value::<ManifestVersion>(json!(version)).unwrap(),
                parsed
            );
        }
    }

    #[test]
    fn manifest_version_rejects_unsupported_majors() {
        for major in [0, 1, 3, u32::MAX] {
            let version = format!("{major}.0");
            let error = version.parse::<ManifestVersion>().unwrap_err();
            assert_eq!(error, ParseManifestVersionError::UnsupportedMajor { major });
            assert_eq!(error.code(), "manifest.unsupported_major");
            assert_eq!(error.path(), "manifest_version");
            let serde_error = serde_json::from_value::<ManifestVersion>(json!(version))
                .expect_err("unsupported major");
            assert!(serde_error.to_string().contains(error.code()));
        }
    }

    #[test]
    fn manifest_version_rejects_malformed_input() {
        for version in [
            "",
            "2",
            "2.07",
            "02.0",
            "2.00",
            "2.",
            ".0",
            "2.0.0",
            "2.+1",
            "+2.0",
            "2.-1",
            "-2.0",
            " 2.0",
            "2.0 ",
            "2.0\n",
            "2. 0",
            "2.1e2",
            "2.１",
            "2.4294967296",
            "4294967296.0",
            "3.07",
            "3.4294967296",
        ] {
            let error = version.parse::<ManifestVersion>().unwrap_err();
            assert_eq!(
                error,
                ParseManifestVersionError::InvalidVersionFormat {},
                "{version:?}"
            );
            assert_eq!(error.code(), "manifest.invalid_version_format");
            assert_eq!(error.path(), "manifest_version");
            let serde_error = serde_json::from_value::<ManifestVersion>(json!(version))
                .expect_err("malformed version");
            assert!(
                serde_error.to_string().contains(error.code()),
                "{version:?}"
            );
        }
        for value in [
            json!(2),
            json!(2.0),
            json!(null),
            json!(true),
            json!([]),
            json!({}),
        ] {
            let error = serde_json::from_value::<ManifestVersion>(value.clone()).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("manifest.invalid_version_format"),
                "{value}"
            );
            assert!(error.to_string().contains("manifest_version"), "{value}");
        }
    }

    #[test]
    fn manifest_version_matches_compatibility_cases() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../../../conformance/compat/manifest.json"))
                .expect("compatibility cases");
        for case in cases.iter().filter(|case| {
            case["name"]
                .as_str()
                .unwrap()
                .starts_with("compat.version.")
        }) {
            let version = case["input"]["manifest_version"].clone();
            let parsed = serde_json::from_value::<ManifestVersion>(version.clone());
            if let Some(expected) = case["expect"].get("error") {
                let error = parsed.expect_err("version must be rejected");
                assert!(
                    error
                        .to_string()
                        .contains(expected["code"].as_str().unwrap())
                );
                assert_eq!(expected["path"], "manifest_version");
                if let Some(version) = version.as_str() {
                    let error = version.parse::<ManifestVersion>().unwrap_err();
                    assert_eq!(error.code(), expected["code"]);
                    assert_eq!(error.path(), expected["path"]);
                    let fields = match error {
                        ParseManifestVersionError::UnsupportedMajor { major } => {
                            json!({"major": major})
                        }
                        ParseManifestVersionError::InvalidVersionFormat {} => json!({}),
                    };
                    assert_eq!(fields, expected["fields"]);
                }
            } else {
                let parsed = parsed.expect("version must be accepted");
                assert_eq!(
                    json!(parsed.is_newer_minor()),
                    case["expect"]["ok"]["newer_minor"]
                );
            }
        }
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
    fn compiled_manifest_ignores_unknown_fields() {
        let reference: Value =
            serde_json::from_str(include_str!("../tests/fixtures/resources.json"))
                .expect("deserialize reference JSON");
        let expected: ResourceManifest =
            serde_json::from_value(reference.clone()).expect("deserialize reference manifest");
        for path in [
            "",
            "/resources/clash_premium",
            "/resources/clash_premium/variants/0",
            "/resources/clash_premium/variants/0/artifacts/windows-x86_64",
            "/registries/github-release",
            "/mirrors/direct",
        ] {
            let mut extended = reference.clone();
            extended
                .pointer_mut(path)
                .expect("fixture node exists")
                .as_object_mut()
                .expect("fixture node is an object")
                .insert("unknown_field".into(), json!(true));
            let parsed: ResourceManifest = serde_json::from_value(extended)
                .unwrap_or_else(|error| panic!("unknown field at {path} must be ignored: {error}"));
            assert_eq!(parsed, expected, "known fields at {path} must be preserved");
        }
    }

    #[test]
    fn template_shared_types_ignore_unknown_fields() {
        let template = json!({
            "manifest_version": "2.0",
            "registries": { "example": { "name": "Example", "url_template": "https://example.com/{artifact}" } },
            "mirrors": { "direct": { "name": "Direct", "url_template": "{url}" } },
            "resources": {
                "example": {
                    "name": "Example",
                    "registry": "example",
                    "tags": {},
                    "variants": [{
                        "name": "Default",
                        "tags": [],
                        "artifacts": { "windows-x86_64": {
                            "artifact": "example.zip",
                            "version": { "type": "static", "latest": "v1", "releases": [{ "version": "v1" }] }
                        } }
                    }]
                }
            }
        });
        let expected: ResourceManifestTemplate =
            serde_json::from_value(template.clone()).expect("deserialize valid template");
        for path in [
            "/registries/example",
            "/mirrors/direct",
            "/resources/example/variants/0/artifacts/windows-x86_64",
            "/resources/example/variants/0/artifacts/windows-x86_64/version",
            "/resources/example/variants/0/artifacts/windows-x86_64/version/releases/0",
        ] {
            let mut extended = template.clone();
            extended
                .pointer_mut(path)
                .expect("template node exists")
                .as_object_mut()
                .expect("template node is an object")
                .insert("unknown_field".into(), json!(true));
            let parsed: ResourceManifestTemplate =
                serde_json::from_value(extended).unwrap_or_else(|error| {
                    panic!("unknown shared field at {path} must be ignored: {error}")
                });
            assert_eq!(parsed, expected, "known fields at {path} must be preserved");
        }
    }

    #[test]
    fn compiled_platform_maps_skip_unknown_values_and_preserve_known_data() {
        let mut input = sample_manifest_json();
        input["manifest_version"] = json!("2.7");
        let expected: ResourceManifest = serde_json::from_value(input.clone()).unwrap();
        for value in [
            json!(null),
            json!(42),
            json!({"version": {"type": "future"}}),
        ] {
            let mut extended = input.clone();
            extended["resources"]["mihomo"]["default_variants"]["future-os-cpu"] = value.clone();
            extended["resources"]["mihomo"]["variants"][0]["artifacts"]["future-os-cpu"] = value;
            let parsed: ResourceManifest = serde_json::from_value(extended)
                .expect("unknown platform values must be skipped without decoding");
            assert!(parsed.manifest_version.is_newer_minor());
            assert_eq!(parsed, expected);
        }
        for path in [
            "/resources/mihomo/default_variants/windows-x86_64",
            "/resources/mihomo/variants/0/artifacts/windows-x86_64",
        ] {
            let mut invalid = input.clone();
            *invalid.pointer_mut(path).unwrap() = json!(42);
            assert!(
                serde_json::from_value::<ResourceManifest>(invalid).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn compiled_compatibility_cases_preserve_known_fields_and_unknown_tags() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../../../conformance/compat/manifest.json"))
                .unwrap();
        for case in cases
            .iter()
            .filter(|case| case["expect"].get("ok").is_some())
        {
            let parsed: ResourceManifest = serde_json::from_value(case["input"].clone())
                .unwrap_or_else(|error| panic!("{}: {error}", case["name"]));
            assert_eq!(
                json!(parsed.manifest_version.is_newer_minor()),
                case["expect"]["ok"]["newer_minor"]
            );
            let mut expected = case["input"].clone();
            let resource = &mut expected["resources"]["sample"];
            resource["default_variants"]
                .as_object_mut()
                .unwrap()
                .remove("future-os-cpu");
            let artifacts = resource["variants"][0]["artifacts"]
                .as_object_mut()
                .unwrap();
            artifacts.remove("future-os-cpu");
            for artifact in artifacts.values_mut() {
                for path in ["/version/version/parser", "/version/checksum/parser"] {
                    if let Some(parser) = artifact.pointer_mut(path) {
                        let tag = parser["type"].as_str().unwrap();
                        if tag.starts_with("future-") {
                            *parser = json!({"type": tag});
                        }
                    }
                }
            }
            assert_eq!(
                serde_json::to_value(&parsed).unwrap(),
                expected,
                "{}",
                case["name"]
            );
            assert_eq!(
                serde_json::to_value(&parsed.resources["sample"].default_variants).unwrap(),
                case["expect"]["ok"]["default_variants"]
            );
        }
    }

    #[test]
    fn newer_minor_accepts_both_unknown_parsers_and_platforms() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("../../../conformance/compat/manifest.json"))
                .unwrap();
        let mut input = cases
            .iter()
            .find(|case| case["name"] == "compat.newer_minor.unknown_platform_and_parser")
            .unwrap()["input"]
            .clone();
        let parser_path =
            "/resources/sample/variants/0/artifacts/windows-x86_64/version/checksum/parser";
        *input.pointer_mut(parser_path).unwrap() =
            json!({"type": "future-checksum-parser", "algorithm": 42});
        let parsed: ResourceManifest = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(parsed.manifest_version.minor(), 7);
        assert!(parsed.manifest_version.is_newer_minor());
        let resource = &parsed.resources["sample"];
        assert_eq!(resource.default_variants.len(), 2);
        let artifacts = &resource.variants[0].artifacts;
        assert_eq!(artifacts.len(), 2);
        let ResourceArtifactVersionTemplate::Remote {
            version, checksum, ..
        } = &artifacts[&SupportedPlatform::WindowsX86_64].version
        else {
            panic!("remote artifact expected");
        };
        assert_eq!(
            version.parser,
            RemoteVersionParser::Unsupported {
                parser_type: "future-version-parser".into()
            }
        );
        assert_eq!(
            checksum.as_ref().unwrap().parser,
            RemoteChecksumParser::Unsupported {
                parser_type: "future-checksum-parser".into()
            }
        );
        let serialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(
            serialized["resources"]["sample"]["variants"][0]["artifacts"]["linux-amd64"],
            input["resources"]["sample"]["variants"][0]["artifacts"]["linux-amd64"]
        );
        input["resources"]["sample"]["default_variants"]
            .as_object_mut()
            .unwrap()
            .remove("future-os-cpu");
        input["resources"]["sample"]["variants"][0]["artifacts"]
            .as_object_mut()
            .unwrap()
            .remove("future-os-cpu");
        for path in [
            parser_path,
            "/resources/sample/variants/0/artifacts/windows-x86_64/version/version/parser",
        ] {
            let parser = input.pointer_mut(path).unwrap();
            *parser = json!({"type": parser["type"]});
        }
        assert_eq!(serialized, input);
    }

    #[test]
    fn unknown_parser_tags_roundtrip_and_ignore_other_fields() {
        for tag in ["future-parser", "Future.Parser", "", "unsupported"] {
            let input =
                json!({"type": tag, "pattern": null, "trim": [], "algorithm": {"future": true}});
            let version: RemoteVersionParser = serde_json::from_value(input.clone()).unwrap();
            let checksum: RemoteChecksumParser = serde_json::from_value(input).unwrap();
            assert_eq!(
                version,
                RemoteVersionParser::Unsupported {
                    parser_type: tag.into()
                }
            );
            assert_eq!(
                checksum,
                RemoteChecksumParser::Unsupported {
                    parser_type: tag.into()
                }
            );
            let expected = json!({"type": tag});
            assert_eq!(serde_json::to_value(&version).unwrap(), expected);
            assert_eq!(serde_json::to_value(&checksum).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<RemoteVersionParser>(expected.clone()).unwrap(),
                version
            );
            assert_eq!(
                serde_json::from_value::<RemoteChecksumParser>(expected).unwrap(),
                checksum
            );
        }
    }

    #[test]
    fn parser_decoding_rejects_missing_tags_and_malformed_known_types() {
        for input in [
            json!({}),
            json!({"type": null}),
            json!({"type": 2}),
            json!({"type": []}),
            json!(null),
            json!([]),
        ] {
            assert!(
                serde_json::from_value::<RemoteVersionParser>(input.clone()).is_err(),
                "{input}"
            );
            assert!(
                serde_json::from_value::<RemoteChecksumParser>(input.clone()).is_err(),
                "{input}"
            );
        }
        for input in [
            json!({"type": "text", "trim": "true"}),
            json!({"type": "regex"}),
            json!({"type": "regex", "pattern": 42}),
            json!({"type": "regex", "pattern": "(.*)", "group": -1}),
            json!({"type": "regex", "pattern": "(.*)", "group": 0.5}),
            json!({"type": "regex", "pattern": "(.*)", "group": 4_294_967_296_u64}),
        ] {
            assert!(
                serde_json::from_value::<RemoteVersionParser>(input.clone()).is_err(),
                "{input}"
            );
        }
        for input in [
            json!({"type": "gnu"}),
            json!({"type": "bsd"}),
            json!({"type": "gnu", "algorithm": "sha1"}),
            json!({"type": "bsd", "algorithm": 42}),
        ] {
            assert!(
                serde_json::from_value::<RemoteChecksumParser>(input.clone()).is_err(),
                "{input}"
            );
        }
        for input in [
            json!({"type": "text"}),
            json!({"type": "text", "trim": false}),
            json!({"type": "regex", "pattern": "(.*)", "group": 0}),
        ] {
            assert!(
                serde_json::from_value::<RemoteVersionParser>(input.clone()).is_ok(),
                "{input}"
            );
        }
        for input in [
            json!({"type": "checksum"}),
            json!({"type": "github-release-assets"}),
            json!({"type": "gnu", "algorithm": "sha256"}),
            json!({"type": "bsd", "algorithm": "sha512"}),
        ] {
            let parsed: RemoteChecksumParser = serde_json::from_value(input.clone()).unwrap();
            assert_eq!(serde_json::to_value(parsed).unwrap(), input);
        }
    }

    #[test]
    fn template_rejects_unknown_platforms_and_parsers() {
        let artifact = sample_manifest_json()["resources"]["mihomo"]["variants"][0]["artifacts"]["windows-x86_64"].clone();
        let template = json!({
            "manifest_version": "2.7",
            "registries": {},
            "resources": {"sample": {
                "name": "Sample", "registry": "example", "tags": {},
                "default_variants": {"windows-x86_64": "stable"},
                "variants": [{"name": "Stable", "tags": [], "artifacts": {"windows-x86_64": artifact}}]
            }}
        });
        serde_json::from_value::<ResourceManifestTemplate>(template.clone()).unwrap();
        for path in [
            "/resources/sample/variants/0/artifacts/windows-x86_64/version/version/parser",
            "/resources/sample/variants/0/artifacts/windows-x86_64/version/checksum/parser",
        ] {
            let mut invalid = template.clone();
            *invalid.pointer_mut(path).unwrap() = json!({"type": "future-template-parser"});
            let error = serde_json::from_value::<ResourceManifestTemplate>(invalid).unwrap_err();
            assert!(
                error.to_string().contains("future-template-parser"),
                "{error}"
            );
        }
        for path in [
            "/resources/sample/default_variants",
            "/resources/sample/variants/0/artifacts",
        ] {
            let mut invalid = template.clone();
            invalid
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("future-os-cpu".into(), json!(null));
            let error = serde_json::from_value::<ResourceManifestTemplate>(invalid).unwrap_err();
            assert!(error.to_string().contains("future-os-cpu"), "{error}");
        }
    }

    #[test]
    fn parses_the_reference_compiled_manifest() {
        // A snapshot of the compiled v2 manifest produced by the reference
        // generator (see `files/manifest/resources.json`); guards against
        // drift between the meta types and reality.
        let expected: Value =
            serde_json::from_str(include_str!("../tests/fixtures/resources.json"))
                .expect("deserialize reference JSON");
        let manifest: ResourceManifest =
            serde_json::from_value(expected.clone()).expect("deserialize reference manifest");
        assert_eq!(serde_json::to_value(&manifest).unwrap(), expected);
        assert_eq!(manifest.manifest_version, ManifestVersion::current());
        assert!(!manifest.resources.is_empty());
    }
}
