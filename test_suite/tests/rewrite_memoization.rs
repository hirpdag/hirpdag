//! Tests that `HirpdagRewriteMemoized` actually memoizes.
//!
//! The memo cache is populated through interior mutability, and the memoizing
//! wrapper is threaded through the whole traversal as the recursion driver, so
//! on a DAG with shared subtrees each unique node's rewrite rule runs exactly
//! once instead of once per root-to-node path. The cache itself
//! (`HirpdagMemoizeCache`) is an ordinary thread-safe value: shareable between
//! threads, and usable for node-keyed computations that have nothing to do with
//! rewriting.

use hirpdag::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[hirpdag_module]
mod memo_dag {
    // A binary node so we can build a Fibonacci DAG: node `k` points at nodes
    // `k-1` and `k-2`, which are shared (hash-consed) rather than duplicated.
    #[hirpdag]
    struct Node {
        pub id: u64,
        pub left: Option<Node>,
        pub right: Option<Node>,
    }
}
use memo_dag::*;

/// Build a Fibonacci-shaped DAG rooted at "node `n`".
///
/// Nodes `0..=n` are distinct (distinct `id`), so the DAG has exactly `n + 1`
/// unique nodes, but a tree walk that does not exploit sharing would visit
/// `O(fib(n))` of them.
fn fib_dag(n: u64) -> Node {
    let mut a = Node::new(0, None, None); // node 0
    let mut b = Node::new(1, None, None); // node 1
    if n == 0 {
        return a;
    }
    for k in 2..=n {
        let next = Node::new(k, Some(b.clone()), Some(a.clone()));
        a = b;
        b = next;
    }
    b
}

/// A rewriter that leaves the tree unchanged but counts how many times its
/// per-node rule runs (via a shared counter, since the rewriter is moved into
/// the memoizing wrapper).
struct CountingIdentity {
    calls: Arc<AtomicUsize>,
}

impl HirpdagRewriter for CountingIdentity {
    fn rewrite_Node<D: HirpdagRewriteDriver>(&self, x: &Node, driver: &D) -> Node {
        self.calls.fetch_add(1, Ordering::Relaxed);
        // Recursion goes through the driver, so under HirpdagRewriteMemoized the
        // children are served from the cache instead of being walked again.
        x.default_rewrite(driver)
    }
}

#[test]
fn memoization_visits_each_unique_node_once() {
    let n = 25u64;
    let root = fib_dag(n);
    let unique_nodes = (n + 1) as usize;

    // Without memoization: the rule runs once per path, which is exponential in
    // n on a Fibonacci DAG.
    let naive_calls = Arc::new(AtomicUsize::new(0));
    let naive = CountingIdentity {
        calls: naive_calls.clone(),
    };
    let naive_out = naive.rewrite(&root);
    assert_eq!(naive_out, root, "identity rewrite must not change the DAG");
    let naive_count = naive_calls.load(Ordering::Relaxed);
    assert!(
        naive_count > unique_nodes * 10,
        "expected the un-memoized walk to blow up on the shared DAG, got {}",
        naive_count
    );

    // With memoization: the wrapper is threaded through the traversal, so each of
    // the n + 1 unique nodes is rewritten exactly once.
    let memo_calls = Arc::new(AtomicUsize::new(0));
    let memoized = HirpdagRewriteMemoized::new(CountingIdentity {
        calls: memo_calls.clone(),
    });
    let memo_out = memoized.rewrite(&root);
    assert_eq!(memo_out, root, "memoized identity rewrite must match");
    assert_eq!(
        memo_calls.load(Ordering::Relaxed),
        unique_nodes,
        "each unique node's rule should run exactly once"
    );

    // Re-running through the same memoizer is a full cache hit at the root: no
    // further rule invocations.
    let again = memoized.rewrite(&root);
    assert_eq!(again, root);
    assert_eq!(
        memo_calls.load(Ordering::Relaxed),
        unique_nodes,
        "a second rewrite of the same input should be served from the cache"
    );
}

/// The caches are interior-mutable state, not part of the rules: clearing them
/// makes the memoizer forget what it has seen and run the rules again.
#[test]
fn clear_caches_forgets_memoized_results() {
    let n = 6u64;
    let root = fib_dag(n);
    let unique_nodes = (n + 1) as usize;

    let memoized = HirpdagRewriteMemoized::new(CountingIdentity {
        calls: Arc::new(AtomicUsize::new(0)),
    });
    let calls = || memoized.rewriter().calls.load(Ordering::Relaxed);

    assert_eq!(memoized.rewrite(&root), root);
    assert_eq!(calls(), unique_nodes);

    memoized.clear_caches();

    assert_eq!(memoized.rewrite(&root), root);
    assert_eq!(
        calls(),
        2 * unique_nodes,
        "after clearing the caches every unique node is rewritten again"
    );
}

