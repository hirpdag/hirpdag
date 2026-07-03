// ==== Serialization Base
//
// DAG-aware serialization support shared by all hirpdag modules.
//
// The archive layout (generated per hirpdag module by `#[hirpdag_end]`) is:
//   version, then a node table in post-order DFS order (children before
//   parents), then a list of roots. `HirpdagRef` fields are encoded as u64
//   indices into the node table. Because children always precede parents,
//   a single forward pass reconstructs everything, forward references are
//   errors, and cycles are unrepresentable.
//
// This module holds the format-agnostic pieces: the collect traversal trait,
// the error type, the format version marker, and the binary magic prefix.

/// Magic prefix identifying a hirpdag binary archive.
pub const HIRPDAG_MAGIC: &[u8; 4] = b"HPDG";

/// Version of the hirpdag archive format written by this library.
pub const HIRPDAG_FORMAT_VERSION: u32 = 1;

/// Error type for hirpdag serialization.
///
/// Distinct from [`HirpdagDeserializeError`], mirroring serde's separation of
/// `serde::ser::Error` and `serde::de::Error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirpdagSerializeError {
    /// A serialization session is already active on this thread.
    /// Sessions are per-thread and not re-entrant.
    SessionActive,
    /// An underlying format error (postcard/serde_json).
    Format(String),
}

impl std::fmt::Display for HirpdagSerializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionActive => write!(
                f,
                "hirpdag: a serialization session is already active on this thread"
            ),
            Self::Format(msg) => write!(f, "hirpdag: {}", msg),
        }
    }
}

impl std::error::Error for HirpdagSerializeError {}

/// Error type for hirpdag deserialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirpdagDeserializeError {
    /// The input does not start with the hirpdag binary magic prefix.
    BadMagic,
    /// A deserialization session is already active on this thread.
    /// Sessions are per-thread and not re-entrant.
    SessionActive,
    /// An underlying format error (postcard/serde_json), including
    /// unsupported format versions, invalid node indices, node type
    /// mismatches and truncated input.
    Format(String),
}

impl std::fmt::Display for HirpdagDeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "hirpdag: not a hirpdag binary archive (bad magic)"),
            Self::SessionActive => write!(
                f,
                "hirpdag: a deserialization session is already active on this thread"
            ),
            Self::Format(msg) => write!(f, "hirpdag: {}", msg),
        }
    }
}

impl std::error::Error for HirpdagDeserializeError {}

/// Strips and validates the binary archive magic prefix.
pub fn hirpdag_strip_magic(bytes: &[u8]) -> Result<&[u8], HirpdagDeserializeError> {
    bytes
        .strip_prefix(&HIRPDAG_MAGIC[..])
        .ok_or(HirpdagDeserializeError::BadMagic)
}

/// Marker type occupying the `version` field of an archive.
///
/// Serializes as `HIRPDAG_FORMAT_VERSION`; deserialization fails eagerly on
/// any other value, before any nodes are decoded (the version is the first
/// archive field).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HirpdagFormatVersion;

impl serde::Serialize for HirpdagFormatVersion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(HIRPDAG_FORMAT_VERSION)
    }
}

impl<'de> serde::Deserialize<'de> for HirpdagFormatVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let version = u32::deserialize(deserializer)?;
        if version != HIRPDAG_FORMAT_VERSION {
            return Err(serde::de::Error::custom(format!(
                "unsupported hirpdag format version {} (supported: {})",
                version, HIRPDAG_FORMAT_VERSION
            )));
        }
        Ok(Self)
    }
}

/// Traversal trait used by the serialization collect phase to register every
/// unique node reachable from the roots, children first (post-order DFS).
///
/// `C` is the collect context generated per hirpdag module by `#[hirpdag_end]`.
/// Follows the same shape as `HirpdagRewritable`: no-op for leaf values,
/// structural for containers, generated for hirpdag types.
pub trait HirpdagCollect<C> {
    fn hirpdag_collect(&self, ctx: &mut C);
}

use crate::base::basic_traits::IsNumber;
impl<C, P: IsNumber> HirpdagCollect<C> for P {
    fn hirpdag_collect(&self, _ctx: &mut C) {}
}

impl<C> HirpdagCollect<C> for String {
    fn hirpdag_collect(&self, _ctx: &mut C) {}
}

impl<C, T: HirpdagCollect<C>> HirpdagCollect<C> for Option<T> {
    fn hirpdag_collect(&self, ctx: &mut C) {
        if let Some(inner) = self {
            inner.hirpdag_collect(ctx);
        }
    }
}

impl<C, T: HirpdagCollect<C>> HirpdagCollect<C> for Vec<T> {
    fn hirpdag_collect(&self, ctx: &mut C) {
        for item in self {
            item.hirpdag_collect(ctx);
        }
    }
}
