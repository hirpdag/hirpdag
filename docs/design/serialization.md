# Design: DAG-aware Serialization / Deserialization

Status: implemented (see `docs/adr/0001-serde-dag-aware-serialization.md`)

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

### A. serde as the trait layer + postcard (binary) + serde_json (text) — **selected**

- The DAG-awareness problem is *orthogonal to the byte format*: it is solved by an
  archive structure (topologically ordered node table + `u64` indices) and by custom
  `Serialize`/`Deserialize` impls on the generated ref types. serde lets us write that
  logic once and get every serde format for free.
- `#[derive(serde::Serialize, serde::Deserialize)]` can simply be appended to the
  already-generated data structs and enums — near-zero code in hirpdag for field
  encoding, which is exactly the "only customize HirpdagRef + surrounding structure"
  requirement.
- The secondary text format requirement becomes trivial: the same archive serialized
  through `serde_json` instead of `postcard`.
- postcard as the primary binary format: compact varint encoding (the `u64` node
  indices cost 1 byte while small), stable wire format with a published spec,
  `no_std`, tiny, well maintained, solid mid-pack performance in the benchmark.
  Because the DAG layer is format-agnostic, swapping postcard for bincode/bitcode
  later is a two-line change.

### B. rkyv (zero-copy) — rejected for v1

- rkyv's headline advantage is zero-copy access to the archived bytes. Hirpdag cannot
  use that: loaded nodes must be **re-interned through the hashcons table** (to merge
  with live nodes, recompute meta, and get fresh creation IDs), so a full
  reconstruction pass is mandatory and the zero-copy benefit evaporates.
- rkyv relies on `unsafe`; the `hirpdag` crate is `#![forbid(unsafe_code)]`.
- It imposes a parallel "archived type" system with heavy trait bounds across the
  generic `Reference`/`Table` abstractions — a large code footprint, against the
  smallness goal. No text format.
- rkyv remains interesting for the separate TODO item about contiguous node memory,
  but that is an allocator/layout concern, not a serialization-format concern.

### C. Schema-IDL formats (flatbuffers, capnp, prost/protobuf) — rejected

Require maintaining an external schema that duplicates the `#[hirpdag]` type
definitions. Violates "serialization should be done on the hirpdag struct".

### D. Own-derive binary crates (borsh, speedy, savefile, bitcode-native) — rejected

Comparable derive ergonomics for binary only, but no free text format, and their
custom-type extension points are no simpler than serde's. bitcode's serde mode stays
available as an alternative *backend* under option A anyway.

### E. Hand-rolled binary format — rejected

Full control, zero deps, but hirpdag would own byte-level encoding of every field
type, endianness, string/varint encoding, and a JSON writer besides. Largest code
size of all options.

## Selected design

### Archive layout (logical structure, same for binary and JSON)

```text
HirpdagArchive
├── version: u32                  format version (starts at 1)
├── nodes: NodeSeq                node table, topological order (children first)
│     └── [ HirpdagArchiveNode ]  tagged union over all #[hirpdag] struct types
│           e.g. Expr(HirpdagStructExpr) | Variables(HirpdagStructVariables)
└── roots: Vec<HirpdagAnyRef>     tagged refs, each encoding a u64 node index
```

- A node's `HirpdagRef` fields are encoded as `u64` indices into `nodes`.
- Collection is a post-order DFS, so **every child's index is strictly smaller than
  its parent's**. The deserializer enforces `index < current_node_index` (and
  `index < nodes.len()` for roots): a single forward pass reconstructs everything,
  forward references are rejected, and cycles are unrepresentable by construction.
- Node types that are `#[hirpdag]` *enums* (e.g. `ExprKind`) are not hashconsed and
  have no table; they are inlined into their parent node's payload by the serde
  derive, recursively, until a ref type is reached. Only struct types appear in the
  node table.
- Binary: `postcard` (enum tags and `u64` indices are varints). A short magic prefix
  (`b"HPDG"`) precedes the postcard payload for file identification.
- Text: the same `HirpdagArchive` through `serde_json`. Refs appear as plain numbers,
  nodes as `{"Expr": {...}}`-style tagged objects. Field order (`nodes` before
  `roots`) must be preserved if a JSON file is edited by hand.

