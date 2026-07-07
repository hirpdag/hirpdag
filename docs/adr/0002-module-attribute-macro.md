---
status: accepted
---

# Generate hirpdag code with a single module attribute macro

Hirpdag's code generation needs to see every data type in a module at once
(the `HirpdagRewriter` trait, the serialization archive, and the schema
fingerprint are all per-module aggregates over the types). The original
interface split this across attribute macro invocations — `#[hirpdag]` on
each type, then a trailing `#[hirpdag_end] pub struct HirpdagEndMarker;` —
which forced the two macros to communicate through a global registry
(`static DATA_TYPES: Mutex<Vec<...>>`) in the proc-macro process, drained by
each end marker. That contract silently depends on the compiler expanding
the attributes in source order. The order holds for attributes written
literally in a file, but not for attributes inside a macro-generated module
whose paths resolve through a glob import: there the invocations are
deferred during name resolution and re-expanded out of source order, so an
end marker can run before the type definitions of its own module and see an
empty (or a sibling module's) registry. This broke every attempt to factor
out the benchmark suite's repeated per-configuration modules.

We decided to replace the pair with one attribute on the module itself:

```rust
#[hirpdag_module]
mod datamodel {
    #[hirpdag]
    struct Node { children: Vec<Node> }
}
```

`#[hirpdag_module]` sees the whole module in a single invocation: it expands
the (now inert) `#[hirpdag]` type markers, passes other items through, and
appends the module-level code — no end marker, no global state, no
dependence on expansion order. The hash-consing configuration moves to the
attribute's arguments, including named presets
(`#[hirpdag_module(preset = "arc_hash_linear")]`), which lets the benchmark
suite stamp out its per-configuration modules with a ten-line
`macro_rules!`.

## Considered Options

- **Keep `#[hirpdag]`/`#[hirpdag_end]` and stamp configuration modules with
  `macro_rules!`.** Fails: the registry handoff breaks under the reordered
  expansion described above. Diagnosed empirically by tracing expansion
  order inside the derive macros; the failure is order-dependent, not (as
  first suspected) macro hygiene — proc-macro-generated items from
  macro-authored attribute tokens resolve fine.
- **Invoke the attributes by absolute path (`#[hirpdag::hirpdag_end]`) in
  the stamping macro.** Empirically restores source-order expansion (the
  paths resolve immediately, so nothing is deferred), but the fix works by
  making the scheduler happier, not by removing the assumption; a
  seemingly irrelevant refactor (glob vs. absolute path) already flipped
  the behavior once.
- **A function-like proc macro (`hirpdag_configurations!`) that expands
  types and module code itself.** Works (it was briefly the implementation)
  and proved the expand-everything-at-once approach, but it only fixed the
  benchmark stamping while leaving the fragile two-attribute interface and
  global registry in place for everyone else, plus a second public macro to
  document.
- **Per-type generation only, no module aggregate.** Would remove the need
  to see the whole module, but the rewriter trait, archive enum, and schema
  fingerprint are inherently whole-module artifacts; hirpdag's design
  requires the aggregate view.

## Consequences

- `#[hirpdag]` and `#[hirpdag_end]` are gone; this is a breaking interface
  change. Types must live in an inline `mod` (attribute macros cannot apply
  to a file's root), so file-scope users add a module and re-export
  (`use datamodel::*;`).
- The derive crate no longer holds cross-invocation state (`lazy_static`
  dependency dropped); each expansion is a pure function of its input.
- `macro_rules!` can now safely generate hirpdag modules (each
  `#[hirpdag_module]` invocation is self-contained), which is what the
  benchmark suite does; named presets live in `hirpdag_derive` and are
  usable by any caller.
- Struct fields may carry `pub`, since consuming code can now sit outside
  the defining module; generated constructors strip the visibility from
  their parameter lists.
- Generated code (including the default and preset configuration types)
  refers to the hirpdag crate by absolute paths, so modules need no
  `use hirpdag::*;` for the generated code — imports inside the module are
  only for the user's own code.
- The inner attribute form (`#![hirpdag_module]` inside the module or at
  file scope) is not possible: Rust rejects proc-macro inner attributes
  (E0658, rust-lang/rust#54726). A `compile_fail` doctest on the hirpdag
  crate pins this. File-scope usage therefore always goes through an inline
  `mod` plus re-export.
