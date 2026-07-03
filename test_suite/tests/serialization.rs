// Tests for DAG-aware serialization/deserialization.
//
// See docs/design/serialization.md and docs/adr/0001-serde-dag-aware-serialization.md.

use hirpdag::*;
use std::convert::{TryFrom, TryInto};

#[hirpdag]
struct Item {
    name: String,
    deps: Vec<Item>,
}

#[hirpdag]
enum Kind {
    Num(u32),
    Sum(Vec<Node>),
}

#[hirpdag]
struct Node {
    kind: Kind,
}

#[hirpdag]
struct Pair {
    left: Item,
    right: Item,
}

#[hirpdag_end]
pub struct HirpdagEndMarker;

#[test]
fn binary_round_trip_pointer_equal() {
    let a = Item::new("rt_leaf_a".to_string(), vec![]);
    let b = Item::new("rt_leaf_b".to_string(), vec![]);
    let parent = Item::new(
        "rt_parent".to_string(),
        vec![a.clone(), b.clone(), a.clone()],
    );

    let bytes = hirpdag_serialize(&[parent.clone().into()]).unwrap();
    let roots = hirpdag_deserialize(&bytes).unwrap();

    assert_eq!(roots.len(), 1);
    let parent2: Item = roots[0].clone().try_into().unwrap();
    // Hirpdag equality is pointer equality: the deserialized root re-interned
    // to the exact same node.
    assert_eq!(parent2, parent);
    assert_eq!(parent2.deps[0], parent2.deps[2]);
}

#[test]
fn json_round_trip_pointer_equal() {
    let a = Item::new("json_leaf".to_string(), vec![]);
    let parent = Item::new("json_parent".to_string(), vec![a.clone(), a.clone()]);

    let text = hirpdag_serialize_json(&[parent.clone().into()]).unwrap();
    let roots = hirpdag_deserialize_json(&text).unwrap();

    assert_eq!(roots.len(), 1);
    let parent2: Item = roots[0].clone().try_into().unwrap();
    assert_eq!(parent2, parent);
}

#[test]
fn enum_payload_round_trip() {
    let n1 = Node::new(Kind::Num(4001));
    let n2 = Node::new(Kind::Num(4002));
    let sum = Node::new(Kind::Sum(vec![n1.clone(), n2.clone(), n1.clone()]));

    let bytes = hirpdag_serialize(&[sum.clone().into()]).unwrap();
    let roots = hirpdag_deserialize(&bytes).unwrap();
    let sum2: Node = roots[0].clone().try_into().unwrap();
    assert_eq!(sum2, sum);
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

    let text = hirpdag_serialize_json(&[curr.clone().into()]).unwrap();
    let value: hirpdag::serde_json::Value = hirpdag::serde_json::from_str(&text).unwrap();
    let node_count = value["nodes"].as_array().unwrap().len();
    // 2 leaves + `depth` sums. Tree expansion would be fib(depth) ~ 10946 nodes.
    assert_eq!(node_count, depth + 2);

    let roots = hirpdag_deserialize_json(&text).unwrap();
    let curr2: Node = roots[0].clone().try_into().unwrap();
    assert_eq!(curr2, curr);
}

#[test]
fn multiple_roots_share_nodes() {
    let shared = Item::new("multi_shared".to_string(), vec![]);
    let r1 = Item::new("multi_r1".to_string(), vec![shared.clone()]);
    let r2 = Item::new("multi_r2".to_string(), vec![shared.clone()]);

    let text = hirpdag_serialize_json(&[r1.clone().into(), r2.clone().into()]).unwrap();
    let value: hirpdag::serde_json::Value = hirpdag::serde_json::from_str(&text).unwrap();
    // The shared child is written once: 3 nodes, not 4.
    assert_eq!(value["nodes"].as_array().unwrap().len(), 3);

    let roots = hirpdag_deserialize_json(&text).unwrap();
    assert_eq!(roots.len(), 2);
    let d1: Item = roots[0].clone().try_into().unwrap();
    let d2: Item = roots[1].clone().try_into().unwrap();
    assert_eq!(d1, r1);
    assert_eq!(d2, r2);
    // The shared subgraph is the same node in both deserialized roots.
    assert_eq!(d1.deps[0], d2.deps[0]);
}

#[test]
fn mixed_type_roots() {
    let i = Item::new("mixed_item".to_string(), vec![]);
    let p = Pair::new(i.clone(), i.clone());

    let bytes = hirpdag_serialize(&[
        HirpdagAnyRef::from(p.clone()),
        HirpdagAnyRef::from(i.clone()),
    ])
    .unwrap();
    let roots = hirpdag_deserialize(&bytes).unwrap();

    assert_eq!(roots.len(), 2);
    let p2: Pair = roots[0].clone().try_into().unwrap();
    let i2: Item = roots[1].clone().try_into().unwrap();
    assert_eq!(p2, p);
    assert_eq!(i2, i);
    // The Item root and the Pair's children are the same interned node.
    assert_eq!(p2.left, i2);
}

