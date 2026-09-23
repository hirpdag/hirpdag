//! The DAG-aware archive: collect phase, encoding, node table, entry points.
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
//! serde's traits carry no user state, so a ref cannot resolve its index inside
//! a `Serialize` impl.  It does not have to: an archive is plain data, and the
//! reference-to-index translation happens in a phase of its own on either side
//! of serde (see `docs/adr/0001-serde-dag-aware-serialization.md`).
//!
//! ```text
//! roots --collect--> node table --encode--> archive --serde--> bytes
//! bytes --serde--> archive --decode (resolve + intern)--> roots
//! ```
//!
//! The collect phase indexes every reachable node; the encode phase turns each
//! node into its [`HirpdagArchived::Archive`] form, where every ref has become
//! an index; serde then sees data with no hirpdag types in it at all.  Coming
//! back, serde decodes that same plain data, and the decode phase walks the
//! node table in order, resolving each node's indices against the nodes already
//! reconstructed and interning the result.  Both phases take the state they
//! need as an argument, so there is no ambient state anywhere: archives nest,
//! and run concurrently, without arrangement.
//!
//! What a module supplies is one [`HirpdagArchive`] impl, one
//! [`HirpdagArchiveMember`] impl per data type, and the
//! [`HirpdagArchived`] impls that name its archived form; the traversal, the
//! node table and the four entry points are all here.

use crate::base::serialize::HirpdagArchived;
use crate::base::serialize::HirpdagCollect;
use crate::base::serialize::HirpdagDeserializeError;
use crate::base::serialize::HirpdagFormatVersion;
use crate::base::serialize::HirpdagNodeIndex;
use crate::base::serialize::HirpdagSchemaFingerprint;
use crate::base::serialize::HirpdagSerializeError;
use crate::base::serialize::{hirpdag_read_binary_header, hirpdag_write_binary_header};

// ==== The interface a module implements

/// The archive schema of one `#[hirpdag_module]` module.
///
/// Implemented by the generated `HirpdagArchiveSchema` marker type, which names
/// the module's node and roots types.  Everything else about archiving is
/// implemented here against this interface.
pub trait HirpdagArchive: Sized {
    /// One entry of the node table: an interned node of any hashconsed type in
    /// the module (the generated `HirpdagNodeRef`).  Node references resolve
    /// their index against a slice of these, and the node's archived form is
    /// one entry of the serialized node table.  The collect phase asks it for
    /// its children.
    type Node: HirpdagArchived<[Self::Node]> + HirpdagCollectNode;

    /// The roots of an archive: one vector per `#[hirpdag(root)]` type (the
    /// generated `HirpdagArchiveRoots`).  [`HirpdagNoRoots`] for a module with
    /// no root types.
    type Roots: HirpdagArchived<[Self::Node]> + HirpdagCollect<HirpdagCollectCtx<Self::Node>>;

    /// Identifies this module's type definitions; embedded in the binary
    /// header and checked on the way back in.
    fn schema_fingerprint() -> HirpdagSchemaFingerprint;
}

/// The archived form of a module's node table entry.
type ArchivedNode<A> =
    <<A as HirpdagArchive>::Node as HirpdagArchived<[<A as HirpdagArchive>::Node]>>::Archive;

/// The archived form of a module's roots.
type ArchivedRoots<A> =
    <<A as HirpdagArchive>::Roots as HirpdagArchived<[<A as HirpdagArchive>::Node]>>::Archive;

/// One hashconsed data type's place in its module's archive.
///
/// Implemented by the generated ref type of every `#[hirpdag]` struct, so that
/// [`archive_resolve_ref`] can name the type in its errors and pick it back out
/// of the reconstructed node table.
pub trait HirpdagArchiveMember<A: HirpdagArchive>: Sized + Clone {
    /// The data type's name, used in error messages.
    const TYPE_NAME: &'static str;

