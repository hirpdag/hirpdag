---
status: accepted
---

# Keep one preset roster, and drive every caller from it with a proc macro

The set of hash-consing presets was written out six times:

- `PRESETS` and `preset_types` in `hirpdag_derive/src/config.rs`,
- `hirpdag_test_configs!` in `test_suite/tests/support/mod.rs`,
- `hirpdag_each_config!` in `test_suite/benches/support/mod.rs`,
- and `CORE_CONFIGS` / `THIRD_PARTY_CONFIGS` beside it, the runtime lists
  `HIRPDAG_BENCH_SCOPE` is validated against.

Three of those carried a comment saying the lists "must stay in sync". Adding
a preset meant six edits, and forgetting one lost its tests, or its benchmark,
or the ability to name it in a scope, with nothing to say so. `PRESETS` had
already drifted into being decorative: it fed the unknown-preset error message
while `preset_types` decided what was actually valid.

We collapsed all six into one roster, `hirpdag_derive::presets::PRESETS` — one
`const` entry per preset carrying the types it selects, its display label, and
whether it needs the `third-party-tables` feature — and gave the crate two
function-like proc macros to reach it:

- `hirpdag_for_each_preset!(callback, payload…)` expands a callback macro once
  per entry, passing `(module ident, name, label)` ahead of the payload, with
  the gated entries under a `#[cfg]`. It is what stamps out a module, a test or
  a benchmark registration per preset.
- `hirpdag_preset_names!(core | third_party | all)` expands to a `&[&str]` of
  names, for code that needs the roster as *runtime* data rather than as
  expansions — validating a preset named in an environment variable, say.

Both were needed: a macro that emits one item per preset cannot also produce a
single array literal, and a crate that only had the first would still have had
to write the scope-validation lists out by hand. `test_suite`'s macros keep
their own public arms and contracts; each gains a single-arm `_one` callback,
and its list of presets becomes one driver invocation.

## Why a proc macro, and not shared data

A `proc-macro = true` crate can export nothing but proc macros: no `const`, no
`macro_rules!`, no `pub fn`. `hirpdag_derive` is such a crate, and the
dependency graph runs `test_suite -> hirpdag -> hirpdag_derive`, so nothing can
flow back up. The roster therefore cannot simply be *read* by the test and
benchmark suites; it has to be *driven* by something the proc-macro crate is
allowed to export. That constraint is invisible from the resulting code, which
is why it is recorded here: the natural question on reading `presets.rs` is
"why isn't the list just shared as a `const`?", and the answer is that the
crate it lives in cannot export one.

## Considered options

- **A function-like proc macro driving callback macros (chosen).** The roster
  stays in the crate that already decides whether a preset name is valid; no
  new crate; the callbacks keep the `@one`-style arms the suites already had,
  now as single-arm macros with no dispatch. Costs: the driver's `#[cfg]` is
  emitted into the calling crate, so a crate driving the roster must declare a
  `third-party-tables` feature mirroring hirpdag's; and the payload passes
  through a proc macro as opaque tokens, so `expr` fragments make a round trip
  through a `None`-delimited group.
- **A fourth crate, `hirpdag_presets`, rejected.** Plain Rust, with the roster
  readable as ordinary data from both `hirpdag_derive` and `hirpdag`, and
  `preset_types` generated from it. Rejected as too much ceremony for around
  sixty lines: another crate to version, publish and keep in lockstep with the
  other three, to avoid one proc macro.
- **`include!` of a shared source file, rejected.** Sharing `presets.rs` by
  path between crates breaks `cargo package`, which only includes files inside
  the package directory.
- **Keeping the lists and adding a guard test, rejected.** Exposing the names
  and asserting the suites' lists match them would catch drift, but the six
  lists would remain six lists; the drift would be reported rather than made
  impossible. (The names are exposed anyway, by
  `hirpdag_preset_names!` — but as the lists themselves, not as something to
  check them against.)
- **Deriving the label from the name, rejected.** It would let the roster carry
  one field fewer, but no snake-to-camel transform produces `SepPadHashLinear`,
  `SepU32HashLinear`, `ArcTovWeakTable`, `ArcDashMap`, `ArcSkipMap` or
  `ArcArcSwap` from their names — six of twelve. This is the non-injectivity
  ADR-0006 already ruled on for generated names, so the roster stores labels
  and derives only the module identifier, which *is* the name.

## Consequences

- Adding a preset is one entry. Every caller that asks for "all presets" gets
  it: the macro's own validation and error message, the test matrix, the
  benchmark modules, the labels criterion reports, and the names
  `HIRPDAG_BENCH_SCOPE` accepts.
- The roster is a table you can read, rather than a `match` whose arms are the
  authority and an array that only feeds an error string.
- Neither macro is public API. Both reach
  users only through the existing `#[doc(hidden)] pub use hirpdag_derive::*;`
  glob, so the set of presets stays free to change as experiments come and go
  — `evmap` was already removed once. A user wanting to benchmark their own
  types across backends must still write their own list; if that turns out to
  be a real need, promoting the driver is a later, separate decision.
- The `#[cfg(feature = "third-party-tables")]` the driver emits is evaluated in
  the calling crate. Both in-tree callers declare a feature of that name. We
  deliberately did **not** extend the same assumption to generated user
  modules: emitting a `compile_error!` guard when a gated preset is selected
  would fire on a correctly-configured downstream crate that enables
  `hirpdag/third-party-tables` without declaring a feature of its own. A better
  diagnostic for that case needs a mechanism that can see a dependency's
  features, which a proc macro cannot.
- Because the roster now exists as data, it is testable on its own: the crate
  asserts that names are unique and identifier-shaped, that the default preset
  is not gated, that concurrent presets are gated and use a `Send + Sync`
  reference, that each preset's `ImplTable` alias matches whether its shared
  table needs one, that the driver emits one invocation per preset with the
  five gated ones under a `#[cfg]`, and that the `core` and `third_party` name
  lists partition the roster — which is what makes validating a user-supplied
  preset name against the pair of them complete.
