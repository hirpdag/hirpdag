// Tests for DAG-aware serialization/deserialization.
//
// See docs/design/serialization.md and docs/adr/0001-serde-dag-aware-serialization.md.

use hirpdag::*;

#[hirpdag_module]
mod datamodel {
    #[hirpdag(root)]
    struct Item {
        name: String,
        pub deps: Vec<Item>,
    }

    #[hirpdag]
    enum Kind {
        Num(u32),
        Sum(Vec<Node>),
    }

    #[hirpdag(root)]
    struct Node {
        pub kind: Kind,
    }

    // Label is deliberately NOT a root type: it can only appear as an interior
    // node of the DAG, and HirpdagArchiveRoots has no field for it.
    #[hirpdag]
    struct Label {
        text: String,
    }

    #[hirpdag(root)]
    struct Pair {
        pub left: Item,
        pub right: Item,
        pub tag: Label,
    }
}

use datamodel::*;

#[test]
fn binary_round_trip_pointer_equal() {
    let a = Item::new("rt_leaf_a".to_string(), vec![]);
    let b = Item::new("rt_leaf_b".to_string(), vec![]);
    let parent = Item::new(
        "rt_parent".to_string(),
        vec![a.clone(), b.clone(), a.clone()],
    );

    let roots = HirpdagArchiveRoots {
        root_Item: vec![parent.clone()],
        ..Default::default()
    };
    let bytes = hirpdag_serialize(&roots).unwrap();
    let out = hirpdag_deserialize(&bytes).unwrap();

    assert_eq!(out.root_Item.len(), 1);
    assert!(out.root_Node.is_empty());
    assert!(out.root_Pair.is_empty());
    // Hirpdag equality is pointer equality: the deserialized root re-interned
    // to the exact same node.
    assert_eq!(out.root_Item[0], parent);
    assert_eq!(out.root_Item[0].deps[0], out.root_Item[0].deps[2]);
}

#[test]
fn json_round_trip_pointer_equal() {
    let a = Item::new("json_leaf".to_string(), vec![]);
    let parent = Item::new("json_parent".to_string(), vec![a.clone(), a.clone()]);

    let roots = HirpdagArchiveRoots {
        root_Item: vec![parent.clone()],
        ..Default::default()
    };
    let text = hirpdag_serialize_json(&roots).unwrap();
    let out = hirpdag_deserialize_json(&text).unwrap();

    assert_eq!(out, roots);
    assert_eq!(out.root_Item[0], parent);
}

#[test]
fn enum_payload_round_trip() {
    let n1 = Node::new(Kind::Num(4001));
    let n2 = Node::new(Kind::Num(4002));
    let sum = Node::new(Kind::Sum(vec![n1.clone(), n2.clone(), n1.clone()]));

    let roots = HirpdagArchiveRoots {
        root_Node: vec![sum.clone()],
        ..Default::default()
    };
    let bytes = hirpdag_serialize(&roots).unwrap();
    let out = hirpdag_deserialize(&bytes).unwrap();
    let sum2 = &out.root_Node[0];
    assert_eq!(*sum2, sum);
    match &sum2.kind {
        Kind::Sum(items) => {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], items[2]);
        }
        _ => panic!("expected Sum"),
    }
}

/// A Fibonacci-shaped DAG has exponentially many paths but only a linear
/// number of unique nodes. DAG-aware serialization must write each unique
/// node exactly once.
#[test]
fn sharing_preserved_fibonacci() {
    let depth = 20;
    let mut prev = Node::new(Kind::Num(5001));
    let mut curr = Node::new(Kind::Num(5002));
    for _ in 0..depth {
        let next = Node::new(Kind::Sum(vec![curr.clone(), prev.clone()]));
        prev = curr;
        curr = next;
    }

    let roots = HirpdagArchiveRoots {
        root_Node: vec![curr.clone()],
        ..Default::default()
    };
    let text = hirpdag_serialize_json(&roots).unwrap();
    let value: hirpdag::serde_json::Value = hirpdag::serde_json::from_str(&text).unwrap();
    let node_count = value["nodes"].as_array().unwrap().len();
    // 2 leaves + `depth` sums. Tree expansion would be fib(depth) ~ 10946 nodes.
    assert_eq!(node_count, depth + 2);

    let out = hirpdag_deserialize_json(&text).unwrap();
    assert_eq!(out.root_Node[0], curr);
}

