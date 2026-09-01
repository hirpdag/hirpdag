//! The DAG-aware archive: collect phase, sessions, node table, entry points.
//!
//! [`HirpdagSerializeError`](crate::base::HirpdagSerializeError) and the format
//! constants live in [`serialize`](crate::base::serialize); this module is the
//! machinery that turns a module's roots into bytes and back.
//!
//! An archive is `version`, then a node table in post-order DFS order (children
//! before parents), then the roots.  A ref is written as a `u64` index into the
//! node table.  Because children always precede parents, one forward pass
//! reconstructs everything: forward references are errors and cycles are
//! unrepresentable.
//!
//! serde's traits carry no user state, so a ref's `Serialize` cannot be handed
//! the index map and its `Deserialize` cannot be handed the nodes reconstructed
//! so far.  Both reach that state through a *session*: a thread-local slot that
//! the entry points here open for the duration of one archive and close on the
//! way out (see `docs/adr/0001-serde-dag-aware-serialization.md`).  A static
//! cannot name a generic parameter, so the two slots are declared by
//! `#[hirpdag_module]` — one pair per module, holding that module's node type —
//! and everything that reads or writes them lives here, reached through
//! [`HirpdagArchive::with_ser_session`] and
//! [`HirpdagArchive::with_de_session`].
//!
//! What a module supplies is one [`HirpdagArchive`] impl plus one
//! [`HirpdagArchiveMember`] impl per data type; the traversal, the session
//! rules, the node table codec and the four entry points are all here.

use crate::base::serialize::HirpdagCollect;
use crate::base::serialize::HirpdagDeserializeError;
use crate::base::serialize::HirpdagFormatVersion;
use crate::base::serialize::HirpdagSchemaFingerprint;
use crate::base::serialize::HirpdagSerializeError;
use crate::base::serialize::{hirpdag_read_binary_header, hirpdag_write_binary_header};

// ==== The interface a module implements

/// The archive schema of one `#[hirpdag_module]` module.
///
/// Implemented by the generated `HirpdagArchiveSchema` marker type, which names
/// the module's node types and gives this module access to its two session
/// slots.  Everything else about archiving is implemented here against this
/// interface.
pub trait HirpdagArchive: Sized {
    /// One entry of the node table: the data of any hashconsed type in the
    /// module (the generated `HirpdagArchiveNode`).
    type Node: serde::Serialize + serde::de::DeserializeOwned;

    /// A reconstructed node of any hashconsed type in the module (the generated
    /// `HirpdagNodeRef`).  Node references resolve their index against a vector
    /// of these.
    type Interned;

    /// The roots of an archive: one vector per `#[hirpdag(root)]` type (the
    /// generated `HirpdagArchiveRoots`).  [`HirpdagNoRoots`] for a module with
    /// no root types.
    type Roots: serde::Serialize
        + serde::de::DeserializeOwned
        + HirpdagCollect<HirpdagCollectCtx<Self::Node>>;

    /// Identifies this module's type definitions; embedded in the binary
    /// header and checked on the way back in.
    fn schema_fingerprint() -> HirpdagSchemaFingerprint;

    /// Re-intern a decoded node through the hash-cons table.
    fn intern(node: Self::Node) -> Self::Interned;

    /// Run `f` on this module's serialization session slot for the current
    /// thread.
    fn with_ser_session<R>(f: impl FnOnce(&mut Option<HirpdagSerSession>) -> R) -> R;

    /// Run `f` on this module's deserialization session slot for the current
    /// thread.
    fn with_de_session<R>(f: impl FnOnce(&mut Option<Vec<Self::Interned>>) -> R) -> R;
}

/// One hashconsed data type's place in its module's archive.
///
/// Implemented by the generated ref type of every `#[hirpdag]` struct, so that
/// [`archive_serialize_ref`] and [`archive_deserialize_ref`] can name the type
/// in their errors and pick it back out of the reconstructed node table.
pub trait HirpdagArchiveMember<A: HirpdagArchive>: Sized + Clone {
    /// The data type's name, used in error messages.
    const TYPE_NAME: &'static str;

    /// This type's node, or `None` if the reconstructed node is a different
    /// type of the module.
    fn hirpdag_archive_member(interned: &A::Interned) -> Option<&Self>;
}

/// The roots of a module with no `#[hirpdag(root)]` type: there is nothing to
/// serialize, and no entry points are generated for such a module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HirpdagNoRoots;

