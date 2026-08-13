# Techniques

Techniques for using Hirpdag to maximize effectiveness.

## Referential Transparency

[Referential Transparency @ Wikipedia](https://en.m.wikipedia.org/wiki/Referential_transparency)

Hirpdag objects should generally be designed to have referential transparency.

Objects which are identical should not have different meanings in a different context/environment.

## No Interior Mutability

Interior mutability (`Cell`, `RefCell`, `Mutex`, `RwLock`, `AtomicUsize`, or any other type that allows mutation through a shared reference) is **forbidden** inside Hirpdag data nodes.

This is not a stylistic preference — it is fundamental. A Hirpdag node's contents *are* its identity:

* **Hash consing keys on content.** A node is interned by hashing and comparing its fields. Mutating a field through interior mutability would silently change the node's hash and equality *after* it has been placed in the hash-consing table, corrupting the table's invariants. The node would be filed under a key that no longer describes it, breaking future lookups and deduplication.
* **Pointer equality depends on it.** Two nodes are equal iff they share an allocation, precisely because identical content is guaranteed to intern to one allocation. Mutable interior state would allow two "equal" pointers to diverge in meaning, defeating the core `O(1)` comparison.
* **Cached metadata would go stale.** `count`, `height`, and `flags` are computed once, bottom-up, at intern time. Mutating a node's contents afterward would invalidate this metadata with no mechanism to recompute it.
* **Memoization would return wrong answers.** Rewrite and analysis results are cached against a node's identity. If the underlying content could change, those cache entries would become incorrect — reintroducing exactly the cache-invalidation problem immutability was chosen to avoid.
* **Referential transparency would be lost.** A node whose observable value can change over time no longer means the same thing in every context.

In short: identity-defining data is fundamentally incompatible with interior mutability. Everything a Hirpdag node participates in — interning, equality, ordering, metadata, memoization — assumes its contents are fixed for its lifetime.

### Alternative: a side-table keyed by node reference

When you genuinely need mutable state associated with a node — annotations, analysis results, scratch state, external bookkeeping — keep that state *outside* the node in a side-table keyed by the node's reference:

```rust
// The node stays immutable; mutable data lives beside it.
let mut annotations: HashMap<MyNode, Annotation> = HashMap::new();
annotations.insert(node.clone(), Annotation::new());
```

Because nodes are hash-consed, a `HirpdagRef` is a stable, cheap, `O(1)`-hashable/comparable key (its creation ID gives a total order as well), which makes it an excellent map key. This pattern cleanly separates the two concerns:

* The node keeps its identity, hashing, deduplication, and cacheability intact.
* The mutable data has an explicit, owned lifetime that you control, rather than being smuggled inside a value that the rest of the system assumes is frozen.

If the state should itself participate in hash consing (i.e. it is part of the value's identity), it does not belong in a side-table — model it as additional immutable fields and construct a new node instead.

## Common Normalization

Hirpdag objects should apply normalization to increase the effectiveness of hashconsing.

Normalization is important for pointer inequality to correspond with semantic inequality.
Fast pointer equality based comparisons is one of the key features hash consing provides.
Without good normalization, deeper comparisons are needed and the pointer equality benefit of hashconsing is lost.

### Order Normalization

`y+x`
`x+y`

Sort commutative operands. Prefer flatter expression trees to make this easier.

### Semantic Normalization

`x+x`
`2*x`

## Structuring for Persistence and Normalization

The structure of Hirpdag objects can have a big impact on the effectiveness of normalization and persistence.

### Prefer Flatter Structures

Consider:
* `A=a+b+d+e`

As a binary tree (before normalization) it might look like:
* `a=b=sum(a, sum(b, sum(d, e)))`
* `a=b=sum(sum(a, b), sum(d, e))`
* `a=b=sum(sum(sum(a, b), d), e)`

With a binary tree representation, the first question is: which of these semantically equivalent structures is the normalized form?

Consider:
* `B=a+d+b+e`

As a binary tree (before normalization) it might look like:
* `B=Sum(Sum(a, d), Sum(b, e))`

`B` is semantically equivalent to `A`, and should normalize to the same thing.
In this case, the order of the operands needs to change.

With a binary tree representation, the second question is: what is necessary to normalized the operand order?
Traversing the existing tree is necessary to gather these operands for sorting.
Performance wise, this is similar to traversing a linked list (i.e: bad).

As a n-ary tree:
  A=Sum(a, b, d, e)

More contiguous. Easier to sort. Easier to traverse. Easier to construct. Easier to normalize (just sort).

When used as a persistent data structure, this means changing one n-ary Sum object rather than several binary Sum objects.

In general, if a Hirpdag object can refer to other Hirpdag objects of the same type and ordering is not important,
this suggests you should consider changing their structure to combine them into one flattened Hirpdag object.

### Not too big, not too small

If a Hirpdag object has too much information, deduplication opportunities will be unlikely.

If a Hirpdag object has too little information, encoding a useful piece of information will require many objects.
This will have a negative impact on performance due to:
* Worse memory access patterns chasing pointers (like a linked list).
* More time spent allocating/deallocating.

Consider which fields may be large (e.g. a vector field may grow large).
Consider which fields will need to mutate together.

### Encoding Graphs

If the graph to store is acyclic, it could be directly constructed.

If the nodes or edges carry some information, they should likely be separate nodes.
This makes the graph better for persistence.

An [adjacency list](https://en.m.wikipedia.org/wiki/Adjacency_list) or [edge list](https://en.m.wikipedia.org/wiki/Edge_list) can encode the graph structure itself.

```
type NodeIndex = u32;

struct Node {
  name: String,
}

struct Graph {
  nodes: Vec<Node>,
  edges: Vec<(NodeIndex, NodeIndex)>, // Sorted
}
```

