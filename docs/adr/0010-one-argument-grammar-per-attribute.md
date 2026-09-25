---
status: accepted
amended-by: [0011-settable-metadata-flags]
---

# Give each attribute its own argument grammar, and reject what it does not use

ADR-0011 added `flags = path` to `#[hirpdag(...)]`; the rest of this stands.

`#[hirpdag_module(...)]` and `#[hirpdag(...)]` parsed the same argument list,
`HirpdagArgs`, into the same `HirpdagConfig`. Each read only part of it: the
module read the hash-consing types, and each type read `normalizer` and `root`.
Everything else was accepted and dropped. So

```rust
#[hirpdag_module(normalizer, root)]
mod m {
    #[hirpdag(preset = "leak_hash_linear", reference_type = "this is not a type")]
    struct Node { a: u32 }
}
```

compiled, and `Node::new` was still generated: none of the four arguments had
any effect. `#[hirpdag(normalizer)]` on an enum was dropped the same way.

We decided that each attribute has its own grammar and rejects an argument it
does not use, with a `syn::Error` at that argument:

- `#[hirpdag_module(...)]` parses into `ModuleConfig`: `preset`,
  `reference_type`, `reference_weak_type`, `table_type`, `tableshared_type`.
- `#[hirpdag(...)]` parses into `TypeConfig`: the flags `normalizer` and `root`.
  Both apply to structs only; on an enum each is an error at the flag.

An argument that belongs to the other attribute is reported as such ("`root` is
a `#[hirpdag(...)]` argument; put it on the struct"), since that is the likely
mistake. A type string must also parse as a Rust type, so a typo is reported at
the string rather than as type errors in the expansion, or as a macro panic for
a string that does not lex.

## Considered options

- **Two grammars, each rejecting the other's arguments (chosen).** The types
  say which arguments each attribute takes, and a misplaced argument cannot be
  silently ignored. Cost: code that compiled before, with arguments that had no
  effect, no longer compiles. That code was already wrong.
- **Allow type strings per type, rejected.** `#[hirpdag(preset = ...)]` could
  have been made to mean something. It cannot, cheaply: every type in a module
  shares the module's `ImplRef` / `ImplTableShared` aliases, and the rewrite and
  archive machinery is generated once per module against them.
- **Keep one grammar and warn on unused arguments, rejected.** Proc macros have
  no stable way to emit a warning, and a warning would still leave the argument
  doing nothing.
