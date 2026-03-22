use hirpdag::*;
use hirpdag::base::serialize::{HirpdagSerializer, HirpdagDeserializer};
use hirpdag::serialization::binary::BinaryFieldEncoder;
use hirpdag::serialization::json::JsonFieldEncoder;

// --------------------------------------------------------------------------
// Test types: same as base.rs so we can exercise cross-type DAG serialization
// --------------------------------------------------------------------------

#[hirpdag]
struct MessageA {
    a: i32,
    b: String,
    c: Option<MessageA>,
    d: i32,
}

#[hirpdag]
enum EnumB {
    Foo(i32),
    Bar(String),
    Baz(Option<MessageA>),
    Brr(String),
}

#[hirpdag]
pub struct MessageC {
    d: i32,
    e: EnumB,
}

#[hirpdag_end]
pub struct HirpdagEndMarker;

// --------------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------------

fn roundtrip_binary(roots_in: &[&MessageA]) -> Vec<MessageA> {
    // Serialize
    let mut ser: HirpdagSerializer<BinaryFieldEncoder> = HirpdagSerializer::new();
    for r in roots_in {
        ser.add_root(*r);
    }
    let mut buf = Vec::<u8>::new();
    ser.write_binary(&mut buf).expect("write_binary failed");

    // Deserialize
    let deser = HirpdagDeserializer::from_binary(
        &mut buf.as_slice(),
        hirpdag_deser_dispatch_binary,
    ).expect("from_binary failed");

    (0..roots_in.len())
        .map(|i| deser.get_root::<MessageA>(i).expect("missing root"))
        .collect()
}

fn roundtrip_binary_c(roots_in: &[&MessageC]) -> Vec<MessageC> {
    let mut ser: HirpdagSerializer<BinaryFieldEncoder> = HirpdagSerializer::new();
    for r in roots_in {
        ser.add_root(*r);
    }
    let mut buf = Vec::<u8>::new();
    ser.write_binary(&mut buf).expect("write_binary failed");

    let deser = HirpdagDeserializer::from_binary(
        &mut buf.as_slice(),
        hirpdag_deser_dispatch_binary,
    ).expect("from_binary failed");

    (0..roots_in.len())
        .map(|i| deser.get_root::<MessageC>(i).expect("missing root"))
        .collect()
}

fn roundtrip_json(roots_in: &[&MessageA]) -> Vec<MessageA> {
    let mut ser: HirpdagSerializer<JsonFieldEncoder> = HirpdagSerializer::new();
    for r in roots_in {
        ser.add_root(*r);
    }
    let mut buf = Vec::<u8>::new();
    ser.write_json(&mut buf).expect("write_json failed");

    let deser = HirpdagDeserializer::from_json(
        &mut buf.as_slice(),
        hirpdag_deser_dispatch_json,
    ).expect("from_json failed");

    (0..roots_in.len())
        .map(|i| deser.get_root::<MessageA>(i).expect("missing root"))
        .collect()
}

// --------------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------------

/// Simple leaf node roundtrips correctly (binary).
#[test]
fn test_roundtrip_binary_leaf() {
    let leaf = MessageA::new(1, "hello".to_string(), None, 2);
    let results = roundtrip_binary(&[&leaf]);
    assert_eq!(results[0], leaf);
}

/// Node with a child roundtrips correctly (binary).
#[test]
fn test_roundtrip_binary_parent_child() {
    let child = MessageA::new(10, "child".to_string(), None, 20);
    let parent = MessageA::new(30, "parent".to_string(), Some(child.clone()), 40);
    let results = roundtrip_binary(&[&parent]);
    assert_eq!(results[0], parent);
}

/// Multiple independent roots are all recovered in order.
#[test]
fn test_multiroot_binary() {
    let a = MessageA::new(1001, "a".to_string(), None, 1);
    let b = MessageA::new(1002, "b".to_string(), None, 2);
    let c = MessageA::new(1003, "c".to_string(), None, 3);
    let results = roundtrip_binary(&[&a, &b, &c]);
    assert_eq!(results[0], a);
    assert_eq!(results[1], b);
    assert_eq!(results[2], c);
}

