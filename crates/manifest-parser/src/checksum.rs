use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use specta::Type;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
