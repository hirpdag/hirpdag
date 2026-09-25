---
status: accepted
---

# Set metadata flags per type with `#[hirpdag(flags = path)]`, and read metadata through `hirpdag_get_meta()`

`HirpdagMeta` carries a `u16` bitfield next to `count` and `height`, meant to
let a pass ask whether any node in a subtree has some property, such as
"contains a free variable", without traversing it. A node's bits came from
`HirpdagStruct::hirpdag_flags`, a default method returning 0. The macro
generates the only `impl HirpdagStruct` for each type, so a user could not
override that method, and the flags were 0 on every node.

We decided to keep the flags and let each type set its own bits with an
argument to its attribute:

```rust
#[hirpdag(flags = var_flags)]
pub struct Var { pub name: String }

fn var_flags(_: &HirpdagStructVar) -> HirpdagMetaFlagType { HAS_VAR }
```

The function takes the type's data (`&HirpdagStructFoo` for a struct, `&Foo`
for an enum) and returns that node's own bits. The generated
`hirpdag_compute_meta` ORs them into the metadata folded from the fields, so a
node's flags are its own bits ORed with every child's. The call is spanned at
the path, so a function with the wrong signature is reported at the attribute.
`HirpdagStruct::hirpdag_flags` is removed, since nothing could override it.

`flags` joins `normalizer` and `root` in `TypeConfig`, the `#[hirpdag(...)]`
grammar from ADR 0010. It is the first argument there that takes a path rather
than being a bare flag, so `RawArg` now reads its value as an expression; a
string, a literal or a call given to `flags` is an error at the argument.
Unlike `normalizer` and `root`, `flags` also applies to enums: an enum is not
interned, but its `hirpdag_compute_meta` is folded into the struct that holds
it, so an enum's own bits reach that struct's flags.

## Considered options

- **An argument naming a function (chosen).** The function can be anywhere the
  module can see, and reads the data it is given, so a bit can depend on field
  values (a negative constant) as well as on the type.
- **A bare `flags` switch plus a method the user writes on the data struct,
  as `normalizer` does with `new`, rejected.** It forces the function to live
  in an `impl` of a generated type and gives it a fixed name, with nothing to
  gain over naming the function.
- **Delete the flags, rejected.** They are a planned feature; the problem was
  only that nothing could set them.

## Reading metadata

Users could only read a node's metadata through
`HirpdagComputeMeta::hirpdag_compute_meta`, which clones and is named as if it
computes something. Every generated reference type now has
`hirpdag_get_meta(&self) -> &HirpdagMeta`, which borrows the value cached at
intern time. It carries the `hirpdag_` prefix like the other methods the macro
adds to a user's type (`hirpdag_cmp_deep`), so it cannot clash with a method
the user defines, and it has the same name as the `HirpdagRef` method it
forwards to.

`HirpdagComputeMeta` stays. It is the machinery that folds a new node's fields
into its metadata at intern time, and for a struct being interned it does
compute. Its documentation now points readers at `hirpdag_get_meta()`.

## Costs

- Removing `HirpdagStruct::hirpdag_flags` changes a public trait, though no
  implementation outside the macro could exist to notice.
- The flag function is written against the generated data struct
  (`HirpdagStructFoo`), a name users otherwise rarely write.
- `hirpdag_get_meta` is longer to type than a bare `meta` would be; the prefix
  is the price of keeping generated names out of the user's namespace.
