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
use crate::table::*;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

pub struct TableSharedDashMap<D, R, HB = DefaultHasher>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D> + Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    map: DashMap<D, R, HB>,
}

impl<D, R, HB> Default for TableSharedDashMap<D, R, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D> + Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn default() -> Self {
        Self::with_hasher(HB::default())
    }
}

impl<D, R, HB> TableSharedDashMap<D, R, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D> + Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    pub fn with_hasher(hash_builder: HB) -> Self {
        Self {
            map: DashMap::with_hasher(hash_builder),
        }
    }
}

impl<D, R, HB> Table<D, R> for TableSharedDashMap<D, R, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D> + Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn get(&self, data: &D) -> Option<R> {
        self.map.get(data).map(|r| R::strong_clone(r.value()))
    }

    fn get_or_insert<CF>(&self, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        // Fast path: a shared (read) lock on the bucket is enough for a hit.
        if let Some(r) = self.map.get(&data) {
            return R::strong_clone(r.value());
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

impl<D, R, HB> BuildTable<D, R> for BuildTableSharedDashMap<D, R, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D> + Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    type TableSharedType = TableSharedDashMap<D, R, HB>;

    fn build_tableshared(&self) -> TableSharedDashMap<D, R, HB> {
        TableSharedDashMap::with_hasher(self.hash_builder.clone())
    }
}
