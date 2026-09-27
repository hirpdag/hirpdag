---
status: accepted
---

# Free deep graphs through a per-thread queue past a fixed nesting depth

Freeing a node drops its data, which drops the child references in its fields,
which free each child whose last reference that was, and so on down the graph.
Rust's drop glue does this by recursion: several stack frames per level of
depth. Dropping a chain about 8,000 nodes deep in a debug build, or 64,000 in a
release build, overflowed a 2 MiB stack and aborted the process. Nothing about
building such a chain warned of it: construction does not recurse.

Every parent-to-child edge in a hirpdag graph is a `HirpdagRef` field, so
`HirpdagRef` is the one place a fix covers every way a graph is freed: a user
dropping a node, a memoization cache being cleared or dropped, an interning
table being reset or dropped, a thread exiting.

`HirpdagRef` now holds its reference in `hirpdag_hashconsing::IterativeDrop`,
whose drop works in three tiers:

1. A drop that frees nothing — a shared handle, or any handle of a reference
   type that never frees on the spot — is a plain drop. A new
   `Reference::strong_is_unique` tells them apart; its default says no.
2. A drop that frees counts the frees nested on the thread's stack. Up to 32
   nest by plain recursion, as before.
3. Past 32, the handle is erased (`Reference::strong_into_deferred_drop`) and
   put on a per-thread queue, and the drop that started the queue frees the
   queued handles one at a time in a loop, each with a fresh budget of 32.

```text
drop(root) ── free ── free ── … 32 levels … ── queue(h₁)
                                               drain loop:
                                                 free(h₁) ── … 32 levels … ── queue(h₂)
                                                 free(h₂) ── … 32 levels … ── queue(h₃)
```

However deep the graph, the stack holds at most 64 nested frees, and only one
free in 32 goes through the queue.

The unsafe code this needs lives in `hirpdag_hashconsing`, beside the reference
implementations that already use it; `hirpdag` stays `forbid(unsafe_code)`.

## Considered options

- **Plain recursion within a budget, a queue past it (chosen).** Covers every
  edge and every reference type, through two trait methods whose defaults keep
  the old behaviour. What it costs was measured against each tier's absence:
  - Queueing every freed node (no budget) cost 8–11% on the free-heavy `churn`
    benchmark: erasing the handle and the indirect call, per free.
  - Counting nesting on every drop, shared or not, cost 35–40% on
    `builder_edits` with `RefLeak`, whose drop is otherwise free, and 3–5% with
    `RefArc`. Checking uniqueness first removes it.
  - Queueing every free past the budget cost 10% on `rewrite_chain`, which frees
    2,000-deep chains: about 8 ns more per queued node than recursion. Giving
    each queued free a fresh budget amortises that over 32 nodes.
  With all three tiers, `churn`, `builder_edits`, `rewrite_chain` and
  `sparse_rewrite` are within the run-to-run spread of the code before, for
  every preset measured.
- **Take the data out of the last handle (`Arc::into_inner`) onto a generated
  per-module queue.** Safe code for `Arc` and `Rc`, but moving the handle out
  of `HirpdagRef` during `Drop` needs either `Option<R>`, a branch on every
  field access, or `unsafe` in `hirpdag`. It also copies each freed node's data
  and adds generated code per module. Rejected.
- **A generated `Drop` on the data struct that detaches its children**, the
  usual fix for a linked list. Only `Option` and `Vec` fields can be detached;
  a plain reference field, a tuple or an enum payload has nothing to swap in.
  It would also forbid moving fields out of the data struct and run on every
  discarded temporary. Rejected.
- **Free on a background thread.** Moves the cost of freeing off the calling
  thread, but needs `Send` (so not `RefRc`), makes frees asynchronous, costs a
  channel send per free, and still recurses on that thread unless it also has
  this queue. Left as a possible optimisation on top of this one.

## Consequences

- A graph of any depth is freed on any stack, for every reference type.
  `test_suite/tests/deep_drop.rs` frees a 10,000-node chain on a 64 KiB thread
  for every preset. `hirpdag_hashconsing`'s unit tests free 100,000-node chains
  of every reference type, chains whose nodes hold their child twice, and chains
  whose handles eight threads drop at once.
- `Reference` gains `strong_is_unique` and `strong_into_deferred_drop`. Their
  defaults (no, and boxing the handle) keep a third-party reference type
  compiling and behaving as before: deep graphs of it still recurse until it
  implements `strong_is_unique`. The bundled `RefArc`, `RefRc` and `RefSep*`
  implement both; `RefLeak` never frees and `RefTlc` frees at its flush, so both
  keep the defaults and their drops cost nothing extra.
- The uniqueness check races with other threads, in either direction. A handle
  wrongly judged last is counted or queued and its drop only decrements; one
  wrongly judged shared is dropped directly, and its children count from zero
  again. A race costs one budget of recursion, never an unbounded amount. Every
  node is still freed exactly once.
- Past the budget, a freed node's children wait in the queue until the drain
  reaches them, a moment later on the same thread. A hash-consing table can
  still hand out such a node in the meantime, which is correct: it is still
  valid.
- `RefTlc` now flushes until its buffer is empty, instead of making one pass.
  Before, applying a buffered decrement that freed a node buffered its
  children's decrements for the *next* flush, so a deep graph was freed one
  level per 4,096 drops, which on a long-running thread meant never. At thread
  exit its drops go through the queue; before, they recursed inside the
  thread-local destructor and died with a bare `SIGSEGV`.
- If a drop panics during a drain, the handles still queued are leaked rather
  than dropped while unwinding, where a second panic would abort.
