# Techniques

Techniques for using Hirpdag to maximize effectiveness.

## Referential Transparency

[Referential Transparency @ Wikipedia](https://en.m.wikipedia.org/wiki/Referential_transparency)

Hirpdag objects should generally be designed to have referential transparency.

Objects which are identical should not have different meanings in a different context/environment.

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