#[test]
fn deserialize_twice_merges() {
    let leaf = Item::new("merge_leaf".to_string(), vec![]);
    let root = Item::new("merge_root".to_string(), vec![leaf]);
    let bytes = hirpdag_serialize(&[root.clone().into()]).unwrap();

    let r1: Item = hirpdag_deserialize(&bytes).unwrap()[0]
        .clone()
        .try_into()
        .unwrap();
    let r2: Item = hirpdag_deserialize(&bytes).unwrap()[0]
        .clone()
        .try_into()
        .unwrap();
    // Re-interning through the hashcons table merges with existing nodes.
    assert_eq!(r1, r2);
    assert_eq!(r1, root);
}

#[test]
fn root_try_from_wrong_type() {
    let item = Item::new("tryfrom_item".to_string(), vec![]);
    let bytes = hirpdag_serialize(&[item.into()]).unwrap();
    let roots = hirpdag_deserialize(&bytes).unwrap();
    let result = Pair::try_from(roots[0].clone());
    assert!(matches!(
        result,
        Err(hirpdag::base::HirpdagSerializeError::TypeMismatch { expected: "Pair" })
    ));
}

#[test]
fn bad_magic_rejected() {
    let err = hirpdag_deserialize(b"XXXX not a hirpdag archive").unwrap_err();
    assert_eq!(err, hirpdag::base::HirpdagSerializeError::BadMagic);
}

#[test]
fn truncated_input_rejected() {
    let item = Item::new("trunc_item".to_string(), vec![]);
    let bytes = hirpdag_serialize(&[item.into()]).unwrap();
    let err = hirpdag_deserialize(&bytes[..bytes.len() - 1]).unwrap_err();
    assert!(matches!(
        err,
        hirpdag::base::HirpdagSerializeError::Format(_)
    ));
}

#[test]
fn ref_serialize_outside_session_fails() {
    // Serializing a hirpdag ref without going through hirpdag_serialize
    // would silently expand the DAG into a tree; it must be an error instead.
    let item = Item::new("no_session_item".to_string(), vec![]);
    let result = hirpdag::serde_json::to_string(&item);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("session"));
}

#[test]
fn unsupported_version_rejected() {
    let err = hirpdag_deserialize_json(r#"{"version":99,"nodes":[],"roots":[]}"#).unwrap_err();
    match err {
        hirpdag::base::HirpdagSerializeError::Format(msg) => {
            assert!(msg.contains("version"), "unexpected message: {}", msg)
        }
        other => panic!("expected Format error, got {:?}", other),
    }
}

#[test]
fn forward_or_out_of_range_index_rejected() {
    // The node's dep index 5 points past everything reconstructed so far;
    // forward references and out-of-range indices are both rejected.
    let text = r#"{"version":1,"nodes":[{"Item":{"name":"x","deps":[5]}}],"roots":[]}"#;
    let err = hirpdag_deserialize_json(text).unwrap_err();
    match err {
        hirpdag::base::HirpdagSerializeError::Format(msg) => {
            assert!(msg.contains("invalid"), "unexpected message: {}", msg)
        }
        other => panic!("expected Format error, got {:?}", other),
    }
}

#[test]
fn node_type_mismatch_rejected() {
    // The root claims node 0 is a Pair, but node 0 is an Item.
    let text = r#"{"version":1,"nodes":[{"Item":{"name":"x","deps":[]}}],"roots":[{"Pair":0}]}"#;
    let err = hirpdag_deserialize_json(text).unwrap_err();
    match err {
        hirpdag::base::HirpdagSerializeError::Format(msg) => {
            assert!(msg.contains("type mismatch"), "unexpected message: {}", msg)
        }
        other => panic!("expected Format error, got {:?}", other),
    }
}

#[test]
fn handwritten_json_accepted() {
    // The JSON text format is readable and writable by hand.
    let text = r#"{
        "version": 1,
        "nodes": [
            {"Item": {"name": "hand_leaf", "deps": []}},
            {"Item": {"name": "hand_root", "deps": [0, 0]}}
        ],
        "roots": [{"Item": 1}]
    }"#;
    let roots = hirpdag_deserialize_json(text).unwrap();
    let root: Item = roots[0].clone().try_into().unwrap();
    assert_eq!(
        root,
        Item::new(
            "hand_root".to_string(),
            vec![
                Item::new("hand_leaf".to_string(), vec![]),
                Item::new("hand_leaf".to_string(), vec![]),
            ],
        )
    );
}
