# TODO

### Hashconsing optimization experiments

- [P0] Optimize HirpdagRef implemetation of Ord:
  - Should not need to do a deep cmp.
  - Consider creation timestamp.

- [P0] Optimize rewrite code:
  - If no changes we can copy input reference instead of hashconsing to reproduce it.
  - Minimize reference count operations.

- [P0] Make memory allocation for hirpdag objects contiguous.
  - Can this be done by using https://github.com/rkyv/rkyv to help serialization?

- [P1] Optimize for small objects:
  - Just pack data into the handle type if less than 128bytes.
  - Warning if Hirpdag attribute is added to a struct which seems too small to benefit.

- [P1] Make hirpdag objects use read-only optimized datastructures.
  - e.g. If object contains a hashmap make it use perfect hashing (e.g https://lib.rs/crates/phf)
  - Use flat datastructures
  - Should Hirpdag automatically change a field to a more optimized type for you? Or just warn or something?

- [P1] Experiment with a single std::any::Any based map for Hashconsing and Rewrite caches.
  - Currently generate a separate map for each type, because this was the easiest thing to do.
  - Hash tables need some free space overhead to operate efficiently.
    Combining all of these hash tables into one may need less overall empty space overhead.

- [P2] Caching for `ref.hirpdag_compute_meta()`. e.g. `ref.hirpdag_compute_meta(meta_cache)`.

- [P2] Experiment with doing deallocation work on another thread when RC==0.

### Experiment with more refcounting implementations

- [P1] Locate many reference counts contiguously, separate from the data.
  - Cachlines holding data can be shared read only and never dirtied (to other CPU cores)
    due to ref count updates.
  - https://users.rust-lang.org/t/why-does-arc-use-one-contiguous-allocation-for-data-and-counters/113319
  - https://ddanilov.me/shared-ptr-is-evil/
  - Try padding ref counts to one (pair, strong and weak) per cacheline. Look for performance/space trade-off.

- [P3] Thread local reference counts, periodically flush back to main counter.
  - Is this even possible? Is it good?
  - Possible design: `thread_local! { static THREAD_RC: RefCell<HashMap<HirpdagRef, u32>> = ...; }`
    - If counter in value of map reaches 0, remove the map entry (release the Rc held as key)
  - Would we need to allow thread local counts to go negative (e.g. thread recieves many Rc handles to drop)?

### More benchmarks

- [P2] More benchmark programs.

- [P3] Perf measuring cache-misses, branch-misses, etc. instead of only execution time.

### Code cleanup

- [P0] Builder API?
  - Supporting modifications on an existing node.
  - Avoid hashconsing construction until once at the end of the builder.
  - Try to leverage an existing crate:
    - https://github.com/rust-unofficial/patterns/blob/master/patterns/builder.md
    - https://doc.rust-lang.org/1.0.0/style/ownership/builders.html
    - https://docs.rs/derive\_builder/0.9.0/derive\_builder/

- [P1] General refactoring and cleanup:
  - Try to reduce use of generic and builders for hashconsing implementations.

- [P1] Address current code duplication in benchmarks for running the same benchmark
  with different hashconsing implementations selected.

- [P3] Find a better solution than the `#[hirpdag_end] pub struct HirpdagEndMarker;` hack.

### Features

- [P0] Serialization/deserialization

- [P1] Visitor traversal code

- [P2] Warning if Hirpdag is used in a probably-wrong way
  - Adding the hirpdag attribute to struct which only contains 1 field and it is a hirpdag ref.

- [P3] IPC (hirpdag objects in shared memory, used from multiple processes)
  - Maybe special support for producer/consumer pattern? - only one process creates hirpdag objects