### How refs are encoded: session context

serde's `Serialize`/`Deserialize` traits carry no user state, so the (de)serialization
session state lives in a thread-local scoped context, generated per hirpdag module by
`#[hirpdag_end]` (mirroring how each module already gets its own table statics):

- **Serialize session**: `creation_id → u64 index` map. `hirpdag_get_creation_id()`
  is globally unique per interned node across all types, so one map suffices.
- **Deserialize session**: `Vec<HirpdagAnyRef>` of already-reconstructed nodes,
  indexed by node index.

Generated `impl Serialize for Foo` (the ref wrapper) looks up its creation ID in the
session map and writes the `u64`; a missing entry (or no active session) is an error —
this is what makes serialization *always* DAG-aware, there is no accidental
tree-expansion path. `impl Deserialize for Foo` reads a `u64`, bounds-checks it,
resolves it in the session vec, and type-checks the variant.

Sessions are established only inside the generated entry points below. Per-thread
scoping means concurrent (de)serialization on different threads works; re-entrant use
on one thread is an error.

### Serialization algorithm

1. `hirpdag_serialize_*(roots)` opens a session and runs the **collect phase**: a
   post-order DFS from each root in order. Dedup by creation ID; on first visit,
   register the node's data (a clone of the interned `HirpdagStructFoo`) in the node
   list and record its index.
2. **Emit phase**: serialize `HirpdagArchive` — the node list in order (ref fields
   resolve through the now-complete session map), then the roots.

Output is deterministic for a given DAG and root order (no hash-map iteration order
leaks into the output; the node list is in DFS completion order).

The collect walk uses a small `HirpdagCollect<C>` trait in `hirpdag::base` with the
same shape as the existing `HirpdagRewritable<T>` / `HirpdagComputeMeta` patterns:
no-op impls for numbers/`String`, structural impls for `Option`/`Vec`, and generated
impls for data structs, enums, and ref types.

### Deserialization algorithm

1. Check magic/version, open a session.
2. Deserialize `nodes` via a custom `NodeSeq` visitor: each `HirpdagArchiveNode` is
   deserialized (its ref fields resolve against the session), then **immediately
   re-interned via `hirpdag_hashcons()`** and the resulting ref is pushed into the
   session vec — so node *i+1* can reference it. Interning recomputes meta and
   assigns fresh creation IDs, and dedups against nodes already live in the process.
3. Deserialize `roots`, resolve, return `Vec<HirpdagAnyRef>`.

Re-interning uses the raw hashcons path (`spawn`-equivalent), **not** `new()`: the
serialized data was produced from already-normalized nodes, so normalizers must not
run again.

Consequences that fall out for free:

- Sharing is preserved exactly (equal subgraphs re-intern to the same pointer).
- Deserializing a file twice, or into a process that already has some of the nodes,
  merges rather than duplicates.
- Round-tripping in one process yields pointer-equal nodes.

## Implementation plan

### Phase 1 — `hirpdag` crate: base support (~150 LOC)

- `Cargo.toml`: add `serde` (with `derive`), `postcard` (with `use-std`) and
  `serde_json` as dependencies (unconditional — no feature matrix; a feature gate can
  be added later if a user needs it). Re-export all three from `hirpdag` so generated
  code can reference `hirpdag::serde` via `#[serde(crate = "...")]` without users
  adding the dependencies themselves.
- New `hirpdag/src/base/serialize.rs`:
  - `trait HirpdagCollect<C> { fn hirpdag_collect(&self, ctx: &mut C); }` with no-op
    impls for `IsNumber` types, `String`, and structural impls for `Option<T>`,
    `Vec<T>` (mirrors `rewrite.rs`).
  - `enum HirpdagSerializeError { Io, Format(String), BadMagic, UnsupportedVersion(u32),
    InvalidNodeIndex { index: u64, limit: u64 }, RootTypeMismatch, NoSession, ... }`
    with `Display`/`Error` impls and `From` conversions for postcard/serde_json errors.
  - Small helpers to write/check the magic prefix and version.

### Phase 2 — `hirpdag_derive`: per-type generation

- `DATA_TYPES` registry: store `(name, kind)` where kind distinguishes struct
  (hashconsed, gets a table + node-table variant) from enum (inline payload).
