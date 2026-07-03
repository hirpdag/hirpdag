# Serialization

Hirpdag serialization is always DAG-aware: each unique node is written exactly
once, so structural sharing survives a round trip and output size is
proportional to the number of *unique* nodes, not the tree expansion. A
Fibonacci-shaped DAG with two-parent sharing serializes in linear space, where
a naive tree walk would be exponential.

## API

Struct types that may be serialization roots are marked with
`#[hirpdag(root)]`. `#[hirpdag_end]` then generates a `HirpdagArchiveRoots`
struct with one vector per root type (field names are the snake_case type
names), plus entry points:

* `hirpdag_serialize(&HirpdagArchiveRoots) -> Result<Vec<u8>, HirpdagSerializeError>`
  — compact binary (via [postcard](https://crates.io/crates/postcard)).
* `hirpdag_deserialize(&[u8]) -> Result<HirpdagArchiveRoots, HirpdagDeserializeError>`
* `hirpdag_serialize_json` / `hirpdag_deserialize_json` — the same archive as
  human-readable JSON.

Types without `#[hirpdag(root)]` can still appear anywhere *inside* the DAG;
they just cannot be roots. `HirpdagArchiveRoots` implements `Default`, so a
subset of the root types can be set with struct update syntax:

```rust
#[hirpdag(root)]
struct Expr { ... }

#[hirpdag(root)]
struct Variables { ... }

let bytes = hirpdag_serialize(&HirpdagArchiveRoots {
    expr: vec![e1, e2],
    variables: vec![vars],
})?;

let out = hirpdag_deserialize(&bytes)?;
let e1_again: &Expr = &out.expr[0];
```

The error types are distinct (`HirpdagSerializeError` /
`HirpdagDeserializeError`), mirroring serde's separation of `ser::Error` and
`de::Error`.

## Format

The archive is a version, then a node table, then the typed roots
(`HirpdagArchiveRoots`, serialized as one index vector per root type). Nodes
are written in post-order DFS order (children before parents), and
`#[hirpdag]` struct fields that reference other nodes are encoded as `u64`
indices into the node table. `#[hirpdag]` enum values are not hashconsed and
are stored inline inside their parent node.

Because children always precede parents, deserialization is a single forward
pass: forward references are rejected, which also makes cycles
unrepresentable. Each node is re-interned through the hashcons table as it is
decoded, so:

* sharing is restored exactly;
* loading merges with nodes already live in the process (an in-process round
  trip yields pointer-equal nodes);
* metadata and creation IDs are recomputed rather than trusted from the file;
* normalizers do **not** re-run (the archived data was produced from
  already-normalized nodes).

## Caveats

* (De)serialization uses a per-thread session; entry points are not re-entrant
  within a thread (concurrent use on different threads is fine). Serializing a
  hirpdag reference outside a session (e.g. calling `serde_json::to_string` on
  a node directly) is an error — there is no accidental tree-expansion path.
* Binary enum tags are ordinal: reordering `#[hirpdag]` type declarations or
  enum variants breaks previously written binary files. JSON is name-tagged
  and more tolerant.
* The collect walk is recursive; extremely deep chains could overflow the
  stack.

See `docs/design/serialization.md` and
`docs/adr/0001-serde-dag-aware-serialization.md` in the repository for the
full design rationale.
