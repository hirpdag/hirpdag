//! Concurrent hash-consing table backed by [`dashmap::DashMap`].
//!
//! DashMap is a bucket-striped concurrent hash map: it internally shards into
//! several independently locked `RwLock<HashMap>` regions, so threads touching
//! structurally different nodes rarely contend. This wrapper stores the mapping
//! directly in the concurrent map, so — unlike the `TableSharedMutex` /
//! `TableSharedSharded` wrappers — it does not delegate to an inner
//! single-threaded [`ThreadUnsafeTable`](crate::ThreadUnsafeTable) at all.
//!
//! Note on retention: entries hold a *strong* reference `R` to each interned
//! node, so nodes are retained for the lifetime of the table (no weak-reference
//! garbage collection of unreferenced nodes). This matches the `RefLeak` style
//! of experiment and keeps the concurrent map free of the extra liveness checks
//! that a weak table would require.

use crate::reference::*;
use crate::table::weak_holder::WeakEntryStrong;
use crate::table::*;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

pub struct TableSharedDashMap<D, V, HB = DefaultHasher>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    V: Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    map: DashMap<D, V, HB>,
}

impl<D, V, HB> Default for TableSharedDashMap<D, V, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    V: Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn default() -> Self {
        Self::with_hasher(HB::default())
    }
}

impl<D, V, HB> TableSharedDashMap<D, V, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    V: Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    pub fn with_hasher(hash_builder: HB) -> Self {
        Self {
            map: DashMap::with_hasher(hash_builder),
        }
    }
}

/// Private map plumbing, shared by the strong ([`Table`]) and weak
/// ([`NonPurgingTable`]) views so the raw `DashMap` calls live in one place.
impl<D, V, HB> TableSharedDashMap<D, V, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    V: Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn map_get(&self, data: &D) -> Option<V> {
        self.map.get(data).map(|r| r.value().clone())
    }

    fn map_retain<F>(&self, mut keep: F)
    where
        F: FnMut(&V) -> bool,
    {
        self.map.retain(|_k, v| keep(v));
    }

    fn map_len(&self) -> usize {
        self.map.len()
    }
}

/// Strong-reference hash-consing table: retains every interned node (no
/// weak-reference GC), matching the `RefLeak` style of experiment.
impl<D, R, WR, HB> Table<D, R, WR> for TableSharedDashMap<D, R, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D> + Clone + Send + Sync,
    WR: ReferenceWeak<D, R>,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn get(&self, data: &D) -> Option<R> {
        self.map_get(data)
    }

    fn get_or_insert<CF>(&self, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        // Fast path: a shared (read) lock on the bucket is enough for a hit.
        if let Some(r) = self.map_get(&data) {
            return r;
        }
        // Slow path: take the bucket entry (exclusive lock) so that creation and
        // insertion of a new node are atomic with respect to other writers —
        // no two threads can both insert a structurally equal node.
        let key = data.clone();
        match self.map.entry(key) {
            Entry::Occupied(e) => R::strong_clone(e.get()),
            Entry::Vacant(e) => {
                creation_meta(&mut data);
                let obj = R::new(data);
                e.insert(R::strong_clone(&obj));
                obj
            }
        }
    }

    #[cfg(feature = "reset-tables")]
    fn reset(&self) {
        self.map.clear();
    }
}

/// Weak-reference (non-purging) view: stores weak handles; the purge adapter
/// drives eviction. Weak downgrade / upgrade / liveness live here; the map
/// plumbing is shared with the strong view above.
impl<D, R, WR, HB> NonPurgingTable<D, R, WR>
    for TableSharedDashMap<D, WeakEntryStrong<D, R, WR>, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D>,
    WR: ReferenceWeak<D, R> + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn get(&self, data: &D) -> Option<R> {
        self.map_get(data).and_then(|entry| entry.upgrade())
    }

    fn get_or_insert<CF>(&self, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        // Lock-free fast path.
        if let Some(existing) = self.get(&data) {
            return existing;
        }
        // Slow path: the per-shard entry lock makes create-and-insert (and
        // dead-slot replacement) atomic against other writers of this key.
        match self.map.entry(data.clone()) {
            Entry::Occupied(mut e) => {
                if let Some(existing) = e.get().upgrade() {
                    return existing;
                }
                creation_meta(&mut data);
                let obj = R::new(data);
                *e.get_mut() = WeakEntryStrong::downgrade(&obj);
                obj
            }
            Entry::Vacant(e) => {
                creation_meta(&mut data);
                let obj = R::new(data);
                e.insert(WeakEntryStrong::downgrade(&obj));
                obj
            }
        }
    }

    fn retain_alive(&self) {
        self.map_retain(|entry| entry.is_alive());
    }

    fn len(&self) -> usize {
        self.map_len()
    }
}

pub struct BuildTableSharedDashMap<D, R, HB> {
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R, HB> BuildTableSharedDashMap<D, R, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D> + Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    pub fn with_hasher(hash_builder: HB) -> Self {
        Self {
            hash_builder,

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}

impl<D, R, HB> Clone for BuildTableSharedDashMap<D, R, HB>
where
    HB: Clone,
{
    fn clone(&self) -> Self {
        Self {
            hash_builder: self.hash_builder.clone(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}

impl<D, R, HB> Default for BuildTableSharedDashMap<D, R, HB>
where
    HB: Default,
{
    fn default() -> Self {
        Self {
            hash_builder: HB::default(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}

impl<D, R, WR, HB> BuildTable<D, R, WR> for BuildTableSharedDashMap<D, R, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D> + Clone + Send + Sync,
    WR: ReferenceWeak<D, R>,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    type TableSharedType = TableSharedDashMap<D, R, HB>;

    fn build_tableshared(&self) -> TableSharedDashMap<D, R, HB> {
        TableSharedDashMap::with_hasher(self.hash_builder.clone())
    }
}
