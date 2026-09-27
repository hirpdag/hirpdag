---
status: accepted
---

# Grow the stack on demand in the rewrite drivers, with `stacker`

A rewrite rule recurses through the driver inside its own body:
`x.default_rewrite(driver)` calls `driver.rewrite(&field)` for each field, and a
rule that rebuilds a node calls `driver.rewrite(&x.child)` itself. The rule's
frame stays alive until the whole subtree below it has been rewritten, so every
level of depth holds a rule's frames and the driver's. The memoized driver adds
its cache lookup and a closure per level. About 2,000 levels in a debug build,
or 18,000 in a release build, overflowed a 2 MiB stack and aborted the process
with the memoized driver; the direct driver managed about 2.5 times as many.

The pending work of a rule lives in its own frame, in the middle of a function
the user wrote, and the only way to resume it is to return to that frame. So a
fix that keeps rules as ordinary functions has to leave those frames on a stack;
what it can change is how big that stack is allowed to get.

The generated `HirpdagRewriteDirect` and `HirpdagRewriteMemoized` now wrap each
per-type driver method in `hirpdag::base::hirpdag_ensure_stack`, which calls
`stacker::maybe_grow`: when less than 128 KiB of stack remains, it continues on
a fresh 2 MiB segment allocated from the heap, and returns to the original
stack afterwards. The rule API is unchanged, every existing rule works at any
depth, and the check costs about 3 ns per node visited.

## Considered options

- **Grow the stack on demand (`stacker`, chosen).** No change to rules or
  drivers' behaviour, covers both drivers and any rule shape. Costs a
  dependency and a minimum Rust version (below).
- **Evaluate tall nodes eagerly, children first, in the memoized driver.**
  Every node stores its height, so on meeting an uncached node taller than some
  K the driver could first rewrite and cache its tall descendants bottom-up on
  an explicit stack; each rule call would then find its tall children cached and
  recurse at most K levels. No dependency, but tall subtrees would be rewritten
  even where a rule would have skipped them (extra work, and extra calls visible
  to rules with side effects), the direct driver would need a temporary cache
  that changes its once-per-path semantics, and the result is harder to reason
  about. Rejected in favour of the simpler option.
- **A separate bottom-up driver with its own rule trait**, handing each rule its
  node with the children already rewritten. Truly iterative and dependency-free,
  but a rule could no longer skip a subtree or continue into a node it built, so
  it does not help existing rules. Worth having for folds and analyses; not a
  fix for this.
- **A helper that runs a closure on a thread with a large stack.** Trivial, but
  opt-in, needs the caller to guess the depth, and needs the closure to be
  `Send`. A workaround to document, not a fix.
- **Async rules on a heap-allocated executor.** A boxed future per node visited
  and a different rule API. Rejected.

## Consequences

- A rewrite of any depth runs on any stack: `test_suite/tests/deep_rewrite.rs`
  rewrites a 10,000-node chain on a 64 KiB thread under both drivers, which
  overflowed in both debug and release before.
- `hirpdag` depends on `stacker`, and through it on `psm`, whose build script
  assembles a small amount of platform code with the `cc` crate. That needs an
  assembler for the target at build time, which the toolchain that links Rust
  programs already provides on the supported platforms. On a platform `psm`
  does not support, `maybe_grow` cannot measure the stack and grows on first
  use; it does not fail.
- The minimum supported Rust version rises from 1.80 to 1.88, the version
  `psm` 0.1.32 declares. Pinning an older `psm` instead was rejected as fragile:
  it would hold back every downstream crate that uses `stacker` too.
- Each call into a generated driver method checks the remaining stack: about
  3 ns, roughly 1% of a node visit in `rewrite_chain`; the rewrite benchmarks
  show no change beyond noise.
- The 128 KiB margin has to cover whatever stack a rule uses between two driver
  calls. A rule with very large locals could still overflow; it would need to
  call `hirpdag_ensure_stack` itself, as would a user-written driver.
- Dropping a deep graph recursed too, and is fixed separately (ADR-0012). The
  memoized driver's cache holds nodes, so dropping it can free a deep graph.
