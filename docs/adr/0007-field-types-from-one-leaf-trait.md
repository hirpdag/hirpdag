---
status: accepted
---

# Define leaf field types through one marker trait, with the containers beside it

The code `#[hirpdag_module]` generates visits every field of a data type four
ways: `HirpdagComputeMeta` when the node is interned, `HirpdagRewritable` in
`default_rewrite`, `HirpdagCollect` and `HirpdagArchived` when archiving. A
field type needs all four. Each trait carried its own copy of the same impl
groups (a blanket over an `IsNumber` marker, `String`, `Option<T>`, `Vec<T>`)
in three files, with `IsNumber` in a fourth.

That left the set of field types written down four times, and small: the
integers and floats, `String`, `Option` and `Vec`. `bool` and `char` were not
field types, and a user crate could not make them so, because the orphan rule
forbids implementing a hirpdag trait for a `std` type outside hirpdag. A user's
own leaf type could get in only by implementing `IsNumber`, which was
undocumented, named for numbers, described as sealed while it was not, and not
enough for serialization without `Copy`. The floats were listed but unusable:
data types derive `Eq`, `Hash` and `Ord`, which `f32` and `f64` do not have. An
unsupported field produced dozens of errors about `IsNumber` bounds, pointing
at the attribute.

We decided:

- **One marker trait for leaves.** `HirpdagLeaf`, in `hirpdag::base::field`,
  means "holds no hirpdag node". Its supertraits are what the generated code asks
  of a field (`Clone`, `Debug`, `Hash`, `Eq`, `Ord`, serde's `Serialize` and
  `DeserializeOwned`), and one blanket impl per field trait gives every leaf all
  four. The archive impl clones rather than copying, so `String` is an ordinary
  leaf. Leaves are the integers, `bool`, `char`, `String` and `()`; a crate adds
  its own with `impl HirpdagLeaf for TheType {}`.
- **The containers beside it.** `Option`, `Vec` and tuples of two to four
  elements, each with its four impls, in the same file. A new container is one
  edit in one place.
- **Errors at the field.** `HirpdagLeaf` and `HirpdagComputeMeta` carry a
  `#[diagnostic::on_unimplemented]` message naming the type and the fix. The
  macro emits a check per field, spanned to the field's type; it walks into
  `Option`, `Vec` and tuples syntactically and checks the element types, because
  a check on `Option<(bool, Colour)>` as a whole matches both the leaf and the
  `Option` impl and the compiler then reports the outer type rather than
  `Colour`. The generated meta code calls the trait function by path rather
  than as a method, so a bad field is a trait error rather than "no method
  named".

## Considered options

- **Blanket impls over one leaf trait (chosen).** One line per leaf type, in
  hirpdag or in a user crate. Costs: `Box` cannot be a container (below), and
  a user's leaf type needs the serde derives.
- **A `hirpdag_leaf!(T)` macro stamping the four impls per type, no blanket.**
  No coherence constraint, so `Box<T>` could be a container too. Rejected: a
  second public macro to document, four impls in every user crate instead of
  one line, and `Box` is not worth it (below).
- **Keep `IsNumber` and add `bool`, `char` to it.** The smallest change, and it
  fixes the two missing primitives. Rejected: the four copies of the impl
  groups stay, a user's leaf type still goes through a trait named for numbers,
  and the errors still talk about `IsNumber`.
- **Keep `IsNumber` as an alias of `HirpdagLeaf`.** Rejected: any existing
  `impl IsNumber` has to meet the new supertraits and so breaks anyway, and the
  alias would keep the misleading name in circulation.

## Consequences

- **`Box` is not a field type.** `Box` is `#[fundamental]`: a downstream crate
  may implement `HirpdagLeaf` for `Box<TheirType>`, so an impl of a field trait
  for `Box<T>` overlaps the leaf blanket impl, and the compiler rejects it
  (E0119, "downstream crates may implement trait `HirpdagLeaf` for type
  `Box<_>`"). A node reference is already a pointer, so a boxed field has
  nothing to add. The same applies to a generic impl for `&T`.
- **Breaking:** `hirpdag::base::basic_traits` and `IsNumber` are removed. A crate
  that implemented `IsNumber` for its own type implements `HirpdagLeaf` instead,
  with the supertraits. `f32` and `f64` are no longer listed as leaves; they were
  never usable as fields.
- The archive format is unchanged for every module that compiled before: the
  impls moved, and none of them changed what they write. Adding `bool`, `char`
  and tuples introduces no new encoding beyond serde's own.
- The error count for an unsupported field does not drop: the generated code
  still fails where it uses the field. What changes is that one of the errors
  is at the offending type, in words, with the fix.
- `hirpdag::base::field` has unit tests for the leaves and containers against
  stand-in drivers and contexts, and `test_suite/tests/field_types.rs` checks
  nodes nested in containers through interning, metadata, both drivers and
  both archive formats.
