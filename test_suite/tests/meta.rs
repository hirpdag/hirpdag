// The metadata cached on every interned node (count, height, flags), as the
// generated code computes it. The `HirpdagMeta` arithmetic is unit tested in
// hirpdag::base::meta; this checks how the generated `hirpdag_compute_meta`
// folds a node's fields: numbers, strings, `Option`, `Vec`, enum payloads and
// child nodes.

use hirpdag::base::HirpdagComputeMeta;
use hirpdag::*;

#[hirpdag_module]
mod datamodel {
    #[hirpdag]
    pub struct Tree {
        pub label: u32,
        pub children: Vec<Tree>,
    }

    #[hirpdag]
    pub enum Payload {
        Plain(u32),
        Subtree(Tree),
    }

    #[hirpdag]
    pub struct Holder {
        pub payload: Payload,
        pub extra: Option<Tree>,
    }
}

use datamodel::*;

fn count_and_height<T: HirpdagComputeMeta>(node: &T) -> (u32, u16) {
    let meta = node.hirpdag_compute_meta();
    assert_eq!(meta.get_flags(), 0, "no type here sets flags");
    (meta.get_count(), meta.get_height())
}

fn leaf(label: u32) -> Tree {
    Tree::new(label, vec![])
}

#[test]
fn a_node_without_children_counts_itself() {
    assert_eq!(count_and_height(&leaf(1)), (1, 1));
}

#[test]
fn a_chain_counts_every_node() {
    let chain = Tree::new(3, vec![Tree::new(2, vec![leaf(1)])]);
    assert_eq!(count_and_height(&chain), (3, 3));
}

#[test]
fn siblings_add_counts_and_take_the_taller_height() {
    let tall = Tree::new(2, vec![leaf(1)]);
    let parent = Tree::new(10, vec![tall, leaf(3)]);
    assert_eq!(count_and_height(&parent), (4, 3));
}

/// `count` is the size of the node's subtree unfolded into a tree: a child
/// shared by several parents is counted once per parent, not once per DAG.
#[test]
fn a_shared_child_is_counted_once_per_reference() {
    let shared = leaf(1);
    let left = Tree::new(2, vec![shared.clone()]);
    let right = Tree::new(3, vec![shared.clone()]);
    let top = Tree::new(4, vec![left, right, shared]);
    assert_eq!(count_and_height(&top), (6, 3));
}

#[test]
fn leaf_fields_contribute_nothing() {
    let holder = Holder::new(Payload::Plain(7), None);
    assert_eq!(count_and_height(&holder), (1, 1));
}

#[test]
fn enum_payloads_and_options_pass_their_contents_through() {
    let chain = Tree::new(2, vec![leaf(1)]);
    let holder = Holder::new(Payload::Subtree(chain), Some(leaf(5)));
    assert_eq!(count_and_height(&holder), (4, 3));
}
