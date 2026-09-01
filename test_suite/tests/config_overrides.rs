// Test the explicit configuration type strings, the alternative to
// `preset = "..."`.
//
// The shared table names itself and nothing else: it is constructed by
// `Default`, so `tableshared_type` stands alone rather than needing a matching
// factory type to be named alongside it.

#[hirpdag::hirpdag_module(
    reference_type = "hirpdag::hirpdag_hashconsing::RefArc<D>",
    reference_weak_type = "hirpdag::hirpdag_hashconsing::RefArcWeak<D>",
    table_type = "hirpdag::hirpdag_hashconsing::TableVecSortedWeak<D, ImplRef<D>, ImplRefWeak<D>>",
    tableshared_type = "hirpdag::hirpdag_hashconsing::TableSharedMutex<D, ImplRef<D>, ImplTable<D>>"
)]
mod explicit {
    #[hirpdag]
    pub struct Node {
        pub value: i64,
        pub child: Option<Node>,
    }
}

#[test]
fn interning() {
    use explicit::Node;

    let a = Node::new(1, None);
    let b = Node::new(1, None);
    assert_eq!(a, b, "equal nodes must be pointer-equal");

    let c = Node::new(2, None);
    assert_ne!(a, c, "distinct nodes must differ");

    let parent1 = Node::new(3, Some(a.clone()));
    let parent2 = Node::new(3, Some(b));
    assert_eq!(parent1, parent2, "shared children must intern the parent");
}