    /// This type's node, or `None` if the reconstructed node is a different
    /// type of the module.
    fn hirpdag_archive_member(node: &A::Node) -> Option<&Self>;
}

/// The roots of a module with no `#[hirpdag(root)]` type: there is nothing to
/// serialize, and no entry points are generated for such a module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HirpdagNoRoots;

impl<N> HirpdagCollect<HirpdagCollectCtx<N>> for HirpdagNoRoots {
    fn hirpdag_collect(&self, _ctx: &mut HirpdagCollectCtx<N>) {}
}

impl<R: ?Sized> HirpdagArchived<R> for HirpdagNoRoots {
    type Archive = HirpdagNoRoots;
    fn hirpdag_to_archive(&self, _index: &HirpdagNodeIndex) -> Result<Self, HirpdagSerializeError> {
        Ok(Self)
    }
    fn hirpdag_from_archive(_archived: Self, _nodes: &R) -> Result<Self, HirpdagDeserializeError> {
        Ok(Self)
    }
}

// ==== Collect phase

/// A node table entry that can name its children: the refs held in its fields.
///
/// Implemented by the module's node type (the generated `HirpdagNodeRef`), by
/// running [`HirpdagCollect`] over the node's data, which hands each child ref
/// to [`HirpdagCollectCtx::visit`].
pub trait HirpdagCollectNode: Sized {
    /// Hand each ref in this node's fields to `ctx`, in field order.
    fn hirpdag_collect_children(&self, ctx: &mut HirpdagCollectCtx<Self>);
}

/// State of the collect phase: the node table being built, the nodes already
/// in it, and the nodes found but not yet expanded.
///
/// Nodes are keyed by creation id, which hash-consing makes a unique name for
/// an interned node, so a node reachable by several paths is registered once.
///
/// The walk is a post-order DFS over an explicit stack rather than the call
/// stack, so its depth is bounded by memory, not by the thread's stack size.
pub struct HirpdagCollectCtx<N> {
    /// Creation id of each registered node, to its index in `nodes`.
    seen: std::collections::HashMap<u64, u64>,
    /// The node table, in post-order DFS order.
    nodes: Vec<N>,
    /// Refs handed to [`visit`](Self::visit) since the walk last took them: the
    /// roots, then the children of the node being expanded, in field order.
    found: Vec<(u64, N)>,
}

/// One entry of the collect walk's explicit stack.
enum CollectStep<N> {
    /// Hand over the node's children, then come back to register it.
    Expand(u64, N),
    /// The node's children are all registered: register the node.
    Register(u64, N),
}

impl<N> HirpdagCollectCtx<N> {
    pub fn new() -> Self {
        Self {
            seen: std::collections::HashMap::new(),
            nodes: Vec::new(),
            found: Vec::new(),
        }
    }

    /// Hand over a ref found in a root or in a node's fields.
    ///
    /// Nothing is walked here: the node is queued for the walk to expand and
    /// register, children first.  `to_node` is only called for a node not
    /// already registered, so an edge to a registered node costs no clone.
    pub fn visit<G>(&mut self, creation_id: u64, to_node: G)
    where
        G: FnOnce() -> N,
    {
        if self.seen.contains_key(&creation_id) {
            return;
        }
        self.found.push((creation_id, to_node()));
    }

    /// Queue the refs found since the last call, so that the first found is
    /// the first taken off the stack.
    fn push_found(&mut self, stack: &mut Vec<CollectStep<N>>) {
        stack.extend(
            self.found
                .drain(..)
                .rev()
                .map(|(creation_id, node)| CollectStep::Expand(creation_id, node)),
        );
    }

    /// The node table, and where in it the encode phase finds each node.
    fn into_parts(self) -> (Vec<N>, HirpdagNodeIndex) {
        (self.nodes, HirpdagNodeIndex::new(self.seen))
    }
}

