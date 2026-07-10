// End-to-end tests exercising every configuration preset through the full
// derive pipeline over real `HirpdagStorage` nodes — not just the unit tests in
// hirpdag_hashconsing.
//
// `hirpdag_test_configs!` (see tests/support) stamps the `Node` type and the
// `interning` test out once per preset, so each runs as `<preset>::interning`.
// The presets backed by third-party collection crates only appear under the
// `third-party-tables` feature (e.g. `cargo test --all-features`).

#[macro_use]
mod support;

hirpdag_test_configs! {
    #[hirpdag]
    pub struct Node {
        pub value: i64,
        pub child: Option<Node>,
    }

    #[test]
    fn interning() {
        // Basic dedup: equal structure interns to the same pointer.
        let a = Node::new(1, None);
        let b = Node::new(1, None);
        assert_eq!(a, b, "equal nodes must be pointer-equal");

        let c = Node::new(2, None);
        assert_ne!(a, c, "distinct nodes must differ");

        // Structural sharing through a child reference.
        let p1 = Node::new(3, Some(a.clone()));
        let p2 = Node::new(3, Some(b.clone()));
        assert_eq!(p1, p2, "parents sharing a child are equal");

        // Concurrent interning: every thread must observe the same interned
        // pointer for each key.
        let n = 200i64;
        let n_threads = 8;
        let mut handles = Vec::new();
        for _ in 0..n_threads {
            handles.push(std::thread::spawn(move || {
                (0..n).map(|k| Node::new(k, None)).collect::<Vec<Node>>()
            }));
        }
        let results: Vec<Vec<Node>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let first = &results[0];
        for other in &results[1..] {
            for k in 0..n as usize {
                assert_eq!(
                    first[k], other[k],
                    "threads disagree on interned node for key {}",
                    k
                );
            }
        }
    }
}
