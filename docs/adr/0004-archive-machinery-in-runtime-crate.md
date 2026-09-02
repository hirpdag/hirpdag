---
status: accepted
---

# Keep the archive machinery in the runtime crate, behind `HirpdagArchive`

> Amended by ADR-0005: the two session slots this ADR left in the generated
> code are gone, along with the `Interned` type and `intern` method of
> `HirpdagArchive`. The rest — the machinery in the runtime crate, behind a
> trait a module implements — stands, and this change is what the resulting
> test surface was for.

ADR-0001 chose serde with a DAG-aware archive layer, and `#[hirpdag_module]`
emitted the whole of that layer into each user crate: the collect context, the
two thread-local sessions and their guards, the node table's `Serialize` and
`Deserialize`, the archive container, and the four entry points — around 370
lines per module, plus around 86 per data type for a ref's `Serialize`,
`Deserialize` and collect. Only a small part of that is specific to a module:
the node enum, the intern arms and the roots fields. Everything else was the
same text every time, and none of it could be exercised without expanding the
macro and compiling a crate that uses it, so the archive — the most intricate
code in the project — had no tests of its own. `test_suite/tests/serialization.rs`
covered it, but only end-to-end, and a change to the session rules had to be
debugged through macro expansion.

The machinery now lives in `hirpdag::base::archive`, written once and generic
over a module's node types. A module supplies an implementation of
`HirpdagArchive` (its `Node`, `Interned` and `Roots` types, its schema
fingerprint, how to intern a decoded node, and access to its two session slots)
and one `HirpdagArchiveMember` implementation per data type (the type's name for
error messages, and how to pick it back out of a reconstructed node). The macro
generates those implementations and four one-line entry points; the traversal,
the session rules, the node table codec and the format handling are library
code.

A `static` cannot name a generic parameter, so `hirpdag::base` cannot hold a
thread-local vector of a module's reconstructed nodes. The two session slots
therefore stay where the macro declares them — one pair per module, as before —
and are reached through `with_ser_session` / `with_de_session`. Session
*semantics* (one at a time per thread, closed on drop, `SessionActive` on
re-entry) are entirely in the library; what the macro emits is two declarations.

## Considered options

- **Machinery in the runtime crate behind a trait (chosen)** — the archive is
  testable through its own interface with a hand-written schema, the session
  rules exist once, and the generated code per module drops to a schema
  description. Costs: two public traits and three `#[doc(hidden)] pub` types in
  every module (the node enums must be public so the schema type does not leak
  private types through a public trait impl), and one more layer to read through
  when following a serialize call.
- **Leave it generated** — rejected. It is the status quo: no test surface short
  of a full macro expansion, and every fix to the archive is a fix to a string
  of tokens.
- **Move only the module-level machinery, keep the per-type impls generated** —
  rejected. The per-type code is the larger half of the duplication and carries
  the index bounds check and the node-type-mismatch error, which are exactly the
  paths worth testing directly.
- **Hold the whole session in `hirpdag::base` behind `Box<dyn Any>`** — rejected.
  It would remove the generated declarations, but at the price of a downcast per
  node on the deserialization path, for two lines of generated code.
- **Move the sessions into `hirpdag::base` as one slot per thread rather than one
  per module** — rejected for now. The serialization session's state
  (`HashMap<u64, u64>`) is not generic, so it *could* live in the library, but
  the deserialization side cannot, and splitting the pair across two homes buys
  nothing. It would also narrow ADR-0001's "one session per thread, per module"
  to "one per thread", a semantic change with no demand behind it.
- **Thread the state through `DeserializeSeed` instead of a thread-local** —
  rejected, as in ADR-0001. `Serialize` has no equivalent, so the serialization
  side would keep its thread-local anyway, and the seed would have to be
  threaded through every derived field impl, which serde's derive cannot do.

## Consequences

- The wire format is unchanged, byte for byte, in both formats.
  `tests/serialization.rs` and `benches/serde_roundtrip.rs` compile and pass
  untouched across all 12 configuration presets, which is what establishes that
  the move changed no behaviour.
- `hirpdag/src/base/archive.rs` carries its own tests, driven through
  `HirpdagArchive` with a hand-written schema standing in for a module: two data
  types, a shared child, both formats, and the error paths (out-of-range and
  forward indices, node type mismatch, refs outside a session, re-entrant
  sessions, bad magic, schema mismatch, unsupported version, truncated input).
  A bug in the archive now fails a unit test in the crate that owns it.
- Each module gains `HirpdagArchiveSchema`, and `HirpdagArchiveNode` /
  `HirpdagNodeRef` become `#[doc(hidden)] pub` rather than private. A module with
  no `#[hirpdag(root)]` type archives `HirpdagNoRoots` and still gets no entry
  points.
- Serializing no longer clones the roots: the archive borrows them on the way
  out.
- `hirpdag_derive/src/lib.rs` loses 281 lines (1754 to 1579), and what remains of
  serialization in it describes a schema rather than implementing a format.
