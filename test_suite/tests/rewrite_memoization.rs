//! Tests that a memoizing rewriter actually memoizes.
//!
//! Memoization is on by default: a rewriter owns a `HirpdagRewriteCache` and
//! derives `HirpdagMemoize`. Because rewriters recurse through `self`, the cache
//! is consulted for every node, so on a DAG with shared subtrees each unique
//! node's rewrite rule runs exactly once instead of once per root-to-node path.
//! A rewriter opts out by implementing `HirpdagMemoize` to return `None`.

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

/// A memoizing rewriter that leaves the tree unchanged but counts how many times
/// its per-node rule runs. The counter is shared (`Arc`) so the test can read it
/// back. Memoization is on by default via `#[derive(HirpdagMemoize)]`.
#[derive(HirpdagMemoize)]
struct CountingIdentity {
    calls: Arc<AtomicUsize>,
    cache: HirpdagRewriteCache,
}

impl HirpdagRewriter for CountingIdentity {
    fn rewrite_Node(&self, x: &Node) -> Node {
        self.calls.fetch_add(1, Ordering::Relaxed);
        x.default_rewrite(self)
    }
}

/// The same rule, but with memoization disabled by implementing `HirpdagMemoize`
/// to return `None` (so it needs no cache field).
struct NaiveIdentity {
    calls: Arc<AtomicUsize>,
}

impl HirpdagMemoize<HirpdagRewriteCache> for NaiveIdentity {
    fn hirpdag_memoize_cache(&self) -> Option<&HirpdagRewriteCache> {
        None
    }
}

impl HirpdagRewriter for NaiveIdentity {
    fn rewrite_Node(&self, x: &Node) -> Node {
        self.calls.fetch_add(1, Ordering::Relaxed);
        x.default_rewrite(self)
    }
}

#[test]
fn memoization_visits_each_unique_node_once() {
    let n = 25u64;
    let root = fib_dag(n);
    let unique_nodes = (n + 1) as usize;

    // Memoization disabled: the rule runs once per path, which is exponential in
    // n on a Fibonacci DAG.
    let naive_calls = Arc::new(AtomicUsize::new(0));
    let naive = NaiveIdentity {
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

    // Memoization on (the default): because the cache is consulted through the
    // whole traversal, each of the n + 1 unique nodes is rewritten exactly once.
    let memo_calls = Arc::new(AtomicUsize::new(0));
    let memoized = CountingIdentity {
        calls: memo_calls.clone(),
        cache: HirpdagRewriteCache::new(),
    };
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
