# Implementation

## Hashing

Want stable hashing.
Do not hash on pointer values.

## Hashconsing

### Weak references

#### Intrusive counters with weak count
Reference counts in same allocation as data. When strong ref count hits zero, destroy the data.
Until the weak ref count hits zero, the object remains to indicate that there is zero strong refs so weak refs cannot access data.

Consider adding indirection to the data if the data is large and weak pointers may survive a long time.

Good catch coherency, fewer allocations.

#### Separate counters with weak count
Reference counts in a separate allocation to the data. When strong ref count hits zero the allocation for the data can be freed.
The allocation for the counts remains to indicate the pointer is now invalid until the weak ref count also hits zero.

Reference objects have two pointers, one to the counters and one to the object. Strong refs can access the data pointer directly.
Weak refs must access the counters first and check strong is nonzero to upgrade.

Less cache coherency, more allocations.

#### Weak reference with intrusive linked list
All weak refs to an object form an intrusive linked list. The head of the linked list is stored with the reference counts.
As part of deleting an object, traverse all the weak references to that object via the intrusive linked list and set them to null.

### Reference Counting Optimizations

#### Cache Invalidation and Immutability

Updating a reference count will [modify a cache line](https://dl.acm.org/doi/10.1145/185009.185016) containing an object.
One of the strengths of immutable data, such as objects in Hirpdag, is that it can be shared between threads.
Modifying reference counts adjacent to objects will dirty unmodified shared cache lines.

Unmodified cache lines can remain in the [Shared state](https://en.m.wikipedia.org/wiki/MESI_protocol)

#### Coalescing and Elision

Coalescing reference count modifications together can reduce update operations by [50-90%](https://dl.acm.org/doi/10.1145/2426642.2259008).
Only the first increment and last decrement need to remain in the same place.
This kind of statement reodering and combining would need compiler support to be
[applied comprehensively](https://www.microsoft.com/en-us/research/uploads/prod/2020/11/perceus-tr-v1.pdf).

## Comparisons

### Pointer Equality

Compare object pointers for equality or inequality.

### Ordering

Cannot use the object pointers for ordering in many contexts, they are unstable and meaningless.

A deterministic ordering is desirable, which reflects a partial order of the global Hirpdag object DAG.

## Normalization

On construction.

## Rewriting

Apply rewrite rule to self.
Rewrite all children.
Construct a replacement for self, if anything changed.

## Memoization

Enabled by immutability and reference counting.

Hash map of reference to reference. Key is input, value is output.

## Serialization

Leaf objects should appear before other objects which use them.
The serialization ordering should be a valid bottom up partial order.
A post-order traversal will produce one possible linear ordering.

Good to mark reference handle roots in serialization format.

## Cache coherent datastructures

Immutable data will typically make non-contiguous data structures (such as tree-sets, tree-maps, and linked-lists) less appealing.

Cache line size is typically 64 bytes on most modern x86 systems.
```shell
$ getconf LEVEL1_DCACHE_LINESIZE
64
```

## Object Metadata

Each Hirpdag Object has a small amount of metadata attached to it. This includes:

* DAG Height
* DAG Count
* Content flags

The content flags are intended to provide a hint to avoid unnecessary traversals.

## Reference Count Update Elision

Incrementing and decrementing can be expensive and they may occur frequently. Because the count is shared so these updates must be atomic.

