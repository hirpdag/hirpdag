---
status: accepted
---

# Separate rewrite rules from the traversal that drives them

`HirpdagRewriteMemoized<Rewriter>` did not memoize anything. It held a
`HashMap` per type, but the maps were only ever read: rewriting takes `&self`,
so nothing could insert into them. Even with interior mutability the caches
would have been almost useless, because the memoizer never saw most of the
traversal. A rule was written as

```rust
fn rewrite_Expr(&self, x: &Expr) -> Expr {
    ...
    x.default_rewrite(self)   // self is the *inner* rewriter
}
```

and `default_rewrite` recursed through the value it was handed, which is the
user's rewriter, not the wrapper around it. The memoizer therefore got one
call — the root — and every node below it was rewritten by the bare rewriter
once per path reaching it. On a DAG with two-parent sharing that is
exponential in the depth: the memoization the library advertises as its
headline rewriting feature was absent, and the escape from re-traversal that
hash-consing is supposed to buy was never taken.

We split the two things the old `HirpdagRewriter` conflated:

- **`HirpdagRewriter`** — the *rules*. One `rewrite_Foo` method per type,
  now taking the recursion driver as a parameter:
  `fn rewrite_Foo<D: HirpdagRewriteDriver>(&self, x: &Foo, driver: &D) -> Foo`.
- **`HirpdagRewriteDriver`** — the *traversal*. One `rewrite_Foo(&self, x: &Foo)`
  method per type. `default_rewrite` and `HirpdagRewritable` recurse through a
  driver, never through a rewriter.

Two drivers are generated per module: `HirpdagRewriteDirect<'_, R>` (apply the
rules on every path — what `rewriter.rewrite(&x)` uses) and
`HirpdagRewriteMemoized<R>` (ask a cache first, run the rule only on a miss).
Because a rule recurses through the driver it is handed, and the memoizer hands
*itself* down, every node in the traversal now passes through the cache: one
rule invocation per unique node, however many paths reach it.
`test_suite/tests/rewrite_memoization.rs` pins this on a Fibonacci DAG, where
the un-memoized walk makes ~10<sup>4</sup> times more calls than there are
nodes.

The cache is a value in its own right, not a private part of the memoizer.
`#[hirpdag_module]` generates a `HirpdagMemoizeCache` holding one
`hirpdag::base::HirpdagMemoizeMap<Foo, Foo>` per data type, and the memoizer's
driver methods are one line each:

```rust
fn rewrite_Foo(&self, x: &Foo) -> Foo {
    self.memoize_cache.get_or_else(x, || self.rewriter.rewrite_Foo(x, self))
}
```

The lookup-or-compute logic lives once in `HirpdagMemoizeMap`, in the hirpdag
crate, rather than being stamped out per type by the macro. A node-keyed cache
is useful well beyond rewriting — hash-consing makes a node an `O(1)` key for
any derived result — so `HirpdagMemoizeCache` is public and standalone: build
one, use `cache.get_or_else(&node, || ..)` for an analysis of your own, prime
one with known results and hand it to a rewriter via
`HirpdagRewriteMemoized::with_cache`, or use `HirpdagMemoizeMap<Node, YourType>`
directly for results that are not nodes. This is also the side-table the
immutability rules point users at (`book/src/ch04-00-techniques.md`): the state
a node cannot hold, held beside it.

`HirpdagMemoizeMap` is thread-safe the way the hash-consing tables are:
`N_SHARDS` independently locked shards chosen by the key's hash. It never holds
a lock while computing a value, so a rule is free to recurse into the same
cache for its children, and one rewriter can be driven from several threads at
once with the work shared rather than repeated.

## Considered Options

- **Keep the single trait; give the memoizer's caches interior mutability.**
  The minimal change, and the one the type signatures invite. It fixes only
  the root call: `x.default_rewrite(self)` inside a rule still hands the
  recursion to the inner rewriter, so the cache is consulted once per
  top-level `rewrite()` and never during the walk it kicks off. Sharing is
  still re-traversed; the exponential stays.
