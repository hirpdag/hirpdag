// End-to-end tests for the shared-table backends that wrap third-party
// concurrent collections (dashmap, flurry, crossbeam-skiplist, arc-swap,
// evmap). Each is selected through a `#[hirpdag_module(preset = "...")]`
// preset, proving the wrappers work through the full derive pipeline over real
// `HirpdagStorage` nodes — not just the unit tests in hirpdag_hashconsing.
//
// For each backend we check:
//   * hash-consing dedup: two structurally equal nodes are pointer-equal;
//   * structural sharing survives across a small DAG;
//   * concurrent interning from many threads agrees on one pointer per node.
//
// These backends live behind the `third-party-tables` feature, so the whole
// file compiles only when it is enabled (e.g. `cargo test --all-features`).
#![cfg(feature = "third-party-tables")]

use hirpdag::*;

macro_rules! concurrent_backend_test {
    ($module:ident, $preset:literal, $test_name:ident) => {
        #[hirpdag_module(preset = $preset)]
        mod $module {
            #[hirpdag]
            pub struct Node {
                pub value: i64,
                pub child: Option<Node>,
            }
        }

        #[test]
        fn $test_name() {
            use $module::Node;

            // Basic dedup: equal structure interns to the same pointer.
            let a = Node::new(1, None);
            let b = Node::new(1, None);
            assert_eq!(a, b, "{}: equal nodes must be pointer-equal", $preset);

            let c = Node::new(2, None);
            assert_ne!(a, c, "{}: distinct nodes must differ", $preset);

            // Structural sharing through a child reference.
            let p1 = Node::new(3, Some(a.clone()));
            let p2 = Node::new(3, Some(b.clone()));
            assert_eq!(p1, p2, "{}: parents sharing a child are equal", $preset);

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
                        "{}: threads disagree on interned node for key {}",
                        $preset, k
                    );
                }
            }
        }
    };
}

concurrent_backend_test!(dashmap_mod, "arc_dashmap", dashmap_backend);
concurrent_backend_test!(flurry_mod, "arc_flurry", flurry_backend);
concurrent_backend_test!(skipmap_mod, "arc_skipmap", skipmap_backend);
concurrent_backend_test!(arcswap_mod, "arc_arcswap", arcswap_backend);
concurrent_backend_test!(evmap_mod, "arc_evmap", evmap_backend);