impl<N> HirpdagCollect<HirpdagCollectCtx<N>> for HirpdagNoRoots {
    fn hirpdag_collect(&self, _ctx: &mut HirpdagCollectCtx<N>) {}
}

// ==== Collect phase

/// State of the collect phase: the node table being built, and the nodes
/// already in it.
///
/// Nodes are keyed by creation id, which hash-consing makes a unique name for
/// an interned node, so a node reachable by several paths is registered once.
pub struct HirpdagCollectCtx<N> {
    /// Creation id of each registered node, to its index in `nodes`.
    seen: std::collections::HashMap<u64, u64>,
    /// The node table, in post-order DFS order.
    nodes: Vec<N>,
}

impl<N> HirpdagCollectCtx<N> {
    pub fn new() -> Self {
        Self {
            seen: std::collections::HashMap::new(),
            nodes: Vec::new(),
        }
    }

    /// Register one node, children first, unless it is already registered.
    ///
    /// `collect_children` recurses into the node's fields and `to_node` builds
    /// its node table entry; running them in that order is what makes every
    /// child's index smaller than its parent's.
    pub fn visit<F, G>(&mut self, creation_id: u64, collect_children: F, to_node: G)
    where
        F: FnOnce(&mut Self),
        G: FnOnce() -> N,
    {
        if self.seen.contains_key(&creation_id) {
            return;
        }
        collect_children(self);
        let index = self.nodes.len() as u64;
        self.nodes.push(to_node());
        self.seen.insert(creation_id, index);
    }

    /// The node table and the creation-id-to-index map it was built with.
    fn into_parts(self) -> (Vec<N>, std::collections::HashMap<u64, u64>) {
        (self.nodes, self.seen)
    }
}

impl<N> Default for HirpdagCollectCtx<N> {
    fn default() -> Self {
        Self::new()
    }
}

// ==== Sessions

/// Serialization session state: where each collected node ended up in the node
/// table, so that a ref can be written as its index.
pub struct HirpdagSerSession {
    index_of_creation_id: std::collections::HashMap<u64, u64>,
}

impl HirpdagSerSession {
    /// The node table index of the node with this creation id, if the collect
    /// phase registered it.
    pub fn index_of(&self, creation_id: u64) -> Option<u64> {
        self.index_of_creation_id.get(&creation_id).copied()
    }
}

/// Holds a module's serialization session open, and closes it on drop.
struct SerSessionGuard<A: HirpdagArchive>(std::marker::PhantomData<A>);

impl<A: HirpdagArchive> SerSessionGuard<A> {
    fn open(session: HirpdagSerSession) -> Result<Self, HirpdagSerializeError> {
        A::with_ser_session(|slot| {
            if slot.is_some() {
                return Err(HirpdagSerializeError::SessionActive);
            }
            *slot = Some(session);
            Ok(Self(std::marker::PhantomData))
        })
    }
}

impl<A: HirpdagArchive> Drop for SerSessionGuard<A> {
    fn drop(&mut self) {
        A::with_ser_session(|slot| *slot = None);
    }
}

/// Holds a module's deserialization session open, and closes it on drop.
struct DeSessionGuard<A: HirpdagArchive>(std::marker::PhantomData<A>);

impl<A: HirpdagArchive> DeSessionGuard<A> {
    fn open() -> Result<Self, HirpdagDeserializeError> {
        A::with_de_session(|slot| {
            if slot.is_some() {
                return Err(HirpdagDeserializeError::SessionActive);
            }
            *slot = Some(Vec::new());
            Ok(Self(std::marker::PhantomData))
        })
    }
}

impl<A: HirpdagArchive> Drop for DeSessionGuard<A> {
    fn drop(&mut self) {
        A::with_de_session(|slot| *slot = None);
    }
}

// ==== References, as node table indices

/// Writes a ref as its `u64` index in the node table of the archive being
/// serialized.
///
/// Fails outside a session, so there is no path on which a ref silently
/// expands into a tree.
pub fn archive_serialize_ref<A, T, S>(creation_id: u64, serializer: S) -> Result<S::Ok, S::Error>
where
    A: HirpdagArchive,
    T: HirpdagArchiveMember<A>,
    S: serde::Serializer,
{
    // The index is copied out before serializing it, so nothing runs while the
    // session slot is borrowed.
    let index = A::with_ser_session(|slot| {
        let session = slot.as_ref().ok_or_else(|| {
            format!(
                "hirpdag ref {} serialized outside a hirpdag serialization session",
                T::TYPE_NAME
            )
        })?;
        session.index_of(creation_id).ok_or_else(|| {
            format!(
                "hirpdag ref {} was not collected before serialization",
                T::TYPE_NAME
            )
        })
    });
    match index {
        Ok(index) => serializer.serialize_u64(index),
        Err(message) => Err(<S::Error as serde::ser::Error>::custom(message)),
    }
}