- **Reach the memoizer from the rewriter through ambient state** — a
  thread-local "current driver" stack, or recovering the wrapper's address
  from the inner rewriter's (`container_of`-style pointer arithmetic).
  Both keep the old rule signature, and both are worse than the problem:
  the thread-local is a hidden side channel that breaks re-entrancy and any
  use of a rewriter across threads, the pointer arithmetic needs `unsafe`
  in a crate that is `#![forbid(unsafe_code)]`.
- **Memoize below the rules instead of around them** — cache inside
  `default_rewrite` rather than at the rule boundary. Needs no API change,
  but caches the wrong thing: a rule that returns without calling
  `default_rewrite` (the interesting case — that is what a rewrite *is*) is
  never cached, and a rule that transforms the result of `default_rewrite`
  has its own work recomputed on every path.
- **Bottom-up traversal driven by the memoizer**, rewriting each unique node
  once in dependency order and never letting the rules recurse. The rules
  would have to be rewritten around it: they could no longer choose whether,
  or in what order, to descend (`Substitute` returning a replacement subtree
  without walking into it, a rule that inspects children before deciding),
  which is most of what rewriting is for.
- **A `RefCell` cache private to the memoizer** — the first cut of this
  change. Simpler, and enough to make the traversal memoize, but it makes the
  memoizer `!Sync` for no reason the problem demands, and it buries a
  generally useful data structure (a thread-safe node-keyed table) inside the
  rewriting code, where nothing else can reach it.
- **A `&dyn HirpdagRewriteDriver` parameter** instead of a generic one.
  Shorter signatures in user code (`driver: &dyn HirpdagRewriteDriver`), at
  one virtual call per child edge — on the hot path of every traversal, in a
  library whose reason to exist is the constant factor. The generic driver
  monomorphizes to the same direct calls as before.

## Consequences

- Breaking change to the rewriter interface: every `rewrite_Foo` gains a
  `<D: HirpdagRewriteDriver>` parameter, and recursion inside a rule changes
  from `x.default_rewrite(self)` / `self.rewrite(&child)` to
  `x.default_rewrite(driver)` / `driver.rewrite(&child)`. Rules that never
  recurse just ignore the parameter. Nothing changes at call sites:
  `HirpdagRewriteMemoized::new(rules).rewrite(&x)` and `rules.rewrite(&x)`
  read as before.
- `HirpdagRewriteMemoized` is no longer a `HirpdagRewriter`, so memoizers
  cannot be nested (they never usefully were). It is a driver, and `rewrite`
  on it now comes from `HirpdagRewriteDriver` — code that imported the
  rewriter trait to call `.rewrite()` on a memoizer imports the driver trait
  instead.
- One memoizer can be shared by several threads (`&memoized` is `Sync` when
  the rules and the reference type are), and they share what they have
  computed. Threads that race to the same node before either finishes it can
  each run the rule; the first result to land is the one kept, so callers still
  agree on one value. Node types built on `RefRc` are single-threaded as
  before — the auto traits decide, nothing new is imposed.
- Cached results assume a rule is a pure function of its node — the usual
  case, since a rewriter's state is fixed when it is constructed. A memoizer
  also keeps every node it has seen alive; `clear_caches()` (or
  `HirpdagMemoizeCache::clear()`) releases them without rebuilding the
  rewriter.
- The generated per-module surface grows by `HirpdagMemoizeCache` and its
  `hirpdag::base::HirpdagMemoize<Foo>` impls, re-exported from the module so a
  glob import is enough to call them.
- Benchmarks that rewrite shared graphs (`primes`, `expr_substitution`,
  `sparse_rewrite`) now do the work their memoized rewriters always claimed
  to do, so their results are not comparable across this change.
