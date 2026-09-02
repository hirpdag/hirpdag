# Design: DAG-aware Serialization / Deserialization

Status: implemented. This describes the archive as it stands; the decisions
behind it, and what was considered instead, are in `docs/adr/` (0001, 0004 and
0005).

## Requirements

- Serialization must always be DAG-aware: every unique node is written exactly once;
  structural sharing survives a round trip. Output size is proportional to the number
  of *unique* nodes, not the tree expansion (a Fibonacci-style DAG must serialize in
  linear, not exponential, space).
- Primary format is binary. A text format (JSON-like) is secondary but desirable.
- Multiple root nodes (possibly of different hirpdag types, possibly sharing
  subgraphs) can be serialized into one file.
- Serialization is defined on the hirpdag struct itself. Only two things are custom:
  1. how `HirpdagRef` fields are encoded/decoded, and
  2. the surrounding archive structure (header, node table, root list).
  Everything else (field encoding, enums, `Vec`, `Option`, `String`, numbers) must be
  derived, not hand-written.
- Node indices in the serialized format are `u64`.
- Keep the hirpdag code small.

## Options considered

Candidates from [rust_serialization_benchmark](https://github.com/djkoloski/rust_serialization_benchmark):

### A. serde as the trait layer + postcard (binary) + serde_json (text), selected

- The DAG-awareness problem is *orthogonal to the byte format*: it is solved by an
  archive structure (topologically ordered node table + `u64` indices) that is built
  before the bytes are written and taken apart after they are read. serde lets us
  write that logic once and get every serde format for free.
- `#[derive(serde::Serialize, serde::Deserialize)]` can be appended to the
  already-generated data structs and enums, so hirpdag holds almost no code for
  field encoding, which is exactly the "only customize HirpdagRef + surrounding
  structure" requirement.
- The secondary text format is then the same archive serialized through `serde_json`
  instead of `postcard`.
- postcard as the primary binary format: compact varint encoding (the `u64` node
  indices cost 1 byte while small), stable wire format with a published spec,
  `no_std`, tiny, well maintained, solid mid-pack performance in the benchmark.
  Because the DAG layer is format-agnostic, swapping postcard for bincode/bitcode
  later is a two-line change.

### B. rkyv (zero-copy), rejected for v1

- rkyv's main advantage is zero-copy access to the archived bytes. Hirpdag cannot
  use that: loaded nodes must be re-interned through the hashcons table (to merge
  with live nodes, recompute meta, and get fresh creation IDs), so a full
  reconstruction pass is mandatory and zero-copy buys nothing.
- rkyv relies on `unsafe`; the `hirpdag` crate is `#![forbid(unsafe_code)]`.
- It imposes a parallel "archived type" system with heavy trait bounds across the
  generic `Reference`/`Table` abstractions, a large code footprint, against the
  smallness goal. No text format.
- rkyv remains interesting for the separate TODO item about contiguous node memory,
  but that is an allocator/layout concern, not a serialization-format concern.

### C. Schema-IDL formats (flatbuffers, capnp, prost/protobuf), rejected

Require maintaining an external schema that duplicates the `#[hirpdag]` type
definitions. Violates "serialization should be done on the hirpdag struct".

### D. Own-derive binary crates (borsh, speedy, savefile, bitcode-native), rejected

Comparable derive ergonomics for binary only, but no free text format, and their
custom-type extension points are no simpler than serde's. bitcode's serde mode stays
available as an alternative *backend* under option A anyway.

### E. Hand-rolled binary format, rejected

Full control, zero deps, but hirpdag would own byte-level encoding of every field
type, endianness, string/varint encoding, and a JSON writer besides. Largest code
size of all options.

## Selected design

### Archive layout (logical structure, same for binary and JSON)

```text
Archive
├── version: u32                  format version (starts at 1)
├── nodes: Vec<HirpdagArchiveNode>  node table, topological order (children first)
│     └── HirpdagArchiveNode      tagged union over all #[hirpdag] struct types
│           e.g. Expr(HirpdagArchiveStructExpr)
│              | Variables(HirpdagArchiveStructVariables)
└── roots: HirpdagArchiveRootIndices  one Vec per #[hirpdag(root)] type, each ref
                                  encoded as a u64 node index
```

- A node's `HirpdagRef` fields are encoded as `u64` indices into `nodes`.
- Collection is a post-order DFS, so every child's index is strictly smaller than
  its parent's. The deserializer enforces `index < current_node_index` (and
  `index < nodes.len()` for roots): a single forward pass reconstructs everything,
  forward references are rejected, and cycles are unrepresentable by construction.
- Node types that are `#[hirpdag]` *enums* (e.g. `ExprKind`) are not hashconsed and
  have no table; they are inlined into their parent node's payload, recursively,
  until a ref type is reached. Only struct types appear in the node table.
- Binary: `postcard` (enum tags and `u64` indices are varints). The header is a
  PNG-style 8-byte magic prefix (`b"\x89HPDG\r\x1a\n"`, which is a high-bit byte
  marking the file as binary, the format name readable as text, and
  text-mode-translation trip bytes ending in a newline) followed by a schema
  fingerprint: a stable FNV-1a hash of the module's type definitions (names, fields,
  variants, root markers, in declaration order) computed at macro expansion time,
  plus a human-readable schema name (package name and type list) for debuggability.
  Decoding verifies the hash and fails with a `SchemaMismatch` error naming both
  schemas if the archive was written by different type definitions.
- Text: the same archive through `serde_json`. Refs appear as plain numbers, nodes as
  `{"Expr": {...}}`-style tagged objects. Indices are resolved after the whole archive
  is decoded, so the order of an archive's three fields does not matter in a JSON file
  edited by hand.

### How refs are encoded: the archived form