impl<N: HirpdagCollectNode> HirpdagCollectCtx<N> {
    /// Walk everything reachable from the refs handed over so far, registering
    /// each unique node once, children before parents.
    ///
    /// Visits nodes in the order a recursive post-order DFS would: each ref in
    /// the order it was found, its children fully registered before it.  A node
    /// found again after it was registered is skipped.  (Found again while it
    /// is being expanded would mean it is its own descendant, which interned
    /// nodes cannot be.)
    fn walk(&mut self) {
        let mut stack: Vec<CollectStep<N>> = Vec::new();
        self.push_found(&mut stack);
        while let Some(step) = stack.pop() {
            match step {
                CollectStep::Expand(creation_id, node) => {
                    if self.seen.contains_key(&creation_id) {
                        continue;
                    }
                    node.hirpdag_collect_children(self);
                    stack.push(CollectStep::Register(creation_id, node));
                    self.push_found(&mut stack);
                }
                CollectStep::Register(creation_id, node) => {
                    let index = self.nodes.len() as u64;
                    self.nodes.push(node);
                    self.seen.insert(creation_id, index);
                }
            }
        }
    }
}

impl<N> Default for HirpdagCollectCtx<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs the collect phase: post-order DFS from each root, registering every
/// unique reachable node exactly once, children first.
fn collect<A: HirpdagArchive>(roots: &A::Roots) -> (Vec<A::Node>, HirpdagNodeIndex) {
    let mut ctx = HirpdagCollectCtx::<A::Node>::new();
    roots.hirpdag_collect(&mut ctx);
    ctx.walk();
    ctx.into_parts()
}

// ==== References, as node table indices

/// Resolves a ref: a `u64` index into the nodes reconstructed so far.
///
/// Nodes are stored children-first, so a valid archive only ever references
/// nodes that are already reconstructed.  A forward reference is
/// indistinguishable from an out-of-range index here, and both are rejected.
pub fn archive_resolve_ref<A, T>(
    index: u64,
    nodes: &[A::Node],
) -> Result<T, HirpdagDeserializeError>
where
    A: HirpdagArchive,
    T: HirpdagArchiveMember<A>,
{
    let node = nodes
        .get(usize::try_from(index).unwrap_or(usize::MAX))
        .ok_or(HirpdagDeserializeError::InvalidNodeIndex {
            index,
            available: nodes.len() as u64,
        })?;
    T::hirpdag_archive_member(node)
        .cloned()
        .ok_or(HirpdagDeserializeError::NodeTypeMismatch {
            expected: T::TYPE_NAME,
        })
}

// ==== The archive

/// An archive, in the form serde sees: plain data, with every ref already a
/// node table index.
///
/// Generic over the node and roots types rather than over the schema, so the
/// serde derive's bounds (`N: Serialize`, `R: Serialize`, ...) are exactly the
/// right ones.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Archive<N, R> {
    version: HirpdagFormatVersion,
    nodes: Vec<N>,
    roots: R,
}

/// Collect and encode: from live roots to the plain data serde writes.
fn archive_encode<A: HirpdagArchive>(
    roots: &A::Roots,
) -> Result<Archive<ArchivedNode<A>, ArchivedRoots<A>>, HirpdagSerializeError> {
    let (nodes, index) = collect::<A>(roots);
    Ok(Archive {
        version: HirpdagFormatVersion,
        nodes: nodes
            .iter()
            .map(|node| node.hirpdag_to_archive(&index))
            .collect::<Result<Vec<_>, _>>()?,
        roots: roots.hirpdag_to_archive(&index)?,
    })
}

/// Decode: from the plain data serde read back to live roots.
///
/// The node table is walked in order.  Each node's refs resolve against the
/// nodes reconstructed before it, and the node is interned as soon as it is
/// resolved, so the next node — and then the roots — can reference it.
fn archive_decode<A: HirpdagArchive>(
    archive: Archive<ArchivedNode<A>, ArchivedRoots<A>>,
) -> Result<A::Roots, HirpdagDeserializeError> {
    let mut nodes: Vec<A::Node> = Vec::with_capacity(archive.nodes.len());
    for archived in archive.nodes {
        let node = A::Node::hirpdag_from_archive(archived, nodes.as_slice())?;
        nodes.push(node);
    }
    A::Roots::hirpdag_from_archive(archive.roots, nodes.as_slice())
}

