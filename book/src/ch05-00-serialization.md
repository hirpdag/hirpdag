# Serialization

Hirpdag serialization is always DAG-aware: each unique node is written exactly
once, so structural sharing survives a round trip and output size is
proportional to the number of *unique* nodes, not the tree expansion. A
Fibonacci-shaped DAG with two-parent sharing serializes in linear space, where
a naive tree walk would be exponential.

## API

Struct types that may be serialization roots are marked with
`#[hirpdag(root)]`. `#[hirpdag_module]` then generates a `HirpdagArchiveRoots`
struct with one vector per root type (field names are the snake_case type
names), plus entry points:

* `hirpdag_serialize(&HirpdagArchiveRoots) -> Result<Vec<u8>, HirpdagSerializeError>`,
  compact binary (via [postcard](https://crates.io/crates/postcard)).
* `hirpdag_deserialize(&[u8]) -> Result<HirpdagArchiveRoots, HirpdagDeserializeError>`
* `hirpdag_serialize_json` / `hirpdag_deserialize_json`, the same archive as
  human-readable JSON.

Types without `#[hirpdag(root)]` can still appear anywhere *inside* the DAG;
they just cannot be roots. Each root type gets a field named `roots_` plus the
type's name, carried through verbatim so that two types differing only in case
cannot name the same field (see
[ADR-0006](https://github.com/hirpdag/hirpdag/blob/main/docs/adr/0006-generated-names-from-the-declared-name.md)).
`HirpdagArchiveRoots` implements `Default`, so a subset of the root types can be
set with struct update syntax:

```rust
#[hirpdag(root)]
struct Expr { ... }

#[hirpdag(root)]
struct Variables { ... }

let bytes = hirpdag_serialize(&HirpdagArchiveRoots {
    roots_Expr: vec![e1, e2],
    roots_Variables: vec![vars],
})?;

let out = hirpdag_deserialize(&bytes)?;
let e1_again: &Expr = &out.roots_Expr[0];
```

The error types are distinct (`HirpdagSerializeError` /
`HirpdagDeserializeError`), mirroring serde's separation of `ser::Error` and
`de::Error`.

## Format

The archive is a version, then a node table, then the typed roots
(`HirpdagArchiveRoots`, serialized as one index vector per root type, keyed by
the `roots_<Type>` field name in JSON). Nodes
are written in post-order DFS order (children before parents), and
`#[hirpdag]` struct fields that reference other nodes are encoded as `u64`
indices into the node table. `#[hirpdag]` enum values are not hashconsed and
are stored inline inside their parent node.

The binary header also carries a schema fingerprint: a stable
hash of the module's hirpdag type definitions (computed at macro expansion
time) plus a human-readable name (the defining package and its type list).
Deserializing a binary archive written by different type definitions fails up
front with a `SchemaMismatch` error naming both schemas, instead of
misparsing:

```text
hirpdag: schema mismatch: archive was written by
"my_app:Item,Kind,Node" (hash 0x…) but is being read by
"my_app:Widget" (hash 0x…)
```

The JSON format deliberately omits the fingerprint so it stays hand-editable.

Because children always precede parents, deserialization is a single forward
pass: forward references are rejected, which also makes cycles
unrepresentable. Each node is re-interned through the hashcons table as it is
decoded, so:

* sharing is restored exactly;
* loading merges with nodes already live in the process (an in-process round
  trip yields pointer-equal nodes);
* metadata and creation IDs are recomputed rather than trusted from the file;
* normalizers do not re-run (the archived data was produced from
  already-normalized nodes).

## Caveats

* A hirpdag reference has no `Serialize` or `Deserialize` implementation of
  its own: what serde sees is an archive, in which every reference has already
  become a node table index. So there is no accidental tree-expansion path —
  `serde_json::to_string` on a node directly does not compile — and no ambient
  state either: archives nest and run concurrently without arrangement.
* Binary enum tags are ordinal: reordering `#[hirpdag]` type declarations or
  enum variants changes the wire format. The schema fingerprint turns this
  into an early, clear error for binary archives; JSON is name-tagged, more
  tolerant, and unfingerprinted.
* The collect walk is recursive; extremely deep chains could overflow the
  stack.

See `docs/design/serialization.md` in the repository for the full design, and
`docs/adr/` for the decisions behind it.
