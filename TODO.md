# TODO

### Hashconsing optimization

- Optimize HirpdagRef implemetation of Ord:
  - Should not need to do a deep cmp.
  - Consider creation timestamp.

- Make memory allocation for hirpdag objects contiguous.

- Optimize for small objects:
  - Just pack data into the handle type if less than 128bytes.
  - Warning if Hirpdag attribute is added to a struct which seems too small to benefit.

- Make hirpdag objects use read-only optimized datastructures.
  - e.g. If object contains a hashmap make it use perfect hashing (e.g https://lib.rs/crates/phf)
  - Use flat datastructures
  - Should Hirpdag automatically change a field to a more optimized type for you? Or just warn or something?

- Experiment with a single std::any::Any based map for Hashconsing and Rewrite caches.
  - Currently generate a separate map for each type, because this was the easiest thing to do.
  - Hash tables need some free space overhead to operate efficiently.
    Combining all of these hash tables into one may need less overall empty space overhead.

- Optimize rewrite code:
  - If no changes we can copy input reference instead of hashconsing to reproduce it.
  - Minimize reference count operations.

- Experiment with doing deallocation work on another thread when RC==0.

### Experiment with more refcounting implementations

- Locate many reference counts contiguously, separate from the data.
  - Cachlines holding data can be shared read only and never dirtied (to other CPU cores)
    due to ref count updates.
  - https://users.rust-lang.org/t/why-does-arc-use-one-contiguous-allocation-for-data-and-counters/113319
  - https://ddanilov.me/shared-ptr-is-evil/
  - Try padding ref counts to one (pair, strong and weak) per cacheline.

- Thread local reference counts, periodically flush back to main counter.
  - Is this even possible? Is it good?
  - Allow thread local counts to go negative (e.g. thread recieves many Rc handles to drop)?

### More benchmarks

### Code cleanup

- General refactoring and cleanup:
  - Try to reduce use of generic and builders for hashconsing implementations.

- Address current code duplication in benchmarks for running the same benchmark
  with different hashconsing implementations selected.

- Find a better solution than the `#[hirpdag_end] pub struct HirpdagEndMarker;` hack.

- Builder pattern?
  - Supporting modifications on an existing node.
  - Avoid hashconsing construction until once at the end of the builder.
  - https://github.com/rust-unofficial/patterns/blob/master/patterns/builder.md
  - https://doc.rust-lang.org/1.0.0/style/ownership/builders.html
  - https://docs.rs/derive\_builder/0.9.0/derive\_builder/

### Features

- Visitor traversal code

- Serialization/deserialization

- IPC (hirpdag objects in shared memory, used from multiple processes)
  - Maybe special support for producer/consumer pattern? - only one process creates hirpdag objects