/// Reads a ref: a `u64` index resolved against the nodes reconstructed so far.
///
/// Nodes are stored children-first, so a valid archive only ever references
/// nodes that are already reconstructed.  A forward reference is
/// indistinguishable from an out-of-range index here, and both are rejected.
pub fn archive_deserialize_ref<'de, A, T, D>(deserializer: D) -> Result<T, D::Error>
where
    A: HirpdagArchive,
    T: HirpdagArchiveMember<A>,
    D: serde::Deserializer<'de>,
{
    let index = <u64 as serde::Deserialize>::deserialize(deserializer)?;
    A::with_de_session(|slot| {
        let nodes = slot.as_ref().ok_or_else(|| {
            format!(
                "hirpdag ref {} deserialized outside a hirpdag deserialization session",
                T::TYPE_NAME
            )
        })?;
        if index >= nodes.len() as u64 {
            return Err(format!(
                "hirpdag node index {} is invalid (out of range or forward reference)",
                index
            ));
        }
        T::hirpdag_archive_member(&nodes[index as usize])
            .cloned()
            .ok_or_else(|| format!("hirpdag node type mismatch: expected {}", T::TYPE_NAME))
    })
    .map_err(<D::Error as serde::de::Error>::custom)
}

// ==== The node table

/// The node table on the way out: a plain sequence of nodes.
struct NodeTableOut<'a, A: HirpdagArchive>(&'a [A::Node]);

impl<A: HirpdagArchive> serde::Serialize for NodeTableOut<'_, A> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for node in self.0 {
            seq.serialize_element(node)?;
        }
        seq.end()
    }
}

/// The node table on the way in.
///
/// Each node is interned as soon as it is decoded and pushed onto the session,
/// so later nodes — and then the roots — can resolve references to it in the
/// same forward pass.  The value itself carries nothing: the reconstructed
/// nodes live in the session.
struct NodeTableIn<A>(std::marker::PhantomData<A>);

impl<'de, A: HirpdagArchive> serde::Deserialize<'de> for NodeTableIn<A> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NodeTableVisitor<A>(std::marker::PhantomData<A>);

        impl<'de, A: HirpdagArchive> serde::de::Visitor<'de> for NodeTableVisitor<A> {
            type Value = NodeTableIn<A>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a sequence of hirpdag nodes")
            }

            fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                // Decoding a node resolves the refs in its fields against the
                // session, so it must not run while the session is borrowed.
                while let Some(node) = seq.next_element::<A::Node>()? {
                    let interned = A::intern(node);
                    A::with_de_session(|slot| match slot {
                        Some(nodes) => {
                            nodes.push(interned);
                            Ok(())
                        }
                        None => Err(<S::Error as serde::de::Error>::custom(
                            "hirpdag nodes deserialized outside a hirpdag deserialization session",
                        )),
                    })?;
                }
                Ok(NodeTableIn(std::marker::PhantomData))
            }
        }

        deserializer.deserialize_seq(NodeTableVisitor(std::marker::PhantomData))
    }
}

// ==== The archive

const ARCHIVE_NAME: &str = "HirpdagArchive";
const ARCHIVE_FIELDS: &[&str] = &["version", "nodes", "roots"];

/// An archive on the way out.  Borrows the roots: serializing does not clone
/// the graph.
struct ArchiveOut<'a, A: HirpdagArchive> {
    nodes: &'a [A::Node],
    roots: &'a A::Roots,
}

impl<A: HirpdagArchive> serde::Serialize for ArchiveOut<'_, A> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut archive = serializer.serialize_struct(ARCHIVE_NAME, ARCHIVE_FIELDS.len())?;
        archive.serialize_field(ARCHIVE_FIELDS[0], &HirpdagFormatVersion)?;
        archive.serialize_field(ARCHIVE_FIELDS[1], &NodeTableOut::<A>(self.nodes))?;
        archive.serialize_field(ARCHIVE_FIELDS[2], self.roots)?;
        archive.end()
    }
}