/// One memoizer, several threads: the cache is shared, so the threads divide
/// the work rather than each repeating it.
#[test]
fn one_memoizer_shared_across_threads() {
    const THREADS: usize = 4;
    let n = 20u64;
    let root = fib_dag(n);
    let unique_nodes = (n + 1) as usize;

    let memoized = HirpdagRewriteMemoized::new(CountingIdentity {
        calls: Arc::new(AtomicUsize::new(0)),
    });
    let calls = || memoized.rewriter().calls.load(Ordering::Relaxed);

    std::thread::scope(|scope| {
        for _ in 0..THREADS {
            scope.spawn(|| {
                assert_eq!(memoized.rewrite(&root), root);
            });
        }
    });

    // Two threads reaching the same node before either has finished it will
    // both run the rule, so the exact count depends on scheduling. What the
    // shared cache guarantees is the bound: a small multiple of the node count,
    // never the exponential an unshared walk of this DAG would cost.
    let concurrent_calls = calls();
    assert!(
        (unique_nodes..=THREADS * unique_nodes).contains(&concurrent_calls),
        "expected between {} and {} rule calls across {} threads, got {}",
        unique_nodes,
        THREADS * unique_nodes,
        THREADS,
        concurrent_calls
    );

    // Whatever the threads raced over, every node is cached now.
    assert_eq!(memoized.rewrite(&root), root);
    assert_eq!(calls(), concurrent_calls);
}

/// The cache is not rewrite-specific: it memoizes any node-keyed computation.
#[test]
fn memoize_cache_is_usable_outside_rewriting() {
    let n = 24u64;
    let root = fib_dag(n);
    let unique_nodes = (n + 1) as usize;

    // Node -> node: the lowest-id node reachable from this one. Recursion is
    // memoized through the generated cache, so the shared subtrees of the DAG
    // are visited once between them.
    let cache = HirpdagMemoizeCache::new();
    let evaluations = AtomicUsize::new(0);
    fn lowest_reachable(
        cache: &HirpdagMemoizeCache,
        evaluations: &AtomicUsize,
        node: &Node,
    ) -> Node {
        cache.get_or_else(node, || {
            evaluations.fetch_add(1, Ordering::Relaxed);
            [&node.left, &node.right]
                .iter()
                .filter_map(|child| child.as_ref())
                .map(|child| lowest_reachable(cache, evaluations, child))
                .min_by_key(|found| found.id)
                .unwrap_or_else(|| node.clone())
        })
    }
    assert_eq!(lowest_reachable(&cache, &evaluations, &root).id, 0);
    assert_eq!(
        evaluations.load(Ordering::Relaxed),
        unique_nodes,
        "each unique node should be evaluated exactly once"
    );

    // A table of one's own, for a result that is not a node.
    let sizes: HirpdagMemoizeMap<Node, u64> = HirpdagMemoizeMap::new();
    fn paths_to_leaves(sizes: &HirpdagMemoizeMap<Node, u64>, node: &Node) -> u64 {
        sizes.get_or_else(node, || match (&node.left, &node.right) {
            (None, None) => 1,
            (left, right) => [left, right]
                .iter()
                .filter_map(|child| child.as_ref())
                .map(|child| paths_to_leaves(sizes, child))
                .sum(),
        })
    }
    // Node k has one path per path of its two children, so the path counts
    // follow the same recurrence the DAG was built from: 1, 1, 2, 3, 5, ...
    let mut paths = (1u64, 1u64); // (node 0, node 1)
    for _ in 2..=n {
        paths = (paths.1, paths.0 + paths.1);
    }
    assert_eq!(paths_to_leaves(&sizes, &root), paths.1);
    assert_eq!(sizes.len(), unique_nodes);
}

/// A cache can be primed before it is handed to a rewriter: a node with an
/// entry is served from it, and its subtree is never walked.
#[test]
fn with_cache_reuses_a_primed_cache() {
    let n = 8u64;
    let root = fib_dag(n);
    let unique_nodes = (n + 1) as usize;

    // Rewrite node 3 (a shared subtree of the root) to node 0.
    let cache = HirpdagMemoizeCache::new();
    cache.insert(fib_dag(3), Node::new(0, None, None));

    let memoized = HirpdagRewriteMemoized::with_cache(
        CountingIdentity {
            calls: Arc::new(AtomicUsize::new(0)),
        },
        cache,
    );
    let rewritten = memoized.rewrite(&root);

    assert_ne!(rewritten, root, "the primed entry should change the result");
    assert_eq!(
        memoized.rewriter().calls.load(Ordering::Relaxed),
        unique_nodes - 1,
        "every unique node but the primed one should run its rule once"
    );
}