/// A shared sub-DAG node is serialized exactly once.
#[test]
fn test_dag_deduplication() {
    let shared = MessageA::new(9999, "shared_unique_dag_dedup".to_string(), None, 0);
    let left  = MessageA::new(1, "left".to_string(), Some(shared.clone()), 0);
    let right = MessageA::new(2, "right".to_string(), Some(shared.clone()), 0);
    let root  = MessageA::new(3, "root".to_string(), Some(left.clone()), 0);

    // left and right both reference `shared`, but root only directly holds `left`
    // So when we serialize root + right we get: shared, left, right, root = 4 records.
    let mut ser: HirpdagSerializer<BinaryFieldEncoder> = HirpdagSerializer::new();
    ser.add_root(&root);
    ser.add_root(&right);

    // shared appears in both subtrees but should only be serialized once
    assert_eq!(ser.ctx.records.len(), 4, "expected exactly 4 unique nodes");

    let mut buf = Vec::<u8>::new();
    ser.write_binary(&mut buf).unwrap();

    let deser = HirpdagDeserializer::from_binary(
        &mut buf.as_slice(),
        hirpdag_deser_dispatch_binary,
    ).unwrap();

    let recovered_root:  MessageA = deser.get_root(0).unwrap();
    let recovered_right: MessageA = deser.get_root(1).unwrap();

    assert_eq!(recovered_root, root);
    assert_eq!(recovered_right, right);

    // After hashconsing, the shared sub-node should be pointer-equal in both subtrees
    assert_eq!(
        recovered_root.c.as_ref().unwrap().c.as_ref().unwrap(),
        recovered_right.c.as_ref().unwrap(),
    );
}

/// DAG with a MessageC node containing an enum field (cross-type).
#[test]
fn test_roundtrip_cross_type_binary() {
    let msg_a = MessageA::new(42, "hello".to_string(), None, 7);
    let c = MessageC::new(99, EnumB::Baz(Some(msg_a.clone())));
    let results = roundtrip_binary_c(&[&c]);
    assert_eq!(results[0], c);
}

/// All enum variants serialize and deserialize correctly.
#[test]
fn test_enum_variants_binary() {
    let c1 = MessageC::new(1, EnumB::Foo(42));
    let c2 = MessageC::new(2, EnumB::Bar("hello".to_string()));
    let c3 = MessageC::new(3, EnumB::Baz(None));
    let c4 = MessageC::new(4, EnumB::Brr("world".to_string()));

    let results = roundtrip_binary_c(&[&c1, &c2, &c3, &c4]);
    assert_eq!(results[0], c1);
    assert_eq!(results[1], c2);
    assert_eq!(results[2], c3);
    assert_eq!(results[3], c4);
}

/// JSON roundtrip for a leaf node.
#[test]
fn test_roundtrip_json_leaf() {
    let leaf = MessageA::new(1, "json_leaf".to_string(), None, 2);
    let results = roundtrip_json(&[&leaf]);
    assert_eq!(results[0], leaf);
}

/// JSON roundtrip for a parent/child pair.
#[test]
fn test_roundtrip_json_parent_child() {
    let child  = MessageA::new(10, "json_child".to_string(), None, 20);
    let parent = MessageA::new(30, "json_parent".to_string(), Some(child.clone()), 40);
    let results = roundtrip_json(&[&parent]);
    assert_eq!(results[0], parent);
}

/// JSON roundtrip with multiple roots.
#[test]
fn test_multiroot_json() {
    let a = MessageA::new(2001, "ja".to_string(), None, 1);
    let b = MessageA::new(2002, "jb".to_string(), None, 2);
    let results = roundtrip_json(&[&a, &b]);
    assert_eq!(results[0], a);
    assert_eq!(results[1], b);
}

/// Verify root_type_tag() returns the correct tag.
#[test]
fn test_root_type_tag() {
    use hirpdag::base::serialize::HirpdagSerNode;
    let node = MessageA::new(777, "type_tag_test".to_string(), None, 0);

    let mut ser: HirpdagSerializer<BinaryFieldEncoder> = HirpdagSerializer::new();
    ser.add_root(&node);
    let mut buf = Vec::<u8>::new();
    ser.write_binary(&mut buf).unwrap();

    let deser = HirpdagDeserializer::from_binary(
        &mut buf.as_slice(),
        hirpdag_deser_dispatch_binary,
    ).unwrap();

    assert_eq!(deser.root_count(), 1);
    assert_eq!(
        deser.root_type_tag(0).unwrap(),
        <MessageA as HirpdagSerNode<BinaryFieldEncoder>>::TYPE_TAG,
    );
}

/// Deep chain: nodes referencing nodes referencing nodes.
#[test]
fn test_deep_chain_binary() {
    let n0 = MessageA::new(0, "n0".to_string(), None, 0);
    let n1 = MessageA::new(1, "n1".to_string(), Some(n0.clone()), 0);
    let n2 = MessageA::new(2, "n2".to_string(), Some(n1.clone()), 0);
    let n3 = MessageA::new(3, "n3".to_string(), Some(n2.clone()), 0);

    let results = roundtrip_binary(&[&n3]);
    assert_eq!(results[0], n3);
}