serde's `Serialize`/`Deserialize` traits carry no user state, so a ref cannot resolve
its index from inside a serde impl. It does not have to: the reference-to-index
translation is a phase of its own, either side of serde, and what serde sees is plain
data with no hirpdag types in it.

```text
roots --collect--> node table --encode--> archive --serde--> bytes
bytes --serde--> archive --decode (resolve + intern)--> roots
```

Every hirpdag type names an *archived form*: the same value with each `HirpdagRef`
replaced by the `u64` index of the node it names. `HirpdagArchived<R>` in
`hirpdag::base` names it and converts both ways, with the same shape as
`HirpdagCollect`: identity for numbers and `String`, structural for `Option`/`Vec`,
`u64` for a ref, and generated for the data types (`HirpdagArchiveStructFoo`,
`HirpdagArchiveEnumKind`, `HirpdagArchiveRootIndices`). Only the archived form derives
serde; the live types do not, so a ref cannot be serialized on its own at all — the
accidental tree-expansion path is a compile error rather than a runtime one.

The two conversions take the state they need as an argument — the node index on the
way out, the nodes reconstructed so far on the way in — so nothing about an archive is
ambient. Archives nest, and run concurrently, without arrangement.

`R` is what a ref resolves against: `[HirpdagNodeRef]`, the module's reconstructed
nodes. It is a type parameter because the leaf and container impls are shared across
modules, so a call has to say which module's node table it means; each module
generates a `hirpdag_archive_encode` / `hirpdag_archive_decode` pair that says it once.

### Serialization algorithm

1. Collect phase: a post-order DFS from each root in order. Dedup by creation ID; on
   first visit, register the interned node in the node table and record its index.
2. Encode phase: convert each node, and then the roots, into their archived form,
   resolving every ref against the index the collect phase built.
3. Hand the archive — version, node table, roots, all plain data — to postcard or
   serde_json.

Output is deterministic for a given DAG and root order (no hash-map iteration order
leaks into the output; the node list is in DFS completion order).

The collect walk uses a small `HirpdagCollect<C>` trait in `hirpdag::base` with the
same shape as the existing `HirpdagRewritable<T>` / `HirpdagComputeMeta` patterns:
no-op impls for numbers/`String`, structural impls for `Option`/`Vec`, and generated
impls for data structs, enums, and ref types.

### Deserialization algorithm

1. Check magic and fingerprint, then decode the archive with postcard or serde_json.
   This is a plain deserialize: the version is checked as the first field, and the
   node table comes back as archived data holding `u64` indices.
2. Decode phase: walk the node table in order. Each node's indices resolve against the
   nodes reconstructed before it (a forward reference is indistinguishable from an
   out-of-range index, and both are rejected), and the resolved data is immediately
   re-interned via `hirpdag_hashcons()`, so node *i+1* can reference it. Interning
   recomputes meta and assigns fresh creation IDs, and dedups against nodes already
   live in the process.
3. Resolve the roots against the full node table and return the typed
   `HirpdagArchiveRoots`.

Re-interning uses the raw hashcons path (`spawn`-equivalent), not `new()`: the
serialized data was produced from already-normalized nodes, so normalizers must not
run again.

Consequences:

- Sharing is preserved exactly (equal subgraphs re-intern to the same pointer).
- Deserializing a file twice, or into a process that already has some of the nodes,
  merges rather than duplicates.
- Round-tripping in one process yields pointer-equal nodes.

## Where the code lives

- `hirpdag/src/base/serialize.rs` — the format-agnostic pieces: the magic prefix,
  the format version marker, the schema fingerprint and header helpers, the two
  error types, and the two traversal traits (`HirpdagCollect` for the collect walk,
  `HirpdagArchived` for the archived form) with their leaf and container impls.
- `hirpdag/src/base/archive.rs` — the archive itself: the `HirpdagArchive` and
  `HirpdagArchiveMember` interfaces a module implements, the collect context, ref
  resolution, and the four entry points. Its unit tests drive all of it through a
  hand-written schema standing in for a generated module, so a bug in the archive
  fails a test in the crate that owns it.
- `hirpdag_derive/src/lib.rs` — per module: the interned-node enum, the node table
  entry enum, the encode/decode helpers, the `HirpdagArchive` impl and the four
  entry points. Per data type: the archived form and its two conversions, the
  collect impl, and (for structs) the `HirpdagArchiveMember` impl.
- `test_suite/tests/serialization.rs` — end-to-end over a generated module: round
  trips in both formats, sharing preserved through a Fibonacci-shaped DAG, multiple
  and mixed-type roots, re-interning on a second load, hand-written JSON, concurrent
  archives, and the error paths (bad magic, truncated input, unsupported version,
  invalid index, node type mismatch, schema mismatch).
- `test_suite/benches/serde_roundtrip.rs` — build, serialize and deserialize a
  round trip, in both formats, across the configuration presets.

## Caveats / future work

- **Recursive collect.** v1 collect is recursive; extremely deep chains could
  overflow the stack. `HirpdagMeta::height` (u16, saturating) gives a cheap upfront
  signal; an explicit-stack DFS is a contained follow-up.
- **Schema evolution.** v1 requires matching type definitions. Binary enum tags are
  ordinal, so reordering `#[hirpdag]` type declarations or enum variants changes the
  wire format. The schema fingerprint in the binary header catches this with an
  early `SchemaMismatch` error instead of misparsing. JSON is name-tagged, more
  tolerant, and carries no fingerprint (kept hand-editable by design).
- **Streaming writer.** v1 buffers the archive; a `std::io::Write`-based path is easy
  to add later since postcard supports incremental flavors.
- **bitcode backend.** If size or speed ever matters more, the archive layer is
  format-agnostic; bitcode-serde can be offered as an alternative codec.
