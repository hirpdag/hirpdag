# Hirpdag

Data structures which are:

* **H**ash Consed
* **I**mmutable
* **R**eference Counted
* **P**ersistent
* **D**irected **A**cyclic **G**raph

These data structures are also Merkle Trees, and amenable to DAG rewriting.

## Strengths

#### Time

Memoization, Persistence, Incremental processing, Easy Caching (no invalidation)

#### Space

Hash Consing can massively reduce space.

Repeatedly referencing the same content is a common way of creating a large amount of complexity from a small amount of source material.

## Synergies

### Immutability and Directed Acyclic Graphs

Immutability naturally ensures graph construction produces Directed Acyclic Graphs.
We cannot know an object's address in advance, so a mutation would be necessary to create a cycle.

### Immutability and Reference Counting

One of the weaknesses of reference counting is that it cannot reclaim reference cycles.
Immutability makes it impossible to construct a reference cycle, which prevents this issue.

### Immutability and Memoization

One of the weaknesses of Memoization is cache invalidation.
Data which is immutable and referentially transparent (no semantic variation with context) avoids the problem of cache invalidation.
Information of cached relations typically remains valid.