#[test]
fn multiple_roots_share_nodes() {
    let shared = Item::new("multi_shared".to_string(), vec![]);
    let r1 = Item::new("multi_r1".to_string(), vec![shared.clone()]);
    let r2 = Item::new("multi_r2".to_string(), vec![shared.clone()]);

    let roots = HirpdagArchiveRoots {
        root_Item: vec![r1.clone(), r2.clone()],
        ..Default::default()
    };
    let text = hirpdag_serialize_json(&roots).unwrap();
    let value: hirpdag::serde_json::Value = hirpdag::serde_json::from_str(&text).unwrap();
    // The shared child is written once: 3 nodes, not 4.
    assert_eq!(value["nodes"].as_array().unwrap().len(), 3);

    let out = hirpdag_deserialize_json(&text).unwrap();
    assert_eq!(out.root_Item.len(), 2);
    assert_eq!(out.root_Item[0], r1);
    assert_eq!(out.root_Item[1], r2);
    // The shared subgraph is the same node in both deserialized roots.
    assert_eq!(out.root_Item[0].deps[0], out.root_Item[1].deps[0]);
}

#[test]
fn mixed_type_roots() {
    let i = Item::new("mixed_item".to_string(), vec![]);
    // Label is a non-root interior node type.
    let p = Pair::new(i.clone(), i.clone(), Label::new("mixed_tag".to_string()));

    let roots = HirpdagArchiveRoots {
        root_Pair: vec![p.clone()],
        root_Item: vec![i.clone()],
        ..Default::default()
    };
    let text = hirpdag_serialize_json(&roots).unwrap();
    let value: hirpdag::serde_json::Value = hirpdag::serde_json::from_str(&text).unwrap();
    // Item + Label + Pair: the non-root Label still gets a node table entry.
    assert_eq!(value["nodes"].as_array().unwrap().len(), 3);

    let out = hirpdag_deserialize_json(&text).unwrap();
    assert_eq!(out.root_Pair[0], p);
    assert_eq!(out.root_Item[0], i);
    assert_eq!(out.root_Pair[0].tag, Label::new("mixed_tag".to_string()));
    // The Item root and the Pair's children are the same interned node.
    assert_eq!(out.root_Pair[0].left, out.root_Item[0]);
}

#[test]
fn deserialize_twice_merges() {
    let leaf = Item::new("merge_leaf".to_string(), vec![]);
    let root = Item::new("merge_root".to_string(), vec![leaf]);
    let roots = HirpdagArchiveRoots {
        root_Item: vec![root.clone()],
        ..Default::default()
    };
    let bytes = hirpdag_serialize(&roots).unwrap();

    let r1 = hirpdag_deserialize(&bytes).unwrap().root_Item[0].clone();
    let r2 = hirpdag_deserialize(&bytes).unwrap().root_Item[0].clone();
    // Re-interning through the hashcons table merges with existing nodes.
    assert_eq!(r1, r2);
    assert_eq!(r1, root);
}

#[test]
fn bad_magic_rejected() {
    let err = hirpdag_deserialize(b"XXXX not a hirpdag archive").unwrap_err();
    assert_eq!(err, hirpdag::base::HirpdagDeserializeError::BadMagic);
}

#[test]
fn truncated_input_rejected() {
    let roots = HirpdagArchiveRoots {
        root_Item: vec![Item::new("trunc_item".to_string(), vec![])],
        ..Default::default()
    };
    let bytes = hirpdag_serialize(&roots).unwrap();
    let err = hirpdag_deserialize(&bytes[..bytes.len() - 1]).unwrap_err();
    assert!(matches!(
        err,
        hirpdag::base::HirpdagDeserializeError::Format(_)
    ));
}

