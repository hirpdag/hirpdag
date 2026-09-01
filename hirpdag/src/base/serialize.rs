// ==== Serialization Base
//
// DAG-aware serialization support shared by all hirpdag modules.
//
// The archive layout (generated per `#[hirpdag_module]` module) is:
//   version, then a node table in post-order DFS order (children before
//   parents), then a list of roots. `HirpdagRef` fields are encoded as u64
//   indices into the node table. Because children always precede parents,
//   a single forward pass reconstructs everything, forward references are
//   errors, and cycles are unrepresentable.
//
// This module holds the format-agnostic pieces: the two traversal traits
// (collect and archive encoding), the error types, the format version marker,
// and the binary magic prefix.

/// Magic prefix identifying a hirpdag binary archive.
///
/// Modelled on the PNG signature (`\x89PNG\r\n\x1a\n`):
/// - `\x89` has the high bit set, marking the file as binary (and catching
///   transfers that strip to 7 bits);
/// - `HPDG` names the format for anyone inspecting the file as text;
/// - `\r` catches CR-to-LF and CR-stripping text-mode translations;
/// - `\x1a` (Ctrl+Z) stops accidental terminal/DOS `type` output;
/// - the trailing `\n` catches LF-to-CRLF translation, and means the magic
///   reads as a tidy single "HPDG" line when opened in a text viewer.
pub const HIRPDAG_MAGIC: &[u8; 8] = b"\x89HPDG\r\x1a\n";

/// Version of the hirpdag archive format written by this library.
pub const HIRPDAG_FORMAT_VERSION: u32 = 1;

/// Error type for hirpdag serialization.
///
/// Distinct from [`HirpdagDeserializeError`], mirroring serde's separation of
/// `serde::ser::Error` and `serde::de::Error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirpdagSerializeError {
    /// A reference was encoded without the node it names having been
    /// registered by the collect phase. Unreachable through the generated
    /// code, where the collect walk and the encode walk visit the same
    /// fields; it exists so encoding a reference is a checked operation
    /// rather than an unchecked map lookup.
    NotCollected(&'static str),
    /// An underlying format error (postcard/serde_json).
    Format(String),
}

impl std::fmt::Display for HirpdagSerializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotCollected(type_name) => write!(
                f,
                "hirpdag: reference {} was not collected before serialization",
                type_name
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
    /// The binary archive was written by a different set of hirpdag type
    /// definitions than the ones trying to read it.
    SchemaMismatch {
        expected_hash: u64,
        expected_name: String,
        found_hash: u64,
        found_name: String,
    },
    /// A node reference names an index that is not a node reconstructed so
    /// far: out of range, or a forward reference (nodes are stored children
    /// first, so a node may only name nodes before it).
    InvalidNodeIndex {
        index: u64,
        /// How many nodes were reconstructed when the reference was resolved.
        available: u64,
    },
    /// A node reference resolved to a node of a different hirpdag type.
    NodeTypeMismatch { expected: &'static str },
    /// An underlying format error (postcard/serde_json), including
    /// unsupported format versions and truncated input.
    Format(String),
}

impl std::fmt::Display for HirpdagDeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "hirpdag: not a hirpdag binary archive (bad magic)"),
            Self::SchemaMismatch {
                expected_hash,
                expected_name,
                found_hash,
                found_name,
            } => write!(
                f,
                "hirpdag: schema mismatch: archive was written by \"{}\" (hash {:#018x}) \
                 but is being read by \"{}\" (hash {:#018x})",
                found_name, found_hash, expected_name, expected_hash
            ),
            Self::InvalidNodeIndex { index, available } => write!(
                f,
                "hirpdag: node index {} is invalid (out of range or a forward \
                 reference; {} nodes reconstructed so far)",
                index, available
            ),
            Self::NodeTypeMismatch { expected } => {
                write!(f, "hirpdag: node type mismatch: expected {}", expected)
            }
            Self::Format(msg) => write!(f, "hirpdag: {}", msg),
        }
    }
}

impl std::error::Error for HirpdagDeserializeError {}

/// Identifies the set of hirpdag type definitions that wrote a binary
/// archive.
///
/// `hash` is a stable hash of the type definitions (names, field names and
/// types, variant names and payloads, root markers, in declaration order) of
/// every hirpdag type in the module, computed at macro expansion time.
/// `name` is a human-readable identifier of those definitions (the defining
/// crate's package name and the list of type names) carried purely for
/// debuggability of mismatch errors; only `hash` decides equality.
///
/// In the binary format this fingerprint sits in the header, between the
/// magic prefix and the archive payload. The JSON format deliberately omits
/// it so JSON stays hand-editable.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HirpdagSchemaFingerprint {
    pub hash: u64,
    pub name: String,
}

/// Writes the binary archive header: magic prefix, then the schema
/// fingerprint. The archive payload is appended after this.
pub fn hirpdag_write_binary_header(
    fingerprint: &HirpdagSchemaFingerprint,
) -> Result<Vec<u8>, HirpdagSerializeError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(HIRPDAG_MAGIC);
    let fp = postcard::to_stdvec(fingerprint)
        .map_err(|e| HirpdagSerializeError::Format(e.to_string()))?;
    bytes.extend_from_slice(&fp);
    Ok(bytes)
}

