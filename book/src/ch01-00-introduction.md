# Introduction

Overview of relevant concepts.

## Hashing

[Hash Function @ Wikipedia](https://en.m.wikipedia.org/wiki/Hash_function)

Producing a signature of an objects based on its contents.

## Hash Consing

[Hash consing @ Wikipedia](https://en.m.wikipedia.org/wiki/Hash_consing)

[Constructor @ Wikipedia](https://en.m.wikipedia.org/wiki/Constructor_(object-oriented_programming))

A constructor is a function which runs automatically to setup an object.
The constructor must run to obtain a new object.

#### Strengths

* Pointer equality
* Saving space

#### Issues

* Overhead on construction.
* Hash table can become large.

## Immutability

[Immutability @ Wikipedia](https://en.m.wikipedia.org/wiki/Immutable_object)

[Copy on Write @ Wikipedia](https://en.m.wikipedia.org/wiki/Copy-on-write)

#### Strengths

* Great for shared data. Multiple threads can share the same data with no synchronization issues.

#### Issues

* Requires more copy on write, which can cause copy overhead.

## Reference Counting

[Reference Counting @ Wikipedia](https://en.m.wikipedia.org/wiki/Reference_counting)

#### Strengths

* Shared ownership of data.
* Prevents use-after-free. A reference is necessary to use the data, and the data will be there as long as at least one reference exists.
* References are cheap to pass and copy.

#### Issues

* Cannot reclaim reference cycles.
* Overhead for incrementing/decrementing reference counts, which must be atomic because the count is shared.
* Necessary indirection, even when the data is small. This may decrease performance by adding unprefetched random memory accesses.

## Persistent Data Structures

[Persistent Data Structures @ Wikipedia](https://en.m.wikipedia.org/wiki/Persistent_data_structure)

#### Strengths

* Avoid storing duplicate information.
* Can make copies and updates faster, by reducing copying of data.

## Directed Acyclic Graph

[Directed Acyclic Graph @ Wikipedia](https://en.m.wikipedia.org/wiki/Directed_acyclic_graph)

Directed graph is a structure composed on nodes/vertices connected by edges with a direction.

#### Strengths

* Clearer ownership compared to a graph with cycles

## Merkle Tree

[Merkle Tree @ Wikipedia](https://en.m.wikipedia.org/wiki/Merkle_tree)

#### Strengths

* Integrity

#### Issues

* Overhead of performing hashing and storing hashes
* Requires data to be immutable
* Requires more copy on write, which can cause copy overhead.

## Rewriting

[Rewriting @ Wikipedia](https://en.m.wikipedia.org/wiki/Rewriting)

Relevant search terms: "Term rewriting", "DAG rewriting", "Rewrite rules" (results for this one are typically overwhelmed by URL rewrite rules for web servers).

#### Strengths

* Systematic modification of structure data

## Memoization

[Memoization @ Wikipedia](https://en.m.wikipedia.org/wiki/Memoization)

#### Strengths

* Avoiding redundant work

#### Issues

* Cache invalidation problem
