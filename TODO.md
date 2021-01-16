# TODO

- Optimize hashconsing implementations.

- Optimize HirpdagRef implemetation of Ord:
  - Should not need to do a deep cmp.
  - Consider creation timestamp.

- Optimize for small objects:
  - Just pack data into the handle type if less than 128bytes.
  - Warning if Hirpdag attribute is added to a struct which seems too small to benefit.

- Optimize rewrite code:
  - If no changes we can copy input reference instead of hashconsing to reproduce it.
  - Minimize reference count operations.

- Experiment with a single std::any::Any based map for Hashconsing and Rewrite caches.
  - Currently generate a separate map for each type, because this was the easiest thing to do.
  - Hash tables need some free space overhead to operate efficiently.
    Combining all of these hash tables into one may need less overall empty space overhead.

- General refactoring and cleanup:
  - Try to reduce use of generic and builders for hashconsing implementations.

- More hashconsing implementations:
  - Locate many reference counts contiguously, separate from the data.
    - Cachlines holding data can be shared read only and never dirtied (to other CPU cores)
      due to ref count updates.
  - Thread local reference counts, periodically flush back to main counter.
    - Allow thread local counts to go negative?

- More benchmarks.

- Address current code duplication in benchmarks for running the same benchmark
  with different hashconsing implementations selected.

- Builder pattern?
  - Supporting modifications on an existing node.
  - Avoid hashconsing construction until once at the end of the builder.
  - https://github.com/rust-unofficial/patterns/blob/master/patterns/builder.md
  - https://doc.rust-lang.org/1.0.0/style/ownership/builders.html
  - https://docs.rs/derive\_builder/0.9.0/derive\_builder/

- Visitor traversal code.
