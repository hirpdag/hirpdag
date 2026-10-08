//! The runtime support for `#[hirpdag_module]`.
//!
//! The items here are the ones a user of hirpdag names: node metadata, leaf
//! field types, memoization tables, the rewrite bound and the serialization
//! errors. Everything the generated code needs besides these is in
//! [`__private`], which is not part of the interface.

mod archive;
pub mod field;
mod memoize;
mod meta;
mod reference;
mod rewrite;
mod serialize;

pub use self::field::HirpdagLeaf;
pub use self::memoize::{HirpdagMemoize, HirpdagMemoizeMap};
pub use self::meta::{
    HirpdagMeta, HirpdagMetaCountType, HirpdagMetaFlagType, HirpdagMetaHeightType,
};
pub use self::rewrite::HirpdagRewritable;
pub use self::serialize::{HirpdagDeserializeError, HirpdagSerializeError};

/// Items used by the code `#[hirpdag_module]` generates, and by nothing else.
///
/// Public only because the generated code lives in the user's crate. Not part
/// of hirpdag's interface: these change whenever the generated code does.
#[doc(hidden)]
pub mod __private {
    pub use super::archive::{
        archive_deserialize, archive_deserialize_json, archive_resolve_ref, archive_serialize,
        archive_serialize_json, HirpdagArchive, HirpdagArchiveMember, HirpdagCollectCtx,
        HirpdagCollectNode, HirpdagNoRoots,
    };
    pub use super::meta::HirpdagComputeMeta;
    pub use super::reference::{HirpdagHashconsTable, HirpdagRef, HirpdagStorage, HirpdagStruct};
    pub use super::serialize::{
        HirpdagArchived, HirpdagCollect, HirpdagFormatVersion, HirpdagNodeIndex,
        HirpdagSchemaFingerprint,
    };
}
