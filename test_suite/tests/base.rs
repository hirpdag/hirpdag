use hirpdag::*;

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

/// Test that the creation-order Ord is semantically correct:
/// if A refers to B, then B < A.
#[test]
fn test_ord_creation_order() {
    // Use unique field values to avoid hash-consing returning pre-existing nodes
    // from other tests (which would have earlier creation IDs).
    let leaf = MessageA::new(88881, "ord_leaf_unique".to_string(), None, 88881);
    let parent = MessageA::new(
        88881,
        "ord_parent_unique".to_string(),
        Some(leaf.clone()),
        88881,
    );

    // parent refers to leaf, so leaf was interned first → leaf < parent
    assert!(
        leaf < parent,
        "leaf should be less than parent (leaf was created first)"
    );
    assert!(parent > leaf);

    // A node must compare equal to itself
    assert_eq!(leaf.cmp(&leaf), std::cmp::Ordering::Equal);
    assert_eq!(parent.cmp(&parent), std::cmp::Ordering::Equal);
}

/// Test that hirpdag_cmp_deep performs a structural (deep) comparison.
#[test]
fn test_hirpdag_cmp_deep() {
    let a = MessageA::new(77771, "deep_a_unique".to_string(), None, 77771);
    let b = MessageA::new(77771, "deep_b_unique".to_string(), None, 77771);

    // Structurally "deep_a_unique" < "deep_b_unique"
    assert_eq!(
        a.hirpdag_cmp_deep(&b),
        std::cmp::Ordering::Less,
        "deep cmp should compare structurally"
    );
    assert_eq!(
        a.hirpdag_cmp_deep(&a),
        std::cmp::Ordering::Equal,
        "deep cmp of same node should be Equal"
    );
}

fn print_hash<T: std::hash::Hash>(t: &T) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    println!("{}", s.finish());
}

#[test]
fn foobar1() {
    println!("========");
    let a: MessageA = MessageA::new(32, "sup".to_string(), None, 1);
    let b: MessageA = MessageA::new(32, "dog".to_string(), Some(a.clone()), 1);
    let c: MessageA = MessageA::new(32, "sup".to_string(), None, 1);
    let d: MessageA = MessageA::new(32, "dog".to_string(), Some(c.clone()), 1);

    let h: MessageC = MessageC::new(32, EnumB::Brr("aaa".to_string()));
    println!("{:?}", h.e);
    let h2: MessageC = MessageC::new(32, EnumB::Brr("aaa".to_string()));
    println!("{:?}", h2.e);

    print_hash(&a);
    print_hash(&b);
    print_hash(&c);
    print_hash(&d);

    print_hash(&h);
    print_hash(&h2);

    assert_eq!(a, c);
    assert_eq!(b, d);
}

#[test]
fn builder_new() {
    let a: MessageA = MessageA::builder()
        .a(32)
        .b("sup".to_string())
        .c(None)
        .d(1)
        .build();
    let a2: MessageA = MessageA::new(32, "sup".to_string(), None, 1);
    assert_eq!(a, a2);
}

#[test]
fn builder_to_builder() {
    let a: MessageA = MessageA::new(32, "sup".to_string(), None, 1);
    // Use to_builder to modify a single field
    let b: MessageA = a.to_builder().b("dog".to_string()).build();
    let b2: MessageA = MessageA::new(32, "dog".to_string(), None, 1);
    assert_eq!(b, b2);
    // Original is unchanged
    assert_eq!(a, MessageA::new(32, "sup".to_string(), None, 1));
}

#[test]
fn builder_from_existing() {
    let a: MessageA = MessageA::new(10, "hello".to_string(), None, 5);
    let b: MessageA = MessageA::new(10, "hello".to_string(), Some(a.clone()), 5);
    // Extend with child using builder
    let c: MessageA = a.to_builder().c(Some(a.clone())).build();
    assert_eq!(b, c);
}

struct MessageAExtendLeaf {
    doot: MessageA,
}

impl MessageAExtendLeaf {
    fn new() -> HirpdagRewriteMemoized<Self> {
        let extension = MessageA::new(0, "DOOT".to_string(), None, 7007);
        HirpdagRewriteMemoized::new(Self { doot: extension })
    }
}

impl HirpdagRewriter for MessageAExtendLeaf {
    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA {
        if x.c.is_none() {
            return MessageA::new(x.a, x.b.clone(), Some(self.doot.clone()), x.d);
        }

        // In the case where we don't want to make changes to extend the leaf,
        // we want to apply the default rewrite which will apply the rewrite
        // transitively to all applicable members.
        x.default_rewrite(self)
    }
}

#[test]
fn foobar3() {
    println!("========");
    let a: MessageA = MessageA::new(32, "sup".to_string(), None, 0);
    let b: MessageA = MessageA::new(32, "dog".to_string(), Some(a.clone()), 0);

    let t = MessageAExtendLeaf::new();
    eprintln!("a\n{:?}", a);
    let ta = t.rewrite(&a);
    eprintln!("t(a)\n{:?}", ta);

    let t = MessageAExtendLeaf::new();
    eprintln!("b\n{:?}", b);
    let tb = t.rewrite(&b);
    eprintln!("t(b)\n{:?}", tb);
}

#[test]
fn foobar4() {
    println!("========");
    let a: MessageA = MessageA::new(32, "sup".to_string(), None, 0);
    let b: MessageA = MessageA::new(32, "dog".to_string(), Some(a.clone()), 0);

    let c: MessageC = MessageC::new(4, EnumB::Baz(Some(b)));
    eprintln!("c\n{:?}", c);

    let t = MessageAExtendLeaf::new();
    let tc = t.rewrite(&c);
    eprintln!("t(c)\n{:?}", tc);
}
