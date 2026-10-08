---
status: accepted
---

# Split `hirpdag::base` into the user interface and `__private` macro support

`hirpdag::base` glob re-exported every public item of its submodules. Generated
code lives in the user's crate, so everything it calls had to be public, and
all of it appeared in `hirpdag::base` beside the items a user actually names.
A reader of the docs could not tell `HirpdagMeta` (read on every node) from
`HirpdagCollectCtx` (a step of the archive's collect phase). The binary header
helpers and `HIRPDAG_MAGIC` were public too, though nothing outside the
runtime crate used them.

We decided that `hirpdag::base` exports only what a user names:

- `HirpdagMeta` and its field types, read through `hirpdag_get_meta()`;
- `HirpdagLeaf`, and the `field` module that documents the field types;
- `HirpdagMemoize` and `HirpdagMemoizeMap`;
- `HirpdagRewritable`, the bound on `rewrite` in the generated rewrite traits;
- `HirpdagSerializeError` and `HirpdagDeserializeError`.

Everything else the generated code needs is re-exported from
`hirpdag::base::__private`, which is `#[doc(hidden)]` and not part of the
interface. The submodules are private, so `__private` is the only other path to
those items. Generated code names them by absolute path, so the
`use hirpdag::base::*;` it emits into the user's module brings in only the user
interface. Items used only inside the runtime crate are `pub(crate)`.

## Considered options

- **Keep one flat module and mark the macro items `#[doc(hidden)]` one by
  one.** Hides them from the docs but leaves them importable by glob, and every
  new item has to remember the attribute.
- **Move the macro items into a separate crate.** `hirpdag_derive` is a
  proc-macro crate and cannot export them, so this would be a fourth crate for
  no gain over a module.

## Consequences

- A user who named a macro-support item through `hirpdag::base` (for example
  `HirpdagRef` or `HirpdagComputeMeta`) must stop doing so. Metadata is read
  with `hirpdag_get_meta()`.
- `__private` can change with the generated code without that being an
  interface change.
