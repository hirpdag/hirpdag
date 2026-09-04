---
status: accepted
---

# Derive generated names from the declared name verbatim, never by transforming its case

Every item `#[hirpdag_module]` generates is named after the type it belongs to.
A struct `Foo` gets `HirpdagStructFoo`, `HirpdagArchiveStructFoo`, `FooBuilder`,
`rewrite_Foo`, `cache_Foo`, an interning table static, and — when it is a
`#[hirpdag(root)]` — a field on `HirpdagArchiveRoots`. Two of those nine
families were built by transforming the declared name's case rather than
carrying it through:

- the table static was uppercased, `HIRPDAG_TABLE_FOO`;
- the roots field was snake-cased, `Foo` becoming `foo`.

Neither transform is injective. `Foo` and `FOO` in one hirpdag module both named
`HIRPDAG_TABLE_FOO`; `AB` and `A_b` both named the roots field `a_b`. Each
collision surfaced as a duplicate-definition error inside expanded code, where
the name the user has to reason about does not appear in their source.

We decided to build every generated name by prefixing or suffixing the declared
name verbatim: `HIRPDAG_TABLE_Foo` and `roots_Foo`. The mapping is then injective
in every family, because the prefix is fixed and the suffix is the declaration,
so no two declarations can collide and there is nothing left to validate. The
generated code carries `#[allow(non_upper_case_globals)]` on the static and
`#[allow(non_snake_case)]` on the two roots structs, in keeping with the
`#[allow(non_snake_case)]` the generated `rewrite_Foo` methods already carry.

The nine families now live in one module, `hirpdag_derive::names`, which is
where this decision is enforced and tested.

## Considered options

- **Prefix or suffix the declared name verbatim (chosen).** Injective by
  construction, so a collision is unrepresentable rather than diagnosed. Costs:
  generated names no longer follow Rust's casing conventions and need `allow`
  attributes, and the roots field rename is a breaking change to the JSON text
  format (see below).
- **Keep the transforms and detect collisions, rejected.** A `syn::Error` naming
  both declarations would give a better message than the duplicate-definition
  error, but it is a diagnosis of a problem we can instead not have. It also
  costs a validation pass that has to be kept correct as families are added.
- **Make the transforms injective, rejected.** Escaping (`Foo` → `foo`, `FOO` →
  `f_o_o`) restores injectivity while keeping conventional casing, but the
  escaping rule is then a second convention users have to learn in order to
  predict the name of their own roots field.
- **Leave it, rejected.** The collisions are unlikely — both triggers need type
  names that already warn under `non_camel_case_types` — but the failure is a
  compiler error about generated code, which is the worst place hirpdag can put
  one.

## Costs

The roots field name is part of the **JSON** text format: `HirpdagArchiveRootIndices`
derives serde's `Serialize`/`Deserialize`, so its field names appear literally in
the output. A JSON archive written before this change carries `"roots":{"foo":…}`
and needs `"roots":{"roots_Foo":…}` now. The binary format is unaffected, because
postcard is positional; the schema fingerprint is also unaffected, because a type's
definition string does not include its generated names — and the JSON format omits
the fingerprint by design, so nothing would have caught the change.

To keep that from being silent, `HirpdagArchiveRootIndices` gained
`deny_unknown_fields` alongside its existing `default`. Without it, an old archive
would deserialize successfully with every roots vector empty: serde ignores the
unknown `"foo"`, and `default` fills in an empty `roots_Foo`. With it, the old name
is an error that names the field. The two attributes compose — `default` governs
what may be omitted, `deny_unknown_fields` what may be added — so a root type whose
vector is empty can still be left out of hand-written JSON.