/// An archive on the way in.  Only the roots survive decoding; the nodes have
/// by then been interned and are reachable through the refs the roots hold.
struct ArchiveIn<A: HirpdagArchive> {
    roots: A::Roots,
}

impl<'de, A: HirpdagArchive> serde::Deserialize<'de> for ArchiveIn<A> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ArchiveVisitor<A>(std::marker::PhantomData<A>);

        impl<'de, A: HirpdagArchive> serde::de::Visitor<'de> for ArchiveVisitor<A> {
            type Value = ArchiveIn<A>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a hirpdag archive")
            }

            /// Binary (and any other non-self-describing) format: the fields
            /// arrive in order, which is the order they must be decoded in.
            fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
            where
                S: serde::de::SeqAccess<'de>,
            {
                use serde::de::Error;
                let _version = seq
                    .next_element::<HirpdagFormatVersion>()?
                    .ok_or_else(|| S::Error::invalid_length(0, &"a hirpdag archive"))?;
                let _nodes = seq
                    .next_element::<NodeTableIn<A>>()?
                    .ok_or_else(|| S::Error::invalid_length(1, &"a hirpdag archive"))?;
                let roots = seq
                    .next_element::<A::Roots>()?
                    .ok_or_else(|| S::Error::invalid_length(2, &"a hirpdag archive"))?;
                Ok(ArchiveIn { roots })
            }

            /// Text format: fields are decoded as they are encountered, so a
            /// hand-written archive must put `nodes` before the `roots` that
            /// index into them.
            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                use serde::de::Error;
                let mut version: Option<HirpdagFormatVersion> = None;
                let mut nodes: Option<NodeTableIn<A>> = None;
                let mut roots: Option<A::Roots> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "version" => {
                            if version.is_some() {
                                return Err(M::Error::duplicate_field(ARCHIVE_FIELDS[0]));
                            }
                            version = Some(map.next_value()?);
                        }
                        "nodes" => {
                            if nodes.is_some() {
                                return Err(M::Error::duplicate_field(ARCHIVE_FIELDS[1]));
                            }
                            nodes = Some(map.next_value()?);
                        }
                        "roots" => {
                            if roots.is_some() {
                                return Err(M::Error::duplicate_field(ARCHIVE_FIELDS[2]));
                            }
                            roots = Some(map.next_value()?);
                        }
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                version.ok_or_else(|| M::Error::missing_field(ARCHIVE_FIELDS[0]))?;
                nodes.ok_or_else(|| M::Error::missing_field(ARCHIVE_FIELDS[1]))?;
                let roots = roots.ok_or_else(|| M::Error::missing_field(ARCHIVE_FIELDS[2]))?;
                Ok(ArchiveIn { roots })
            }
        }

        deserializer.deserialize_struct(
            ARCHIVE_NAME,
            ARCHIVE_FIELDS,
            ArchiveVisitor(std::marker::PhantomData),
        )
    }
}

/// Runs the collect phase: post-order DFS from each root, registering every
/// unique reachable node exactly once, children first.
fn collect<A: HirpdagArchive>(roots: &A::Roots) -> (Vec<A::Node>, HirpdagSerSession) {
    let mut ctx = HirpdagCollectCtx::<A::Node>::new();
    roots.hirpdag_collect(&mut ctx);
    let (nodes, index_of_creation_id) = ctx.into_parts();
    (
        nodes,
        HirpdagSerSession {
            index_of_creation_id,
        },
    )
}

// ==== Entry points

