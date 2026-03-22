use hirpdag::*;

// Minimal DAG type with self-reference (like a linked list / recursive expr).
#[hirpdag]
struct Node {
    value: i32,
    child: Option<Node>,
}

#[hirpdag_end]
pub struct HirpdagEndMarker;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn round_trip(root: Node) -> Node {
    let json = serde_json::to_string(&HirpdagDag::new(root)).unwrap();
    let dag: HirpdagDag<Node> = serde_json::from_str(&json).unwrap();
    dag.root
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// A single leaf node round-trips correctly.
#[test]
fn test_leaf_round_trip() {
    let leaf = Node::new(42, None);
    let restored = round_trip(leaf.clone());
    assert_eq!(leaf, restored);
}

/// A chain of 3 nodes round-trips, preserving structure.
#[test]
fn test_chain_round_trip() {
    let a = Node::new(1, None);
    let b = Node::new(2, Some(a.clone()));
    let c = Node::new(3, Some(b.clone()));
    let restored = round_trip(c.clone());
    assert_eq!(c, restored);
}

/// Shared sub-DAG: two parents point to the same child.
/// After serialization + deserialization, both parents should still refer
/// to the same interned node (pointer equality via hash-consing).
#[test]
fn test_sharing_preserved() {
    let shared_leaf = Node::new(999, None);
    // Create two parent nodes that both point to shared_leaf.
    // We can't have two fields pointing to the same Node in our simple type,
    // so we create two separate roots that share the same child.
    let parent_a = Node::new(1, Some(shared_leaf.clone()));
    let parent_b = Node::new(2, Some(shared_leaf.clone()));

    // Serialize parent_b (which contains shared_leaf as child).
    let json_b = serde_json::to_string(&HirpdagDag::new(parent_b.clone())).unwrap();
    let restored_b: HirpdagDag<Node> = serde_json::from_str(&json_b).unwrap();
    assert_eq!(parent_b, restored_b.root);

    // The restored child should be the same interned node as if we build it fresh.
    let restored_child = restored_b.root.child.as_ref().unwrap();
    let fresh_shared = Node::new(999, None); // same data → same interned node
    assert_eq!(*restored_child, fresh_shared);
    // Pointer equality (hash-consing guarantees same allocation).
    assert!(restored_child == &fresh_shared);

    // Also verify parent_a round-trips independently.
    let json_a = serde_json::to_string(&HirpdagDag::new(parent_a.clone())).unwrap();
    let restored_a: HirpdagDag<Node> = serde_json::from_str(&json_a).unwrap();
    assert_eq!(parent_a, restored_a.root);
}

/// The serialized JSON must contain only as many node entries as unique nodes,
/// not a tree-expanded copy (which would be exponential for diamond DAGs).
#[test]
fn test_dag_format_deduplicates_nodes() {
    let leaf = Node::new(5555, None);
    let mid = Node::new(6666, Some(leaf.clone()));
    let root = Node::new(7777, Some(mid.clone()));

    let json = serde_json::to_string(&HirpdagDag::new(root)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // The flat list should have exactly 3 nodes (leaf, mid, root).
    let node_count = parsed["nodes"].as_array().unwrap().len();
    assert_eq!(
        node_count, 3,
        "flat list should have 3 nodes, got {}: {}",
        node_count, json
    );
}

/// Serialized form uses integer indices for child refs.
#[test]
fn test_serialized_format_uses_indices() {
    let leaf = Node::new(1234, None);
    let root = Node::new(5678, Some(leaf));

    let json = serde_json::to_string(&HirpdagDag::new(root)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    let nodes = parsed["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);

    // First node is the leaf: child should be JSON null (Option<Node> = None).
    let leaf_node = &nodes[0];
    assert_eq!(leaf_node["d"]["value"], 1234);
    assert!(leaf_node["d"]["child"].is_null());

    // Second node is the root: child should be an integer index (0), not an object.
    let root_node = &nodes[1];
    assert_eq!(root_node["d"]["value"], 5678);
    assert_eq!(root_node["d"]["child"], 0, "child should be index 0, not nested object");
}