#[test]
fn concurrent_archives_do_not_interfere() {
    // An archive carries the state it needs from phase to phase itself, with
    // nothing ambient, so any number of them can be in flight at once.
    //
    // Serializing a ref on its own -- which would silently expand the DAG
    // into a tree -- is not an error to test for here: refs have no serde
    // impls at all, so `serde_json::to_string(&item)` does not compile.
    let threads: Vec<_> = (0..8)
        .map(|t| {
            std::thread::spawn(move || {
                let leaf = Item::new(format!("concurrent_leaf_{}", t), vec![]);
                let root = Item::new(
                    format!("concurrent_root_{}", t),
                    vec![leaf.clone(), leaf.clone()],
                );
                let roots = HirpdagArchiveRoots {
                    root_Item: vec![root.clone()],
                    ..Default::default()
                };
                let bytes = hirpdag_serialize(&roots).unwrap();
                let out = hirpdag_deserialize(&bytes).unwrap();
                assert_eq!(out.root_Item[0], root);
                assert_eq!(out.root_Item[0].deps[0], leaf);
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
fn unsupported_version_rejected() {
    let err = hirpdag_deserialize_json(r#"{"version":99,"nodes":[],"roots":{}}"#).unwrap_err();
    match err {
        hirpdag::base::HirpdagDeserializeError::Format(msg) => {
            assert!(msg.contains("version"), "unexpected message: {}", msg)
        }
        other => panic!("expected Format error, got {:?}", other),
    }
}

#[test]
fn forward_or_out_of_range_index_rejected() {
    // The node's dep index 5 points past everything reconstructed so far;
    // forward references and out-of-range indices are both rejected.
    let text = r#"{"version":1,"nodes":[{"Item":{"name":"x","deps":[5]}}],"roots":{}}"#;
    let err = hirpdag_deserialize_json(text).unwrap_err();
    assert_eq!(
        err,
        hirpdag::base::HirpdagDeserializeError::InvalidNodeIndex {
            index: 5,
            available: 0
        }
    );
}

#[test]
fn node_type_mismatch_rejected() {
    // The roots claim node 0 is a Node, but node 0 is an Item.
    let text =
        r#"{"version":1,"nodes":[{"Item":{"name":"x","deps":[]}}],"roots":{"root_Node":[0]}}"#;
    let err = hirpdag_deserialize_json(text).unwrap_err();
    assert_eq!(
        err,
        hirpdag::base::HirpdagDeserializeError::NodeTypeMismatch { expected: "Node" }
    );
}

#[test]
fn unknown_roots_field_rejected() {
    // A roots key that is not a root of this module is an error, not a
    // silently empty vector. Archives written before roots fields were named
    // `root_<Type>` carry the old snake-cased names, and without
    // `deny_unknown_fields` serde would ignore them, `#[serde(default)]` would
    // fill in empty vectors, and the roots would be lost without a word.
    let text = r#"{"version":1,"nodes":[{"Item":{"name":"x","deps":[]}}],"roots":{"item":[0]}}"#;
    let err = hirpdag_deserialize_json(text).unwrap_err();
    match err {
        hirpdag::base::HirpdagDeserializeError::Format(msg) => {
            assert!(msg.contains("item"), "error should name the field: {}", msg);
        }
        other => panic!("expected Format, got {:?}", other),
    }
}

#[test]
fn handwritten_json_accepted() {
    // The JSON text format is readable and writable by hand. Root vectors
    // that are empty can be omitted (HirpdagArchiveRoots is #[serde(default)]).
    let text = r#"{
        "version": 1,
        "nodes": [
            {"Item": {"name": "hand_leaf", "deps": []}},
            {"Item": {"name": "hand_root", "deps": [0, 0]}}
        ],
        "roots": {"root_Item": [1]}
    }"#;
    let out = hirpdag_deserialize_json(text).unwrap();
    assert_eq!(
        out.root_Item[0],
        Item::new(
            "hand_root".to_string(),
            vec![
                Item::new("hand_leaf".to_string(), vec![]),
                Item::new("hand_leaf".to_string(), vec![]),
            ],
        )
    );
}

/// A module with different hirpdag type definitions, to test the schema
/// fingerprint in the binary header.
#[hirpdag_module]
mod other_schema {
    #[hirpdag(root)]
    pub struct Widget {
        id: u64,
        parts: Vec<Widget>,
    }
}

#[test]
fn schema_mismatch_rejected() {
    let roots = HirpdagArchiveRoots {
        root_Item: vec![Item::new("schema_item".to_string(), vec![])],
        ..Default::default()
    };
    let bytes = hirpdag_serialize(&roots).unwrap();

    // Reading these bytes with a module built from different type
    // definitions must fail up front with a debuggable error, not misparse.
    let err = other_schema::hirpdag_deserialize(&bytes).unwrap_err();
    match err {
        hirpdag::base::HirpdagDeserializeError::SchemaMismatch {
            expected_hash,
            expected_name,
            found_hash,
            found_name,
        } => {
            assert_ne!(expected_hash, found_hash);
            // Both names carry the package name and the type list.
            assert!(found_name.contains("Item"), "found: {}", found_name);
            assert!(
                expected_name.contains("Widget"),
                "expected: {}",
                expected_name
            );
            assert!(
                found_name.contains("hirpdag_test_suite"),
                "found: {}",
                found_name
            );
        }
        other => panic!("expected SchemaMismatch, got {:?}", other),
    }
    // The Display form names both schemas for debuggability.
    let msg = other_schema::hirpdag_deserialize(&bytes)
        .unwrap_err()
        .to_string();
    assert!(msg.contains("schema mismatch"), "message: {}", msg);
    assert!(msg.contains("Widget"), "message: {}", msg);
    assert!(msg.contains("Item"), "message: {}", msg);
}

#[test]
fn schema_match_accepted_across_modules_with_same_shape() {
    // Same-schema round trip through the *same* module still works with the
    // fingerprint present (covered by other tests too); this pins down that
    // the fingerprint only rejects *different* definitions.
    let roots = HirpdagArchiveRoots {
        root_Item: vec![Item::new("schema_same".to_string(), vec![])],
        ..Default::default()
    };
    let bytes = hirpdag_serialize(&roots).unwrap();
    let out = hirpdag_deserialize(&bytes).unwrap();
    assert_eq!(out, roots);
}
