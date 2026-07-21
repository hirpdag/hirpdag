use crate::reference::*;

/// Single-threaded hash-consing table — the inner storage unit behind [`Table`].
///
/// Implementations vary in lookup strategy (linear scan, sorted binary search, hash map) and
/// eviction policy (weak references allow GC of unreferenced nodes).
///
/// The `WR` parameter names the [`ReferenceWeak`] type the table evicts against:
/// every inner table here stores weak references and can purge entries whose
/// referent has been dropped.
pub trait ThreadUnsafeTable<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
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
pub trait BuildThreadUnsafeTable<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
{
    type ThreadUnsafeTable: ThreadUnsafeTable<D, R, WR>;

    fn build_table(&self) -> Self::ThreadUnsafeTable;
}

pub struct BuildThreadUnsafeTableDefault<T>(std::marker::PhantomData<T>);

impl<D, R, WR, T> BuildThreadUnsafeTable<D, R, WR> for BuildThreadUnsafeTableDefault<T>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    T: Default + ThreadUnsafeTable<D, R, WR>,
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
///
/// The `WR` parameter names the [`ReferenceWeak`] type the table can purge
/// against. A concurrent backend that implements `Table` directly stores
/// **strong** references and never purges (retain-forever). To get purging
/// weak-key hash-consing from such a backend, use its [`NonPurgingTable`] view
/// and wrap it in [`TableAmortizedPurge`](crate::TableAmortizedPurge), which
/// adds amortized purging.
pub trait Table<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
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
pub trait BuildTable<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
{
    type TableSharedType: Table<D, R, WR>;

    fn build_tableshared(&self) -> Self::TableSharedType;
}

pub struct BuildTableDefault<TS>(std::marker::PhantomData<TS>);

impl<D, R, WR, TS> BuildTable<D, R, WR> for BuildTableDefault<TS>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    TS: Table<D, R, WR> + Default,
{
    type TableSharedType = TS;

    fn build_tableshared(&self) -> TS {
        TS::default()
    }
}

/// A concurrent hash-consing map that stores **weak** references but does *not*
/// purge dead entries on its own.
///
/// It takes a [`ReferenceWeak`] and, on its own, would leak entries whose
/// referent has been dropped; wrapping it in
/// [`TableAmortizedPurge`](crate::TableAmortizedPurge) adds amortized purging
/// and yields a purging [`Table`]. The name states the
/// invariant: it stores weak references but does no purging — that capability is
/// what a [`Table`] adds.
///
/// The concurrent third-party backends implement this directly (alongside a
/// direct strong-retention [`Table`] impl), sharing their map plumbing through
/// private inherent methods and adding only weak downgrade / upgrade / liveness
/// here.
///
/// Crucially, [`get_or_insert`](Self::get_or_insert) is implemented with each
/// backend's *own* concurrency primitive (dashmap's per-shard entry lock,
/// skipmap's `compare_insert`, flurry's `try_insert` / `compute_if_present`,
/// arc-swap's inherent writer serialization), so the purge adapter
/// needs no lock of its own.
pub trait NonPurgingTable<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
{
    /// Look up a key and upgrade the stored weak reference. Returns `None` if
    /// the key is absent or its referent has been dropped.
    fn get(&self, data: &D) -> Option<R>;

    /// Atomically return the existing live node for `data` or intern a fresh
    /// one. `creation_meta` runs at most once, only when this call performs the
    /// insertion. A dead weak entry left under the key is replaced. The
    /// atomicity comes from the backend's native concurrency, so concurrent
    /// callers never observe two live nodes for the same key.
    fn get_or_insert<CF>(&self, data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D);

    /// Drop every entry whose referent has died. This is the sweep the purge
    /// adapter drives amortized; the table itself never calls it.
    fn retain_alive(&self);

    /// The number of entries currently stored (including dead ones not yet
    /// swept by [`retain_alive`](Self::retain_alive)).
    fn len(&self) -> usize;

    /// Whether the table currently holds no entries.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// Table-support helper (cached-hash weak entry for the vector-backed tables).
mod weak_entry;

// A strongly-storable weak reference: a weak handle wrapped so it is
// `Clone + Hash + Eq` (and `Send + Sync` when the weak type is), so it can be
// held as the value in a backend's `NonPurgingTable` view.
pub(crate) mod weak_holder;

// Adapter turning a `NonPurgingTable` into a purging `Table`.
pub(crate) mod amortized_purge;

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
