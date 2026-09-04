# Architecture Decision Records

One file per decision that shaped hirpdag, in the order they were made. Each
records what was decided, what else was considered and why it lost, and what
the decision costs.

An ADR is a record of a decision at a point in time, not a description of the
code as it stands. It is not rewritten when a later decision changes it: the
later ADR carries the reasoning, and the earlier one gains an `amended-by` in
its frontmatter and a line under its title saying what moved. For how a
subsystem works *now*, read the design doc for it.

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-serde-dag-aware-serialization.md) | serde as the trait layer for DAG-aware serialization, postcard for binary and serde_json for text | accepted, amended by 0005 |
| [0002](0002-module-attribute-macro.md) | Generate hirpdag code from one module attribute macro rather than per-type derives | accepted |
| [0003](0003-rewrite-driver.md) | Separate rewrite rules from the traversal that drives them | accepted |
| [0004](0004-archive-machinery-in-runtime-crate.md) | Keep the archive machinery in the runtime crate, behind `HirpdagArchive` | accepted, amended by 0005 |
| [0005](0005-archive-as-plain-data.md) | Encode references to node indices outside serde, so an archive carries no ambient state | accepted |
| [0006](0006-generated-names-from-the-declared-name.md) | Derive generated names from the declared name verbatim, never by transforming its case | accepted |

Design docs, which do describe the current code:

- [`../design/serialization.md`](../design/serialization.md) — the archive: layout,
  the archived form, both algorithms, and where the code lives.

`../TERMINOLOGY.md` defines the vocabulary these documents use.
