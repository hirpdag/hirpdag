use crate::reference::*;

/// Single-threaded hash-consing table — the inner storage unit behind [`Table`].
///
/// Implementations vary in lookup strategy (linear scan, sorted binary search, hash map) and
/// eviction policy (weak references allow GC of unreferenced nodes).
pub trait ThreadUnsafeTable<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    /// Look up an already-interned value by precomputed hash and equality.
    ///
    /// Returns `None` if no structurally equal value is currently stored.
    fn get(&self, hash: u64, data: &D) -> Option<R>;

    /// Return an existing interned value or intern a fresh one.
    ///
    /// If `data` is not yet in the table, `creation_meta` is called on the new entry
    /// before it is stored — allowing metadata and creation IDs to be set atomically
    /// with insertion.
    fn get_or_insert<CF>(&mut self, hash: u64, data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D);

    /// Empty the table, discarding all interned entries.
    ///
    /// The default implementation is a no-op; backends that can cheaply drop
    /// their storage override it. See [`Table::reset`] for the semantics and
    /// caveats.
    #[cfg(feature = "reset-tables")]
    fn reset(&mut self) {}
}

/// Factory for constructing [`ThreadUnsafeTable`] instances.
///
/// Used by [`BuildTable`] implementations to create per-shard inner tables.
pub trait BuildThreadUnsafeTable<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    type ThreadUnsafeTable: ThreadUnsafeTable<D, R>;

    fn build_table(&self) -> Self::ThreadUnsafeTable;
}

pub struct BuildThreadUnsafeTableDefault<T>(std::marker::PhantomData<T>);

impl<D, R, T> BuildThreadUnsafeTable<D, R> for BuildThreadUnsafeTableDefault<T>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Default + ThreadUnsafeTable<D, R>,
{
    type ThreadUnsafeTable = T;

    fn build_table(&self) -> T {
        T::default()
    }
}

impl<T> Default for BuildThreadUnsafeTableDefault<T>
where
    T: Default,
{
    fn default() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<T> Clone for BuildThreadUnsafeTableDefault<T> {
    fn clone(&self) -> Self {
        Self(std::marker::PhantomData)
    }
}

/// Thread-safe hash-consing table.
///
/// Implementations choose how to serialize concurrent access. Some wrap one or more inner
/// single-threaded [`ThreadUnsafeTable`] instances behind a locking strategy (a single mutex, sharded
/// mutexes); others store the mapping directly in a concurrent collection (lock-free hash
/// maps, skip lists, RCU). The `hirpdag` macro selects the implementation via
/// `#[hirpdag(tableshared_type = "...")]`.
pub trait Table<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    /// Look up an already-interned value; returns `None` if not present.
    fn get(&self, data: &D) -> Option<R>;

    /// Return an existing interned value or intern a fresh one, thread-safely.
    ///
    /// `creation_meta` is called exactly once if a new entry is inserted.
    fn get_or_insert<CF>(&self, data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D);

    /// Empty the table, discarding all interned entries, so that subsequent
    /// lookups behave as if nothing had ever been interned.
    ///
    /// This is done in place through the table's existing interior mutability,
    /// so the lookup/insert hot path is unaffected. The default implementation
    /// is a no-op; backends override it where they can cheaply drop their
    /// storage.
    ///
    /// # Caveat
    ///
    /// Resetting breaks the hash-consing invariant for any references interned
    /// *before* the reset: a structurally equal value interned afterwards will
    /// be a distinct allocation and will not compare pointer-equal to the old
    /// one. Only safe to call when such references are not relied upon (e.g.
    /// between benchmark iterations that have dropped all their nodes).
    #[cfg(feature = "reset-tables")]
    fn reset(&self) {}
}

/// Factory for constructing [`Table`] instances.
///
/// The default implementation calls [`BuildThreadUnsafeTable`] for each shard.
pub trait BuildTable<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    type TableSharedType: Table<D, R>;

    fn build_tableshared(&self) -> Self::TableSharedType;
}

pub struct BuildTableDefault<TS>(std::marker::PhantomData<TS>);

impl<D, R, TS> BuildTable<D, R> for BuildTableDefault<TS>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    TS: Table<D, R> + Default,
{
    type TableSharedType = TS;

    fn build_tableshared(&self) -> TS {
        TS::default()
    }
}

// Table-support helper (cached-hash weak entry for the vector-backed tables).
mod weak_entry;

// ThreadUnsafeTable implementations (single-threaded; weak-reference eviction).
pub(crate) mod hashmap_fallback_threadunsafe;
pub(crate) mod vec_linear_threadunsafe;
pub(crate) mod vec_sorted_threadunsafe;

// Table adapters connecting a ThreadUnsafeTable to the thread-safe interface.
pub(crate) mod shared_mutex;
pub(crate) mod shared_sharded;

// Table backends built on third-party collection crates, behind the opt-in
// `third-party-tables` feature. `tov_weak_table_threadunsafe` is an inner
// ThreadUnsafeTable (over the `weak-table` crate); the `*_strong` backends store
// the interned mapping directly in a concurrent collection.
#[cfg(feature = "third-party-tables")]
pub(crate) mod arcswap_strong;
#[cfg(feature = "third-party-tables")]
pub(crate) mod dashmap_strong;
#[cfg(feature = "third-party-tables")]
pub(crate) mod flurry_strong;
#[cfg(feature = "third-party-tables")]
pub(crate) mod skipmap_strong;
#[cfg(feature = "third-party-tables")]
pub(crate) mod tov_weak_table_threadunsafe;
