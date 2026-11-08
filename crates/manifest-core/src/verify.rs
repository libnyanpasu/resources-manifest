use manifest_parser::ChecksumAlgorithm;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use snafu::Snafu;
use specta::Type;

use crate::ChecksumKind;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct VerifyPolicy {
    pub allow_parsed: bool,
    pub allow_snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Snafu, Type)]
#[non_exhaustive]
#[must_use]
pub enum VerifyError {
    #[snafu(display("checksum kind {kind} is rejected by verification policy"))]
    CheckKind { kind: ChecksumKind },
    #[snafu(display("{algorithm:?} digest mismatch: expected {expected}, got {actual}"))]
    CompareDigest {
        algorithm: ChecksumAlgorithm,
        expected: String,
        actual: String,
    },
}

impl VerifyError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::CheckKind { .. } => "verify.kind_rejected",
            Self::CompareDigest { .. } => "verify.digest_mismatch",
        }
    }

    #[must_use]
    pub fn path(&self) -> &'static str {
        match self {
            Self::CheckKind { .. } => "arg.kind",
            Self::CompareDigest { .. } => "arg.actual",
        }
    }

    #[must_use]
    pub fn fields(&self) -> Value {
        match self {
            Self::CheckKind { kind } => json!({ "kind": kind }),
            Self::CompareDigest {
                algorithm,
                expected,
                actual,
            } => json!({ "algorithm": algorithm, "expected": expected, "actual": actual }),
        }
    }
}

#[must_use = "verification policy decisions must be handled"]
pub fn check_policy(kind: ChecksumKind, policy: &VerifyPolicy) -> Result<(), VerifyError> {
    match kind {
        ChecksumKind::Authoritative => Ok(()),
        ChecksumKind::Parsed if policy.allow_parsed => Ok(()),
        ChecksumKind::Snapshot if policy.allow_snapshot => Ok(()),
        _ => Err(VerifyError::CheckKind { kind }),
    }
}
