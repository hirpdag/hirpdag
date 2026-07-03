---
status: accepted
---

# Use serde for DAG-aware serialization, with postcard binary and JSON text formats

Hirpdag needs serialization that always preserves DAG sharing, has a primary binary
format plus a secondary text format, supports multiple roots per file, and derives
almost everything from the `#[hirpdag]` struct while keeping hirpdag's code small.
We implement DAG-awareness once as a format-agnostic archive layer (a topologically
ordered node table indexed by `u64`, with `HirpdagRef` fields encoded as indices via
custom serde impls and thread-local session state) and delegate the byte format to
serde, using postcard for binary and serde_json for text — the same archive code
serves both formats, and only the ref encoding and the surrounding archive structure
are hand-written.

## Considered options

- **serde + postcard/serde_json (chosen)** — DAG logic is written once against
  serde's traits; field encoding is fully derived; the text format is free; the
  binary codec is swappable (bincode, bitcode) without touching DAG logic. Costs:
  serde carries no user state, so ref index resolution uses per-module thread-local
  sessions, and (de)serialization is not re-entrant within a thread.
- **rkyv (zero-copy)** — rejected. Loaded nodes must be re-interned through the
  hashcons table (to merge with live nodes, recompute meta, assign creation IDs), so
  a full reconstruction pass is mandatory and zero-copy access buys nothing. rkyv
  also requires `unsafe` (hirpdag is `#![forbid(unsafe_code)]`), imposes a parallel
  archived-type system across the generic `Reference`/`Table` abstractions, and has
  no text format.
- **Schema-IDL formats (flatbuffers, capnp, protobuf)** — rejected: require an
  external schema duplicating the `#[hirpdag]` type definitions.
- **Own-derive binary crates (borsh, speedy, bitcode-native)** — rejected: binary
  only, no simpler custom-type extension points than serde.
- **Hand-rolled binary format** — rejected: hirpdag would own byte-level encoding of
  every field type plus a JSON writer; largest code footprint of all options.

## Consequences

- Nodes are written in post-order DFS, so every child index is strictly smaller than
  its parent's; the deserializer resolves indices against already-reconstructed nodes
  only, making forward references errors and cycles unrepresentable.
- Deserialization re-interns through the raw hashcons path (not `new()`): normalizers
  do not re-run, sharing is restored exactly, and loading merges with nodes already
  live in the process (in-process round trips are pointer-equal).
- A ref serialized outside a session is a hard error — there is no accidental
  tree-expansion path, so serialization is always DAG-aware.
- Roots are typed: struct types opt in with `#[hirpdag(root)]`, and the generated
  `HirpdagArchiveRoots` struct (one vector per root type) is the serialize input and
  deserialize output. Error types are split (`HirpdagSerializeError` /
  `HirpdagDeserializeError`), mirroring serde's `ser::Error`/`de::Error` separation.
- `serde`, `postcard`, and `serde_json` become unconditional dependencies of
  `hirpdag` (re-exported so users don't declare them); serialization code is
  generated unconditionally, avoiding a feature/cfg matrix in the proc macro. An
  opt-out attribute can be added later if a user needs it.
- Binary enum tags are ordinal: reordering `#[hirpdag]` type declarations or enum
  variants breaks previously written binary files (JSON is name-tagged and more
  tolerant). A schema fingerprint in the header is future work.
- The collect walk is recursive; extremely deep DAG chains could overflow the stack
  (an explicit-stack DFS is a contained follow-up).

See `docs/design/serialization.md` for the full design.