- `expand_hirpdag_struct` additionally emits:
  - `#[derive(Serialize, Deserialize)]` (crate-pathed to `hirpdag::serde`) on
    `HirpdagStructFoo`.
  - `impl Serialize for Foo` — session lookup of creation ID → emit `u64`.
  - `impl<'de> Deserialize<'de> for Foo` — read `u64` → session resolve + variant
    check.
  - `impl<C: …> HirpdagCollect<…> for Foo` — dedup by creation ID, recurse into
    fields first, then register `(**self).clone()` as a node.
  - `impl HirpdagCollect for HirpdagStructFoo` — `self.field.hirpdag_collect(ctx)`
    per field (same fold style as `get_fields_rewrite`).
- `expand_hirpdag_enum` additionally emits the serde derive and a per-variant
  `HirpdagCollect` impl.

Generation is unconditional (no attribute flag, no feature matrix): less generated
code, no cfg plumbing; serde+postcard are small, ubiquitous dependencies. An opt-out
attribute (`#[hirpdag(no_serialize)]`) can be added later if a user needs it.

### Phase 3 — `hirpdag_derive`: `#[hirpdag_end]` module-level generation

- `enum HirpdagArchiveNode { Foo(HirpdagStructFoo), … }` (struct types only) with the
  serde derive.
- `pub enum HirpdagAnyRef { Foo(Foo), … }` with the serde derive (variant tag + inner
  ref-as-`u64`), plus `From<Foo>` and `TryFrom<HirpdagAnyRef> for Foo` conveniences.
- `HirpdagCollectCtx { seen: HashMap<u64 /*creation_id*/, u64 /*index*/>,
  nodes: Vec<HirpdagArchiveNode> }`.
- Thread-local serialize session (`creation_id → index` map) and deserialize session
  (`Vec<HirpdagAnyRef>`), with RAII guards so sessions are cleaned up on error paths.
- `struct NodeSeq` with custom serde impls: serialize emits the collected node vec as
  a seq; deserialize is a `SeqAccess` visitor that interns each element immediately
  and appends the resulting ref to the session (this is what enables the single
  forward pass).
- Public entry points:
  - `hirpdag_serialize(roots: &[HirpdagAnyRef]) -> Result<Vec<u8>, HirpdagSerializeError>`
  - `hirpdag_deserialize(bytes: &[u8]) -> Result<Vec<HirpdagAnyRef>, HirpdagSerializeError>`
  - `hirpdag_serialize_json` / `hirpdag_deserialize_json`

### Phase 4 — tests (`test_suite`)

- Round trip of the README `Expr` DAG (binary and JSON): equal roots, and
  pointer-equality with the originals in-process.
- **Sharing preserved**: serialize a Fibonacci-style DAG (`fibonacci.rs` bench
  shape); assert the archive node count equals the unique node count and the byte
  size grows linearly with N.
- Multiple roots of different types, including roots sharing subgraphs — shared nodes
  appear once.
- Deserialize twice → both results pointer-equal (hashcons merge).
- Error cases: bad magic, truncated input, forward/out-of-range node index, root
  variant mismatch, ref (de)serialization outside a session.
- Two hirpdag modules in one binary (`modules.rs` shape) don't cross-talk.

### Phase 5 — docs

- Book chapter: format description, DAG-awareness guarantees, examples, caveats.
- README example snippet.

## Caveats / future work

- **Recursive collect**: v1 collect is recursive; extremely deep chains could
  overflow the stack. `HirpdagMeta::height` (u16, saturating) gives a cheap upfront
  signal; an explicit-stack DFS is a contained follow-up.
- **Schema evolution**: v1 requires matching type definitions. Binary enum tags are
  ordinal, so reordering `#[hirpdag]` type declarations or enum variants breaks old
  binary files (JSON is name-tagged and more tolerant). Future: schema fingerprint in
  the header for early, clear errors.
- **Streaming writer**: v1 buffers the archive; a `std::io::Write`-based path is easy
  to add later since postcard supports incremental flavors.
- **bitcode backend**: if size/speed ever matters more, the archive layer is
  format-agnostic; bitcode-serde can be offered as an alternative codec.