// ==== Entry points

/// Serializes the given roots (and every node reachable from them) into the
/// hirpdag binary archive format.  Each unique node is written exactly once,
/// preserving DAG sharing.  The header carries a fingerprint of the module's
/// type definitions.
pub fn archive_serialize<A: HirpdagArchive>(
    roots: &A::Roots,
) -> Result<Vec<u8>, HirpdagSerializeError> {
    let payload = postcard::to_stdvec(&archive_encode::<A>(roots)?)
        .map_err(|e| HirpdagSerializeError::Format(e.to_string()))?;
    let mut bytes = hirpdag_write_binary_header(&A::schema_fingerprint())?;
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Deserializes a hirpdag binary archive, re-interning every node through the
/// hash-cons table, and returns the typed roots.  Fails with `SchemaMismatch`
/// if the archive was written by different hirpdag type definitions, and
/// with `Format` if the input does not end where the archive does.
pub fn archive_deserialize<A: HirpdagArchive>(
    bytes: &[u8],
) -> Result<A::Roots, HirpdagDeserializeError> {
    let payload = hirpdag_read_binary_header(bytes, &A::schema_fingerprint())?;
    // `postcard::from_bytes` ignores whatever follows the value it decodes,
    // so check for leftovers here, as the JSON path does.
    let (archive, rest): (Archive<ArchivedNode<A>, ArchivedRoots<A>>, &[u8]) =
        postcard::take_from_bytes(payload)
            .map_err(|e| HirpdagDeserializeError::Format(e.to_string()))?;
    if !rest.is_empty() {
        return Err(HirpdagDeserializeError::Format(format!(
            "{} trailing bytes after the archive",
            rest.len()
        )));
    }
    archive_decode::<A>(archive)
}

/// JSON (text format) variant of [`archive_serialize`].
pub fn archive_serialize_json<A: HirpdagArchive>(
    roots: &A::Roots,
) -> Result<String, HirpdagSerializeError> {
    serde_json::to_string(&archive_encode::<A>(roots)?)
        .map_err(|e| HirpdagSerializeError::Format(e.to_string()))
}

/// JSON (text format) variant of [`archive_deserialize`].
pub fn archive_deserialize_json<A: HirpdagArchive>(
    text: &str,
) -> Result<A::Roots, HirpdagDeserializeError> {
    let archive: Archive<ArchivedNode<A>, ArchivedRoots<A>> =
        serde_json::from_str(text).map_err(|e| HirpdagDeserializeError::Format(e.to_string()))?;
    archive_decode::<A>(archive)
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
    //
    // Each data type comes in two forms: the live one, holding refs, and the
    // archived one, holding node table indices.  Only the archived form meets
    // serde.

    #[derive(Clone, Debug, Hash, PartialEq, Eq)]
    struct TagData {
        label: String,
    }

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct ArchivedTagData {
        label: String,
    }

    #[derive(Clone, Debug, Hash, PartialEq, Eq)]
    struct ItemData {
        name: String,
        deps: Vec<Item>,
        tag: Option<Tag>,
    }

    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    struct ArchivedItemData {
        name: String,
        deps: Vec<u64>,
        tag: Option<u64>,
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

    /// An interned node of either type: the live node table.
    #[derive(Clone, Debug)]
    enum ToyNode {
        Item(Item),
        Tag(Tag),
    }

    /// One entry of the serialized node table.
    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
    enum ArchivedToyNode {
        Item(ArchivedItemData),
        Tag(ArchivedTagData),
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct ToyRoots {
        item: Vec<Item>,
        tag: Vec<Tag>,
    }

    #[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
    #[serde(default)]
    struct ArchivedToyRoots {
        item: Vec<u64>,
        tag: Vec<u64>,
    }

    struct ToySchema;

    impl HirpdagArchive for ToySchema {
        type Node = ToyNode;
        type Roots = ToyRoots;

        fn schema_fingerprint() -> HirpdagSchemaFingerprint {
            HirpdagSchemaFingerprint {
                hash: 0x7031_5f73_6368_656d,
                name: "toy".to_string(),
            }
        }
    }

    /// `String` and the containers are archived the same way whatever the
    /// module, so a call has to say which module's node table it resolves
    /// against.  These two pin that down, exactly as the generated pair in a
    /// `#[hirpdag_module]` module does.
    fn encode<T: HirpdagArchived<[ToyNode]>>(
        value: &T,
        index: &HirpdagNodeIndex,
    ) -> Result<T::Archive, HirpdagSerializeError> {
        value.hirpdag_to_archive(index)
    }

    fn decode<T: HirpdagArchived<[ToyNode]>>(
        archived: T::Archive,
        nodes: &[ToyNode],
    ) -> Result<T, HirpdagDeserializeError> {
        T::hirpdag_from_archive(archived, nodes)
    }

    impl HirpdagArchived<[ToyNode]> for ToyNode {
        type Archive = ArchivedToyNode;

        fn hirpdag_to_archive(
            &self,
            index: &HirpdagNodeIndex,
        ) -> Result<Self::Archive, HirpdagSerializeError> {
            Ok(match self {
                ToyNode::Item(item) => ArchivedToyNode::Item(encode(item.data(), index)?),
                ToyNode::Tag(tag) => ArchivedToyNode::Tag(encode(tag.data(), index)?),
            })
        }

        // Resolving and interning in one step: the reconstructed node is what
        // the node table holds, and what later nodes resolve against.
        fn hirpdag_from_archive(
            archived: Self::Archive,
            nodes: &[ToyNode],
        ) -> Result<Self, HirpdagDeserializeError> {
            Ok(match archived {
                ArchivedToyNode::Item(data) => {
                    ToyNode::Item(Item::new(decode::<ItemData>(data, nodes)?))
                }
                ArchivedToyNode::Tag(data) => {
                    ToyNode::Tag(Tag::new(decode::<TagData>(data, nodes)?))
                }
            })
        }
    }

    impl HirpdagArchived<[ToyNode]> for ItemData {
        type Archive = ArchivedItemData;
        fn hirpdag_to_archive(
            &self,
            index: &HirpdagNodeIndex,
        ) -> Result<Self::Archive, HirpdagSerializeError> {
            Ok(ArchivedItemData {
                name: encode(&self.name, index)?,
                deps: encode(&self.deps, index)?,
                tag: encode(&self.tag, index)?,
            })
        }
        fn hirpdag_from_archive(
            archived: Self::Archive,
            nodes: &[ToyNode],
        ) -> Result<Self, HirpdagDeserializeError> {
            Ok(ItemData {
                name: decode::<String>(archived.name, nodes)?,
                deps: decode::<Vec<Item>>(archived.deps, nodes)?,
                tag: decode::<Option<Tag>>(archived.tag, nodes)?,
            })
        }
    }

    impl HirpdagArchived<[ToyNode]> for TagData {
        type Archive = ArchivedTagData;
        fn hirpdag_to_archive(
            &self,
            index: &HirpdagNodeIndex,
        ) -> Result<Self::Archive, HirpdagSerializeError> {
            Ok(ArchivedTagData {
                label: encode(&self.label, index)?,
            })
        }
        fn hirpdag_from_archive(
            archived: Self::Archive,
            nodes: &[ToyNode],
        ) -> Result<Self, HirpdagDeserializeError> {
            Ok(TagData {
                label: decode::<String>(archived.label, nodes)?,
            })
        }
    }

    impl HirpdagArchived<[ToyNode]> for ToyRoots {
        type Archive = ArchivedToyRoots;
        fn hirpdag_to_archive(
            &self,
            index: &HirpdagNodeIndex,
        ) -> Result<Self::Archive, HirpdagSerializeError> {
            Ok(ArchivedToyRoots {
                item: encode(&self.item, index)?,
                tag: encode(&self.tag, index)?,
            })
        }
        fn hirpdag_from_archive(
            archived: Self::Archive,
            nodes: &[ToyNode],
        ) -> Result<Self, HirpdagDeserializeError> {
            Ok(ToyRoots {
                item: decode::<Vec<Item>>(archived.item, nodes)?,
                tag: decode::<Vec<Tag>>(archived.tag, nodes)?,
            })
        }
    }

    /// A ref: its own creation id's index on the way out, a lookup in the
    /// reconstructed node table on the way back.
    macro_rules! toy_ref_archived {
        ($name:ident, $variant:ident) => {
            impl HirpdagArchiveMember<ToySchema> for $name {
                const TYPE_NAME: &'static str = stringify!($name);
                fn hirpdag_archive_member(node: &ToyNode) -> Option<&Self> {
                    match node {
                        ToyNode::$variant(inner) => Some(inner),
                        _ => None,
                    }
                }
            }

            impl HirpdagArchived<[ToyNode]> for $name {
                type Archive = u64;
                fn hirpdag_to_archive(
                    &self,
                    index: &HirpdagNodeIndex,
                ) -> Result<u64, HirpdagSerializeError> {
                    index.index_of(self.creation_id(), Self::TYPE_NAME)
                }
                fn hirpdag_from_archive(
                    archived: u64,
                    nodes: &[ToyNode],
                ) -> Result<Self, HirpdagDeserializeError> {
                    archive_resolve_ref::<ToySchema, Self>(archived, nodes)
                }
            }
        };
    }

    toy_ref_archived!(Item, Item);
    toy_ref_archived!(Tag, Tag);

    type ToyCtx = HirpdagCollectCtx<ToyNode>;

    impl HirpdagCollect<ToyCtx> for Item {
        fn hirpdag_collect(&self, ctx: &mut ToyCtx) {
            ctx.visit(self.creation_id(), || ToyNode::Item(self.clone()));
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
            ctx.visit(self.creation_id(), || ToyNode::Tag(self.clone()));
        }
    }

    impl HirpdagCollectNode for ToyNode {
        fn hirpdag_collect_children(&self, ctx: &mut ToyCtx) {
            match self {
                ToyNode::Item(item) => item.data().hirpdag_collect(ctx),
                ToyNode::Tag(_) => {}
            }
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
        let archive = archive_encode::<ToySchema>(&roots).unwrap();
        // tag, leaf, top: the child shared by two paths is written once.
        assert_eq!(archive.nodes.len(), 3);
        // Children precede parents.
        assert!(matches!(&archive.nodes[0], ArchivedToyNode::Tag(_)));
        assert!(matches!(&archive.nodes[2], ArchivedToyNode::Item(data) if data.name == "top"));
        // And a parent names its children by index, not by value.
        assert!(
            matches!(&archive.nodes[2], ArchivedToyNode::Item(data) if data.deps == vec![1, 1])
        );
    }

    /// The node table is in the order a recursive post-order DFS registers the
    /// nodes: roots in order, each node's refs in field order, a child before
    /// its parent, and a node reached again skipped.
    #[test]
    fn node_table_is_in_post_order() {
        let hot = tag("order_hot");
        let a = item("order_a", vec![], Some(hot.clone()));
        let b = item("order_b", vec![a.clone()], None);
        let c = item("order_c", vec![a.clone(), b.clone()], Some(hot.clone()));
        let roots = ToyRoots {
            item: vec![c, b],
            tag: vec![hot, tag("order_cold")],
        };
        let archive = archive_encode::<ToySchema>(&roots).unwrap();
        let order: Vec<&str> = archive
            .nodes
            .iter()
            .map(|node| match node {
                ArchivedToyNode::Item(data) => data.name.as_str(),
                ArchivedToyNode::Tag(data) => data.label.as_str(),
            })
            .collect();
        assert_eq!(
            order,
            vec!["order_hot", "order_a", "order_b", "order_c", "order_cold"]
        );
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
        assert_eq!(
            err,
            HirpdagDeserializeError::InvalidNodeIndex {
                index: 7,
                available: 0
            }
        );
    }

    #[test]
    fn forward_reference_rejected() {
        // Node 0 references node 1, which is not reconstructed yet.
        let text = r#"{"version":1,"nodes":[
            {"Item":{"name":"parent","deps":[1],"tag":null}},
            {"Item":{"name":"child","deps":[],"tag":null}}
        ],"roots":{}}"#;
        let err = archive_deserialize_json::<ToySchema>(text).unwrap_err();
        assert_eq!(
            err,
            HirpdagDeserializeError::InvalidNodeIndex {
                index: 1,
                available: 0
            }
        );
    }

    #[test]
    fn node_type_mismatch_rejected() {
        // The roots claim node 0 is a Tag, but node 0 is an Item.
        let text = r#"{"version":1,"nodes":[{"Item":{"name":"x","deps":[],"tag":null}}],"roots":{"tag":[0]}}"#;
        let err = archive_deserialize_json::<ToySchema>(text).unwrap_err();
        assert_eq!(
            err,
            HirpdagDeserializeError::NodeTypeMismatch { expected: "Tag" }
        );
    }

    #[test]
    fn archives_do_not_interfere() {
        // Nothing about an archive is ambient, so any number of them can be
        // in progress at once: two encodes and their decodes, interleaved.
        let first = diamond();
        let second = ToyRoots {
            tag: vec![tag("second")],
            ..Default::default()
        };

        let first_encoded = archive_encode::<ToySchema>(&first).unwrap();
        let second_encoded = archive_encode::<ToySchema>(&second).unwrap();
        let second_out = archive_decode::<ToySchema>(second_encoded).unwrap();
        let first_out = archive_decode::<ToySchema>(first_encoded).unwrap();

        assert_eq!(first_out, first);
        assert_eq!(second_out, second);
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
            type Roots = ToyRoots;
            fn schema_fingerprint() -> HirpdagSchemaFingerprint {
                HirpdagSchemaFingerprint {
                    hash: 0x0ee7_7300_6368_656d,
                    name: "other".to_string(),
                }
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
    fn trailing_bytes_rejected() {
        let mut bytes = archive_serialize::<ToySchema>(&diamond()).unwrap();
        bytes.extend_from_slice(b"garbage");
        let err = archive_deserialize::<ToySchema>(&bytes).unwrap_err();
        assert_eq!(
            err,
            HirpdagDeserializeError::Format("7 trailing bytes after the archive".to_string())
        );
    }

    #[test]
    fn uncollected_ref_is_an_error() {
        // The generated collect and encode walks visit the same fields, so
        // this cannot happen through the macro; encoding a ref checks it
        // rather than assuming it.
        let err = item("uncollected", vec![], None)
            .hirpdag_to_archive(&HirpdagNodeIndex::default())
            .unwrap_err();
        assert_eq!(err, HirpdagSerializeError::NotCollected("Item"));
    }

    #[test]
    fn no_roots_archive_is_empty() {
        struct EmptySchema;

        impl HirpdagArchive for EmptySchema {
            type Node = ToyNode;
            type Roots = HirpdagNoRoots;
            fn schema_fingerprint() -> HirpdagSchemaFingerprint {
                HirpdagSchemaFingerprint {
                    hash: 0,
                    name: "empty".to_string(),
                }
            }
        }

        let bytes = archive_serialize::<EmptySchema>(&HirpdagNoRoots).unwrap();
        assert_eq!(
            archive_deserialize::<EmptySchema>(&bytes).unwrap(),
            HirpdagNoRoots
        );
    }
}
