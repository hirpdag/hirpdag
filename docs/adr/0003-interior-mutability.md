---
status: accepted
---

# Interior mutability in hirpdag

Hirpdag's value comes from node data being *frozen* once interned. A node's
data is its identity: the interning table keys on `Hash`/`Eq` of the data, two
`HirpdagRef`s are equal iff they are the same allocation (pointer equality),
ordering and hashing use the `creation_id` assigned at intern time, the
`HirpdagMeta` (count/height/flags) is folded bottom-up once at intern time, and
serialization identity is the node graph. Mutating any of that after interning
is the classic "mutate a key that is already inside a hash set" bug: it silently
corrupts every table, cache, and cross-thread share that assumed the value was
stable.

So the question "how should hirpdag work with interior mutability?" is really:
*where does mutable state belong in a system whose whole point is that data does
not change?* This ADR records the rule we follow, and fixes the one place the
codebase already needed interior mutability but lacked it.

Note that hirpdag already relies on interior mutability heavily — just never in
node *data*. The interning tables are sharded `Mutex`es
(`table/shared_sharded.rs`), the creation counter is an `AtomicU64`
(`base/reference.rs`), the reference-count backends use atomics and a slot-pool
`Mutex` (`reference/tlc.rs`, `reference/sepcount.rs`), and the serde sessions are
`thread_local! RefCell`s (generated). This is all *machinery*; it does not touch
the immutable data.

## The rule: three levels

**Level A — identity-defining node fields: interior mutability is forbidden.**
A `Cell`/`RefCell`/`Mutex`/atomic field that participates in `Hash`/`Eq`/`Ord`
breaks hash-consing, the cached `HirpdagMeta`, the `creation_id` order,
serialization, and `Send + Sync` sharing across the default `RefArc`. The macro
already funnels all construction through a single hash-cons call and exposes only
`Deref` (never `DerefMut`), so a node's data is structurally read-only after
interning; a user must not smuggle mutability back in through a field's type.
"Modifying" a node means building a new one (`to_builder().field(v).build()`),
and that stays the only way.

**Level B — identity-independent derived/cached data: this is the sanctioned
use of interior mutability.** Data that is a *pure function of the frozen node*
and is never observed through `Hash`/`Eq`/`Ord`/serialization can be computed
lazily and cached. Two shapes, each with its own concurrency posture (deciding
per mechanism rather than globally):

- **B1 — side-table memoization keyed by node** (e.g. the rewrite cache). A
  single rewrite traversal runs on one thread and each memoizer is owned by its
  caller, so a **`RefCell<HashMap>`** is the right, zero-lock choice. A memoizer
  intended to be shared across threads would instead need a `Mutex`/sharded map.
- **B2 — a per-node lazily-computed slot** (e.g. a cached deep-hash, an
  evaluation/type result, or the `hirpdag_compute_meta()` caching in `TODO.md`).
  It would live in `HirpdagStorage`, excluded from every identity trait and from
  serialization. Because nodes are shared across threads via the default
  `RefArc` (`Send + Sync`), such a slot **must** be thread-safe — a
  **`OnceLock<T>`** (write-once, lock-free reads) for a single derived value, or
  an atomic for a small scalar. Note that hirpdag currently computes `HirpdagMeta`
  *eagerly* at intern time, so it needs no slot; a lazy per-node slot only earns
  its place (an extra word on every allocation) for a value that is expensive and
  not always needed. We describe the pattern here but do not add such a slot
  speculatively.

**Level C — machinery: unchanged.** The tables, counters, refcounts, and serde
sessions above are interior mutability done right and are out of scope.

## Worked example: make the rewrite memo cache real

`HirpdagRewriteMemoized` is the concrete B1 case, and it was broken. Each
generated `cache_<Type>: HashMap<Ref, Ref>` was read via `&self` but **never
inserted into anywhere** — the `HirpdagRewriter` methods take `&self`, so with a
plain `HashMap` there was no way to populate it. Every lookup missed and
recomputed; the cache was dead code.

Fixing it took two changes, because interior mutability alone was necessary but
not sufficient:

1. **Interior mutability so the cache can be written.** `cache_<Type>` is now a
   `RefCell<HashMap<Ref, Ref>>`; on a miss the wrapper computes the result,
   inserts it, and returns it.

2. **Thread the memoizer through the traversal so shared subtrees actually
   hit.** Previously the wrapper was threaded only at the outermost node: its
   `rewrite_<T>` delegated to the *inner* rewriter, whose rule recursed via
   `x.default_rewrite(self_inner)`, so every child bypassed the cache. We split
   the *recursion driver* from `self` (the object holding the per-type rule): the
   generated per-type methods now take an explicit driver,

   ```rust
   fn rewrite_Expr(&self, rec: &impl HirpdagRewriter, x: &Expr) -> Expr {
       // ... custom cases ...
       x.default_rewrite(rec) // recurse through the driver, not self
   }
   ```

   At the traversal entry the driver is the rewriter itself, so a plain rewriter
   is unchanged. For `HirpdagRewriteMemoized` the driver is the memoizing
   wrapper, so on a miss it runs the inner rule but passes *itself* as the driver
   — every child re-enters the wrapper's cache. A rewrite is a pure function of
   its input node, so memoizing input → output is always correct; on a DAG with
   shared subtrees this collapses the work from `O(paths)` to `O(unique nodes)`.
   `tests/memoization.rs` proves it: an identity rewrite over a Fibonacci DAG runs
   its per-node rule exactly `n + 1` times (once per unique node) instead of
   exponentially.

The one user-visible cost is the extra `rec: &impl HirpdagRewriter` parameter on
per-type rewrite methods, and recursing through `rec` instead of `self`. This is
a deliberate, small change to the rewriting surface (README and all example
rewriters updated) that turns a dead feature into a working one.

## Considered Options

- **Only add interior mutability to the cache (a `RefCell`), without threading
  the driver.** Rejected as a non-fix: it would make the cache non-dead but it
  would still only ever cache the outermost node of each top-level call, never
  the shared subtrees within a traversal — which is the entire reason a
  hash-consed DAG wants memoized rewriting. It buys only cross-call caching of
  identical roots.
- **A thread-local / global memo table consulted transparently by
  `rewrite<U>`.** Would avoid the signature change, but a table keyed only by
  node identity is wrong when more than one rewriter is in play (different
  rewriters give different outputs), and scoping it per traversal reintroduces
  the same "who is the current driver" plumbing through hidden global state
  instead of an explicit parameter. Rejected as less clear and more error-prone.
- **Allow interior-mutable fields on nodes for "cached" data, excluded from
  `Hash`/`Eq` by attribute.** Rejected: it puts identity-independent state
  *inside* the identity-defining struct, one typo away from corrupting the
  interning table, and it breaks `Send + Sync`/serialization unless every such
  field is independently proven safe. Level B keeps cached state either in a side
  table (B1) or in an identity-excluded storage slot (B2), never in the node's
  own data.
- **Reject interior-mutable field types in the macro.** A good defensive
  follow-up (a warning when a `#[hirpdag]` struct field is a known
  interior-mutable type), but orthogonal to this change and not required to state
  the rule. Left for later.
