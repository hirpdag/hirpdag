use hirpdag::*;

#[hirpdag_module]
mod datamodel {
    #[hirpdag]
    struct MessageA {
        pub a: i32,
        pub b: String,
        pub c: Option<MessageA>,
        pub d: i32,
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
        pub e: EnumB,
    }
}

use datamodel::*;

// A rewriter defined outside the hirpdag module: the generated
// HirpdagRewriter trait, HirpdagRewriteCache, and default_rewrite are
// public, so external rewriters only need the fields they touch to be pub.
#[derive(HirpdagMemoize)]
struct MessageAExtendLeaf {
    doot: MessageA,
    cache: HirpdagRewriteCache,
}

impl MessageAExtendLeaf {
    fn new() -> Self {
        let extension = MessageA::new(0, "DOOT".to_string(), None, 7007);
        Self {
            doot: extension,
            cache: HirpdagRewriteCache::new(),
        }
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

// A rewriter that changes nothing: every node falls through to the default
// rewrite. This exercises the "no changes" fast path in default_rewrite, which
// should return the input reference rather than reconstructing/re-hashconsing an
// identical node.
#[derive(HirpdagMemoize)]
struct Identity {
    cache: HirpdagRewriteCache,
}

impl Identity {
    fn new() -> Self {
        Identity {
            cache: HirpdagRewriteCache::new(),
        }
    }
}

impl HirpdagRewriter for Identity {
    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA {
        x.default_rewrite(self)
    }
}

#[test]
fn identity_rewrite_preserves_nodes() {
    let leaf: MessageA = MessageA::new(11, "leaf".to_string(), None, 22);
    let root: MessageA = MessageA::new(33, "root".to_string(), Some(leaf.clone()), 44);

    let t = Identity::new();
    let rewritten = t.rewrite(&root);

    // An identity rewrite must reproduce the same interned node.
    assert_eq!(rewritten, root);
    // The untouched child subtree is preserved as well.
    assert_eq!(rewritten.c, Some(leaf));
}

#[test]
fn partial_rewrite_preserves_untouched_subtree() {
    // Only leaves (c.is_none()) are extended by MessageAExtendLeaf; a node that
    // already has a child is rebuilt only if one of its rewritten fields
    // actually changed.
    let untouched: MessageA = MessageA::new(1, "keep".to_string(), None, 2);
    // A parent whose leaf child *does* get extended, so the parent changes.
    let parent: MessageA = MessageA::new(3, "parent".to_string(), Some(untouched.clone()), 4);

    let t = MessageAExtendLeaf::new();
    let rewritten = t.rewrite(&parent);

    // The parent changed (its leaf child was extended)...
    assert_ne!(rewritten, parent);
    // ...but the rewritten child is the extended version of the original leaf,
    // which is itself unchanged apart from gaining the extension.
    let extended_leaf = t.rewrite(&untouched);
    assert_eq!(rewritten.c, Some(extended_leaf));
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

// A rewriter that counts how many times its node-local transform actually runs
// (i.e. how many cache misses occur). The counter is shared via `Rc` so the
// test can read it after the rewriter owns the cache.
#[derive(HirpdagMemoize)]
struct CountingIdentity {
    calls: std::rc::Rc<std::cell::Cell<usize>>,
    cache: HirpdagRewriteCache,
}

impl HirpdagRewriter for CountingIdentity {
    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA {
        self.calls.set(self.calls.get() + 1);
        x.default_rewrite(self)
    }
}

// The memoization cache must actually be populated and consulted: each distinct
// node is transformed once, and repeated encounters (within a traversal or
// across separate `rewrite` calls) are served from the cache without re-running
// the node-local transform.
#[test]
fn memoization_caches_rewrites() {
    let calls = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let t = CountingIdentity {
        calls: calls.clone(),
        cache: HirpdagRewriteCache::new(),
    };

    // A three-node chain: root -> mid -> leaf.
    let leaf: MessageA = MessageA::new(1, "memo_leaf".to_string(), None, 1);
    let mid: MessageA = MessageA::new(2, "memo_mid".to_string(), Some(leaf.clone()), 2);
    let root: MessageA = MessageA::new(3, "memo_root".to_string(), Some(mid.clone()), 3);

    // First rewrite visits each of the three nodes exactly once.
    let r1 = t.rewrite(&root);
    assert_eq!(
        r1, root,
        "identity rewrite reproduces the same interned node"
    );
    assert_eq!(calls.get(), 3, "first rewrite transforms each node once");

    // Rewriting the same root again is a full cache hit: no extra work.
    let r2 = t.rewrite(&root);
    assert_eq!(r2, root);
    assert_eq!(
        calls.get(),
        3,
        "second rewrite of the same root hits the cache"
    );

    // Rewriting an already-seen subtree is likewise served from the cache.
    let r_mid = t.rewrite(&mid);
    assert_eq!(r_mid, mid);
    assert_eq!(
        calls.get(),
        3,
        "rewriting a cached subtree adds no transforms"
    );
}

// A rewriter that appends a fixed suffix to every MessageA's string, so the
// transform is observable and non-trivial. `Send + Sync` so the memoizer can be
// shared across threads.
#[derive(HirpdagMemoize)]
struct AppendTag {
    tag: String,
    cache: HirpdagRewriteCache,
}

impl HirpdagRewriter for AppendTag {
    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA {
        let recursed = x.default_rewrite(self);
        MessageA::new(
            recursed.a,
            format!("{}{}", recursed.b, self.tag),
            recursed.c.clone(),
            recursed.d,
        )
    }
}

// The cache is thread-safe: a single memoizer, shared by reference, can drive
// rewrites from several threads at once. The cache's per-type `Mutex` maps
// guard concurrent access, and all threads must agree on the result.
#[test]
fn memoization_cache_is_thread_safe() {
    let leaf: MessageA = MessageA::new(7, "ts_leaf".to_string(), None, 7);
    let root: MessageA = MessageA::new(8, "ts_root".to_string(), Some(leaf), 8);

    let t = AppendTag {
        tag: "_x".to_string(),
        cache: HirpdagRewriteCache::new(),
    };
    let expected = t.rewrite(&root);

    std::thread::scope(|scope| {
        let t = &t;
        let root = &root;
        let expected = &expected;
        for _ in 0..8 {
            scope.spawn(move || {
                for _ in 0..1000 {
                    assert_eq!(&t.rewrite(root), expected);
                }
            });
        }
    });
}
