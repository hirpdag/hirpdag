---
status: accepted
amends: [0001-serde-dag-aware-serialization, 0004-archive-machinery-in-runtime-crate]
---

# Encode references to indices outside serde, so an archive carries no ambient state

ADR-0001 encoded a `HirpdagRef` inside its own `Serialize` implementation, by
looking its creation id up in a map that the entry point had put in a
thread-local for the duration of the call — serde's traits carry no user state,
so the state had to reach the implementation some other way. ADR-0004 moved
every rule about those sessions into `hirpdag::base::archive`, but the two slots
themselves stayed in the generated code, because a `static` cannot name a
generic parameter and the deserialization session holds a vector of a module's
own node type:

```rust
std::thread_local! {
    static HIRPDAG_SER_SESSION:
        std::cell::RefCell<Option<hirpdag::base::HirpdagSerSession>> =
            const { std::cell::RefCell::new(None) };
    static HIRPDAG_DE_SESSION: std::cell::RefCell<Option<Vec<HirpdagNodeRef>>> =
            const { std::cell::RefCell::new(None) };
}
```

The premise underneath all of that is that a reference is translated *while*
serde is running. It does not have to be. Every hirpdag type now names an
*archived form* — the same value with each reference replaced by the `u64` index
of the node it names — and the translation happens in a phase of its own on
either side of serde:

```text
roots --collect--> node table --encode--> archive --serde--> bytes
bytes --serde--> archive --decode (resolve + intern)--> roots
```

What serde is handed is plain data with no hirpdag types in it, so it needs no
state, and the two phases that do need state take it as an argument: encoding
takes the index the collect phase built, and decoding takes the nodes
reconstructed so far. The thread-locals are gone, and with them the session
concept, its two guards, the two `SessionActive` errors, and the ref
`Serialize`/`Deserialize` implementations that could only be called from inside
one.

`HirpdagArchived<R>` is the trait that names the archived form, in the same
shape as `HirpdagCollect`: identity for numbers and `String`, structural for
`Option` and `Vec`, `u64` for a reference, generated for the data types. Because
the leaf and container implementations are generic over which module's node
table a reference resolves against, a call has to say which — so each module
also generates a `hirpdag_archive_encode` / `hirpdag_archive_decode` pair that
pins it once.

## Considered options

- **Encode references outside serde (chosen).** The archive becomes plain data,
  so there is nothing to thread through serde and nothing ambient to scope. It
  also removes a failure mode rather than reporting it: a reference has no
  serde implementation at all, so "serializing a ref outside an archive", which
  would expand a DAG into a tree, stops being a runtime error and becomes a
  compile error. Costs: one archived type generated per data type (a struct's
  fields and an enum's variants, with reference types replaced), and one more
  pass over the node table in each direction.
- **`DeserializeSeed` for the deserialization side.** Rejected — and, with the
  option above taken, unnecessary. A seed does carry state into
  `Deserialize`, but only into the implementations that thread it onward, and
  serde's derive does not: every field type between the archive and a reference
  would need a hand-written seeded implementation. It also only ever addressed
  half the problem, since `Serialize` has no equivalent.
- **A wrapper `Serializer`/`Deserializer` carrying the state.** Rejected. Every
  method of both traits, and of their sub-access traits, would have to be
  forwarded, and a reference's implementation — generic over `S: Serializer` —
  still could not tell that it had been handed the wrapper rather than a plain
  serializer.
- **Type-erase the session into the runtime crate (`Box<dyn Any>`).** Rejected
  in ADR-0004 and still rejected: a downcast per node, to keep a mechanism that
  no longer needs to exist.
- **Leave the thread-locals.** Rejected. They are the reason serialization is
  not re-entrant, the reason a ref's serde implementations can fail at runtime,
  and two declarations of generated code whose semantics live in another crate.

## Consequences

- The wire format is unchanged, byte for byte, in both formats: an archive
  written before this change reads back after it. The archived types are
  declared so that serde sees exactly what it saw before — the same struct field
  names, the same externally tagged enum variants in the same order, `u64` where
  a reference was already written as `u64`.
- Nothing about an archive is ambient, so archives nest and run concurrently
  without arrangement. `HirpdagSerializeError::SessionActive` and
  `HirpdagDeserializeError::SessionActive` are gone.
- Index and node-type failures are typed rather than stringly:
  `HirpdagDeserializeError::InvalidNodeIndex` and `NodeTypeMismatch` replace the
  `Format(String)` messages they used to be squeezed into, because they no
  longer have to travel through serde's `custom` error path.
- Field order in a hand-written JSON archive no longer matters. Resolution
  happens after the whole archive is decoded, so `roots` before `nodes` reads
  the same as `nodes` before `roots`.
- `HirpdagStructFoo` and `#[hirpdag]` enums no longer derive `Serialize` /
  `Deserialize`, and ref types no longer implement them. A user who was
  serializing one of those types outside an archive has to derive serde on their
  own type instead.
- The generated code per module loses the two thread-locals and gains, per data
  type, an archived type and its two conversions — a wash in size, and what is
  left is a description of a data shape rather than a use of hidden state.
- Peak memory holds the archived node table alongside the live one for the
  length of a call, in both directions. The round-trip benchmark
  (`benches/serde_roundtrip.rs`, 2000 nodes, both formats, all presets) shows no
  measurable change either way.