/// Validates the binary archive header (magic prefix and schema fingerprint)
/// and returns the remaining archive payload.
pub fn hirpdag_read_binary_header<'a>(
    bytes: &'a [u8],
    expected: &HirpdagSchemaFingerprint,
) -> Result<&'a [u8], HirpdagDeserializeError> {
    let payload = hirpdag_strip_magic(bytes)?;
    let (found, rest): (HirpdagSchemaFingerprint, &[u8]) = postcard::take_from_bytes(payload)
        .map_err(|e| HirpdagDeserializeError::Format(e.to_string()))?;
    if found.hash != expected.hash {
        return Err(HirpdagDeserializeError::SchemaMismatch {
            expected_hash: expected.hash,
            expected_name: expected.name.clone(),
            found_hash: found.hash,
            found_name: found.name,
        });
    }
    Ok(rest)
}

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
/// `C` is the collect context generated per `#[hirpdag_module]` module.
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

/// Where the collect phase put each node: creation id to node table index.
///
/// Hash-consing makes a creation id a unique name for an interned node, so
/// one map covers every hirpdag type in a module.  Built by the collect
/// phase and handed to the encode phase; a reference encodes as the index
/// this returns for it.
#[derive(Debug, Default)]
pub struct HirpdagNodeIndex {
    index_of_creation_id: std::collections::HashMap<u64, u64>,
}

impl HirpdagNodeIndex {
    pub fn new(index_of_creation_id: std::collections::HashMap<u64, u64>) -> Self {
        Self {
            index_of_creation_id,
        }
    }

    /// The node table index of the node with this creation id.
    ///
    /// `type_name` names the referencing type in the error, which the
    /// generated code cannot produce: everything the encode walk reaches was
    /// registered by the collect walk over the same fields.
    pub fn index_of(
        &self,
        creation_id: u64,
        type_name: &'static str,
    ) -> Result<u64, HirpdagSerializeError> {
        self.index_of_creation_id
            .get(&creation_id)
            .copied()
            .ok_or(HirpdagSerializeError::NotCollected(type_name))
    }
}

/// How a value is represented inside an archive.
///
/// The archive is plain data: a value's [`Archive`](Self::Archive) form is
/// the same value with every hirpdag reference replaced by the `u64` index
/// of the node it names.  Only that form is handed to serde, so the byte
/// format never has to know about references and a reference never has a
/// serde impl that could expand a DAG into a tree.
///
/// The two directions run in the two phases either side of serde:
/// [`hirpdag_to_archive`](Self::hirpdag_to_archive) after the collect phase
/// has indexed every node, and
/// [`hirpdag_from_archive`](Self::hirpdag_from_archive) as the decoded node
/// table is walked back into interned nodes.  Both take their state as an
/// argument, so neither needs any ambient state.
///
/// `R` is what a reference resolves against: the reconstructed nodes so far
/// (`[HirpdagNodeRef]` for a generated module).  Follows the same shape as
/// [`HirpdagCollect`]: identity for leaf values, structural for containers,
/// generated for hirpdag types.
pub trait HirpdagArchived<R: ?Sized>: Sized {
    /// This value's form inside an archive.
    type Archive: serde::Serialize + serde::de::DeserializeOwned;

    /// Encode, resolving every reference to its node table index.
    fn hirpdag_to_archive(
        &self,
        index: &HirpdagNodeIndex,
    ) -> Result<Self::Archive, HirpdagSerializeError>;

    /// Decode, resolving every node index against the nodes in `nodes`.
    fn hirpdag_from_archive(
        archived: Self::Archive,
        nodes: &R,
    ) -> Result<Self, HirpdagDeserializeError>;
}

impl<R: ?Sized, P> HirpdagArchived<R> for P
where
    P: IsNumber + Copy + serde::Serialize + serde::de::DeserializeOwned,
{
    type Archive = P;
    fn hirpdag_to_archive(&self, _index: &HirpdagNodeIndex) -> Result<P, HirpdagSerializeError> {
        Ok(*self)
    }
    fn hirpdag_from_archive(archived: P, _nodes: &R) -> Result<P, HirpdagDeserializeError> {
        Ok(archived)
    }
}

impl<R: ?Sized> HirpdagArchived<R> for String {
    type Archive = String;
    fn hirpdag_to_archive(
        &self,
        _index: &HirpdagNodeIndex,
    ) -> Result<String, HirpdagSerializeError> {
        Ok(self.clone())
    }
    fn hirpdag_from_archive(
        archived: String,
        _nodes: &R,
    ) -> Result<String, HirpdagDeserializeError> {
        Ok(archived)
    }
}

impl<R: ?Sized, T: HirpdagArchived<R>> HirpdagArchived<R> for Option<T> {
    type Archive = Option<T::Archive>;
    fn hirpdag_to_archive(
        &self,
        index: &HirpdagNodeIndex,
    ) -> Result<Self::Archive, HirpdagSerializeError> {
        self.as_ref()
            .map(|v| v.hirpdag_to_archive(index))
            .transpose()
    }
    fn hirpdag_from_archive(
        archived: Self::Archive,
        nodes: &R,
    ) -> Result<Self, HirpdagDeserializeError> {
        archived
            .map(|v| T::hirpdag_from_archive(v, nodes))
            .transpose()
    }
}

impl<R: ?Sized, T: HirpdagArchived<R>> HirpdagArchived<R> for Vec<T> {
    type Archive = Vec<T::Archive>;
    fn hirpdag_to_archive(
        &self,
        index: &HirpdagNodeIndex,
    ) -> Result<Self::Archive, HirpdagSerializeError> {
        self.iter().map(|v| v.hirpdag_to_archive(index)).collect()
    }
    fn hirpdag_from_archive(
        archived: Self::Archive,
        nodes: &R,
    ) -> Result<Self, HirpdagDeserializeError> {
        archived
            .into_iter()
            .map(|v| T::hirpdag_from_archive(v, nodes))
            .collect()
    }
}