/// Serializes the given roots (and every node reachable from them) into the
/// hirpdag binary archive format.  Each unique node is written exactly once,
/// preserving DAG sharing.  The header carries a fingerprint of the module's
/// type definitions.
pub fn archive_serialize<A: HirpdagArchive>(
    roots: &A::Roots,
) -> Result<Vec<u8>, HirpdagSerializeError> {
    let (nodes, session) = collect::<A>(roots);
    let _session = SerSessionGuard::<A>::open(session)?;
    let payload = postcard::to_stdvec(&ArchiveOut::<A> {
        nodes: &nodes,
        roots,
    })
    .map_err(|e| HirpdagSerializeError::Format(e.to_string()))?;
    let mut bytes = hirpdag_write_binary_header(&A::schema_fingerprint())?;
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Deserializes a hirpdag binary archive, re-interning every node through the
/// hash-cons table, and returns the typed roots.  Fails with `SchemaMismatch`
/// if the archive was written by different hirpdag type definitions.
pub fn archive_deserialize<A: HirpdagArchive>(
    bytes: &[u8],
) -> Result<A::Roots, HirpdagDeserializeError> {
    let payload = hirpdag_read_binary_header(bytes, &A::schema_fingerprint())?;
    let _session = DeSessionGuard::<A>::open()?;
    let archive: ArchiveIn<A> = postcard::from_bytes(payload)
        .map_err(|e| HirpdagDeserializeError::Format(e.to_string()))?;
    Ok(archive.roots)
}

/// JSON (text format) variant of [`archive_serialize`].
pub fn archive_serialize_json<A: HirpdagArchive>(
    roots: &A::Roots,
) -> Result<String, HirpdagSerializeError> {
    let (nodes, session) = collect::<A>(roots);
    let _session = SerSessionGuard::<A>::open(session)?;
    serde_json::to_string(&ArchiveOut::<A> {
        nodes: &nodes,
        roots,
    })
    .map_err(|e| HirpdagSerializeError::Format(e.to_string()))
}

/// JSON (text format) variant of [`archive_deserialize`].
pub fn archive_deserialize_json<A: HirpdagArchive>(
    text: &str,
) -> Result<A::Roots, HirpdagDeserializeError> {
    let _session = DeSessionGuard::<A>::open()?;
    let archive: ArchiveIn<A> =
        serde_json::from_str(text).map_err(|e| HirpdagDeserializeError::Format(e.to_string()))?;
    Ok(archive.roots)
}

#[cfg(test)]
mod tests {
    //! The archive exercised through its own interface, with a hand-written
    //! schema standing in for a `#[hirpdag_module]` module.
    //!
    //! The stand-in interns by structural equality and hands out reference
    //! counted handles with creation ids, which is all the archive requires of
    //! a hirpdag module.  What it deliberately is *not* is generated: these
    //! tests fail on a bug in this module, not on a bug in the macro.

    use super::*;

    // ---- A hand-written schema: two data types, both roots.

    #[derive(Clone, Debug, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct TagData {
        label: String,
    }

    #[derive(Clone, Debug, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct ItemData {
        name: String,
        deps: Vec<Item>,
        tag: Option<Tag>,
    }

    /// A handle to an interned node, standing in for a generated ref type.
    macro_rules! toy_ref {
        ($name:ident, $data:ident, $table:ident) => {
            #[derive(Clone, Debug)]
            struct $name(std::rc::Rc<(u64, $data)>);

            impl $name {
                fn new(data: $data) -> Self {
                    $table.with(|table| {
                        if let Some(existing) = table.borrow().get(&data) {
                            return existing.clone();
                        }
                        let id = NEXT_ID.with(|n| {
                            let id = n.get();
                            n.set(id + 1);
                            id
                        });
                        let node = Self(std::rc::Rc::new((id, data.clone())));
                        table.borrow_mut().insert(data, node.clone());
                        node
                    })
                }
                fn creation_id(&self) -> u64 {
                    self.0 .0
                }
                fn data(&self) -> &$data {
                    &self.0 .1
                }
            }

            impl PartialEq for $name {
                fn eq(&self, other: &Self) -> bool {
                    std::rc::Rc::ptr_eq(&self.0, &other.0)
                }
            }
            impl Eq for $name {}
            impl std::hash::Hash for $name {
                fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                    self.creation_id().hash(state)
                }
            }

            thread_local! {
                static $table: std::cell::RefCell<
                    std::collections::HashMap<$data, $name>
                > = std::cell::RefCell::new(std::collections::HashMap::new());
            }
        };
    }

    thread_local! {
        static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    }

    toy_ref!(Item, ItemData, ITEM_TABLE);
    toy_ref!(Tag, TagData, TAG_TABLE);

    // ---- Everything a module supplies to the archive.

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    enum ToyNode {
        Item(ItemData),
        Tag(TagData),
    }

    enum ToyInterned {
        Item(Item),
        Tag(Tag),
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(default)]
    struct ToyRoots {
        item: Vec<Item>,
        tag: Vec<Tag>,
    }

    struct ToySchema;

    thread_local! {
        static SER_SESSION: std::cell::RefCell<Option<HirpdagSerSession>> =
            const { std::cell::RefCell::new(None) };
        static DE_SESSION: std::cell::RefCell<Option<Vec<ToyInterned>>> =
            const { std::cell::RefCell::new(None) };
    }

    impl HirpdagArchive for ToySchema {
        type Node = ToyNode;
        type Interned = ToyInterned;
        type Roots = ToyRoots;

        fn schema_fingerprint() -> HirpdagSchemaFingerprint {
            HirpdagSchemaFingerprint {
                hash: 0x7031_5f73_6368_656d,
                name: "toy".to_string(),
            }
        }

        fn intern(node: Self::Node) -> Self::Interned {
            match node {
                ToyNode::Item(data) => ToyInterned::Item(Item::new(data)),
                ToyNode::Tag(data) => ToyInterned::Tag(Tag::new(data)),
            }
        }

        fn with_ser_session<R>(f: impl FnOnce(&mut Option<HirpdagSerSession>) -> R) -> R {
            SER_SESSION.with(|cell| f(&mut cell.borrow_mut()))
        }

        fn with_de_session<R>(f: impl FnOnce(&mut Option<Vec<Self::Interned>>) -> R) -> R {
            DE_SESSION.with(|cell| f(&mut cell.borrow_mut()))
        }
    }

    impl HirpdagArchiveMember<ToySchema> for Item {
        const TYPE_NAME: &'static str = "Item";
        fn hirpdag_archive_member(interned: &ToyInterned) -> Option<&Self> {
            match interned {
                ToyInterned::Item(item) => Some(item),
                _ => None,
            }
        }
    }

    impl HirpdagArchiveMember<ToySchema> for Tag {
        const TYPE_NAME: &'static str = "Tag";
        fn hirpdag_archive_member(interned: &ToyInterned) -> Option<&Self> {
            match interned {
                ToyInterned::Tag(tag) => Some(tag),
                _ => None,
            }
        }
    }

    macro_rules! toy_ref_serde {
        ($name:ident) => {
            impl serde::Serialize for $name {
                fn serialize<S: serde::Serializer>(
                    &self,
                    serializer: S,
                ) -> Result<S::Ok, S::Error> {
                    archive_serialize_ref::<ToySchema, Self, S>(self.creation_id(), serializer)
                }
            }
            impl<'de> serde::Deserialize<'de> for $name {
                fn deserialize<D: serde::Deserializer<'de>>(
                    deserializer: D,
                ) -> Result<Self, D::Error> {
                    archive_deserialize_ref::<ToySchema, Self, D>(deserializer)
                }
            }
        };
    }

    toy_ref_serde!(Item);
    toy_ref_serde!(Tag);

    type ToyCtx = HirpdagCollectCtx<ToyNode>;

    impl HirpdagCollect<ToyCtx> for Item {
        fn hirpdag_collect(&self, ctx: &mut ToyCtx) {
            ctx.visit(
                self.creation_id(),
                |ctx| self.data().hirpdag_collect(ctx),
                || ToyNode::Item(self.data().clone()),
            );
        }
    }

    impl HirpdagCollect<ToyCtx> for ItemData {
        fn hirpdag_collect(&self, ctx: &mut ToyCtx) {
            self.deps.hirpdag_collect(ctx);
            self.tag.hirpdag_collect(ctx);
        }
    }

    impl HirpdagCollect<ToyCtx> for Tag {
        fn hirpdag_collect(&self, ctx: &mut ToyCtx) {
            ctx.visit(
                self.creation_id(),
                |_ctx| {},
                || ToyNode::Tag(self.data().clone()),
            );
        }
    }

    impl HirpdagCollect<ToyCtx> for ToyRoots {
        fn hirpdag_collect(&self, ctx: &mut ToyCtx) {
            for root in &self.item {
                root.hirpdag_collect(ctx);
            }
            for root in &self.tag {
                root.hirpdag_collect(ctx);
            }
        }
    }

    // ---- Fixtures

    fn item(name: &str, deps: Vec<Item>, tag: Option<Tag>) -> Item {
        Item::new(ItemData {
            name: name.to_string(),
            deps,
            tag,
        })
    }

    fn tag(label: &str) -> Tag {
        Tag::new(TagData {
            label: label.to_string(),
        })
    }

    /// A shared child reached by two paths, plus a node of the second type.
    fn diamond() -> ToyRoots {
        let leaf = item("leaf", vec![], Some(tag("hot")));
        let top = item("top", vec![leaf.clone(), leaf], None);
        ToyRoots {
            item: vec![top],
            ..Default::default()
        }
    }

    // ---- Tests

    #[test]
    fn binary_round_trip_preserves_sharing() {
        let roots = diamond();
        let bytes = archive_serialize::<ToySchema>(&roots).unwrap();
        let out = archive_deserialize::<ToySchema>(&bytes).unwrap();

        // Interning means a round trip through the archive lands on the very
        // same nodes, and the shared child is still one node.
        assert_eq!(out, roots);
        let top = &out.item[0];
        assert!(std::rc::Rc::ptr_eq(
            &top.data().deps[0].0,
            &top.data().deps[1].0
        ));
    }

    #[test]
    fn each_unique_node_written_once() {
        let roots = diamond();
        let (nodes, _session) = collect::<ToySchema>(&roots);
        // tag, leaf, top: the child shared by two paths is written once.
        assert_eq!(nodes.len(), 3);
        // Children precede parents.
        assert!(matches!(&nodes[0], ToyNode::Tag(_)));
        assert!(matches!(&nodes[2], ToyNode::Item(data) if data.name == "top"));
    }

    #[test]
    fn json_round_trip_preserves_sharing() {
        let roots = diamond();
        let text = archive_serialize_json::<ToySchema>(&roots).unwrap();
        let out = archive_deserialize_json::<ToySchema>(&text).unwrap();
        assert_eq!(out, roots);
    }

    #[test]
    fn out_of_range_index_rejected() {
        let text =
            r#"{"version":1,"nodes":[{"Item":{"name":"x","deps":[7],"tag":null}}],"roots":{}}"#;
        let err = archive_deserialize_json::<ToySchema>(text).unwrap_err();
        match err {
            HirpdagDeserializeError::Format(msg) => {
                assert!(msg.contains("invalid"), "unexpected message: {}", msg)
            }
            other => panic!("expected Format error, got {:?}", other),
        }
    }

    #[test]
    fn forward_reference_rejected() {
        // Node 0 references node 1, which is not reconstructed yet.
        let text = r#"{"version":1,"nodes":[
            {"Item":{"name":"parent","deps":[1],"tag":null}},
            {"Item":{"name":"child","deps":[],"tag":null}}
        ],"roots":{}}"#;
        let err = archive_deserialize_json::<ToySchema>(text).unwrap_err();
        assert!(matches!(err, HirpdagDeserializeError::Format(_)));
    }

    #[test]
    fn node_type_mismatch_rejected() {
        // The roots claim node 0 is a Tag, but node 0 is an Item.
        let text = r#"{"version":1,"nodes":[{"Item":{"name":"x","deps":[],"tag":null}}],"roots":{"tag":[0]}}"#;
        let err = archive_deserialize_json::<ToySchema>(text).unwrap_err();
        match err {
            HirpdagDeserializeError::Format(msg) => {
                assert!(msg.contains("type mismatch"), "unexpected message: {}", msg)
            }
            other => panic!("expected Format error, got {:?}", other),
        }
    }

    #[test]
    fn ref_outside_a_session_is_an_error() {
        // Without this a ref would silently expand the DAG into a tree.
        let err = serde_json::to_string(&item("lonely", vec![], None)).unwrap_err();
        assert!(err.to_string().contains("session"), "{}", err);

        let err = serde_json::from_str::<Item>("0").unwrap_err();
        assert!(err.to_string().contains("session"), "{}", err);
    }

    #[test]
    fn sessions_are_not_re_entrant() {
        let roots = diamond();
        let bytes = archive_serialize::<ToySchema>(&roots).unwrap();

        // A session opened while one is already active is refused, and the
        // failure does not close the session that was already open.
        let (_, session) = collect::<ToySchema>(&roots);
        let outer = SerSessionGuard::<ToySchema>::open(session).unwrap();
        let (_, session) = collect::<ToySchema>(&roots);
        assert!(matches!(
            SerSessionGuard::<ToySchema>::open(session),
            Err(HirpdagSerializeError::SessionActive)
        ));
        assert_eq!(
            archive_serialize::<ToySchema>(&roots).unwrap_err(),
            HirpdagSerializeError::SessionActive
        );
        drop(outer);

        let outer = DeSessionGuard::<ToySchema>::open().unwrap();
        assert!(matches!(
            DeSessionGuard::<ToySchema>::open(),
            Err(HirpdagDeserializeError::SessionActive)
        ));
        assert_eq!(
            archive_deserialize::<ToySchema>(&bytes).unwrap_err(),
            HirpdagDeserializeError::SessionActive
        );
        drop(outer);

        // Both slots are closed again, so ordinary use still works.
        archive_serialize::<ToySchema>(&roots).unwrap();
        archive_deserialize::<ToySchema>(&bytes).unwrap();
    }

    #[test]
    fn bad_magic_rejected() {
        let err = archive_deserialize::<ToySchema>(b"not a hirpdag archive").unwrap_err();
        assert_eq!(err, HirpdagDeserializeError::BadMagic);
    }

    #[test]
    fn schema_mismatch_rejected() {
        /// The same data types, declared by a different module.
        struct OtherSchema;

        impl HirpdagArchive for OtherSchema {
            type Node = ToyNode;
            type Interned = ToyInterned;
            type Roots = ToyRoots;
            fn schema_fingerprint() -> HirpdagSchemaFingerprint {
                HirpdagSchemaFingerprint {
                    hash: 0x0ee7_7300_6368_656d,
                    name: "other".to_string(),
                }
            }
            fn intern(node: Self::Node) -> Self::Interned {
                ToySchema::intern(node)
            }
            fn with_ser_session<R>(f: impl FnOnce(&mut Option<HirpdagSerSession>) -> R) -> R {
                ToySchema::with_ser_session(f)
            }
            fn with_de_session<R>(f: impl FnOnce(&mut Option<Vec<Self::Interned>>) -> R) -> R {
                ToySchema::with_de_session(f)
            }
        }

        let bytes = archive_serialize::<ToySchema>(&diamond()).unwrap();
        let err = archive_deserialize::<OtherSchema>(&bytes).unwrap_err();
        match err {
            HirpdagDeserializeError::SchemaMismatch {
                expected_name,
                found_name,
                ..
            } => {
                assert_eq!(found_name, "toy");
                assert_eq!(expected_name, "other");
            }
            other => panic!("expected SchemaMismatch, got {:?}", other),
        }
    }

    #[test]
    fn unsupported_version_rejected() {
        let err = archive_deserialize_json::<ToySchema>(r#"{"version":99,"nodes":[],"roots":{}}"#)
            .unwrap_err();
        match err {
            HirpdagDeserializeError::Format(msg) => {
                assert!(msg.contains("version"), "unexpected message: {}", msg)
            }
            other => panic!("expected Format error, got {:?}", other),
        }
    }

    #[test]
    fn truncated_binary_rejected() {
        let bytes = archive_serialize::<ToySchema>(&diamond()).unwrap();
        let err = archive_deserialize::<ToySchema>(&bytes[..bytes.len() - 1]).unwrap_err();
        assert!(matches!(err, HirpdagDeserializeError::Format(_)));
    }

    #[test]
    fn no_roots_archive_is_empty() {
        struct EmptySchema;

        thread_local! {
            static EMPTY_SER: std::cell::RefCell<Option<HirpdagSerSession>> =
                const { std::cell::RefCell::new(None) };
            static EMPTY_DE: std::cell::RefCell<Option<Vec<ToyInterned>>> =
                const { std::cell::RefCell::new(None) };
        }

        impl HirpdagArchive for EmptySchema {
            type Node = ToyNode;
            type Interned = ToyInterned;
            type Roots = HirpdagNoRoots;
            fn schema_fingerprint() -> HirpdagSchemaFingerprint {
                HirpdagSchemaFingerprint {
                    hash: 0,
                    name: "empty".to_string(),
                }
            }
            fn intern(node: Self::Node) -> Self::Interned {
                ToySchema::intern(node)
            }
            fn with_ser_session<R>(f: impl FnOnce(&mut Option<HirpdagSerSession>) -> R) -> R {
                EMPTY_SER.with(|cell| f(&mut cell.borrow_mut()))
            }
            fn with_de_session<R>(f: impl FnOnce(&mut Option<Vec<Self::Interned>>) -> R) -> R {
                EMPTY_DE.with(|cell| f(&mut cell.borrow_mut()))
            }
        }

        let bytes = archive_serialize::<EmptySchema>(&HirpdagNoRoots).unwrap();
        assert_eq!(
            archive_deserialize::<EmptySchema>(&bytes).unwrap(),
            HirpdagNoRoots
        );
    }
}
