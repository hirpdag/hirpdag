//! Concurrent hash-consing table backed by [`flurry::HashMap`].
//!
//! Flurry is a Rust port of Java's `ConcurrentHashMap`: a lock-free hash map
//! that uses atomic operations plus epoch-based reclamation (via `crossbeam`)
//! rather than per-bucket locks. Reads never block and writes lock only the
//! single bin they touch.
//!
//! Like the other concurrent wrappers here, the interned mapping is stored
//! directly in the map (there is no inner single-threaded [`ThreadUnsafeTable`](crate::ThreadUnsafeTable)),
//! and strong references are retained (no weak-reference GC of unreferenced
//! nodes).
//!
//! Because flurry's bins can promote to balanced trees on hash collisions, the
//! key type must be [`Ord`] in addition to [`Hash`](std::hash::Hash) + `Eq`.

use crate::reference::*;
use crate::table::*;
use flurry::HashMap;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

pub struct TableSharedFlurry<D, R, HB = DefaultHasher>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    R: Reference<D> + Clone + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    map: HashMap<D, R, HB>,
}

impl<D, R, HB> Default for TableSharedFlurry<D, R, HB>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    R: Reference<D> + Clone + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn default() -> Self {
        Self::with_hasher(HB::default())
    }
}

impl<D, R, HB> TableSharedFlurry<D, R, HB>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    R: Reference<D> + Clone + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    pub fn with_hasher(hash_builder: HB) -> Self {
        Self {
            map: HashMap::with_hasher(hash_builder),
        }
    }
}

impl<D, R, HB> Table<D, R> for TableSharedFlurry<D, R, HB>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    R: Reference<D> + Clone + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn get(&self, data: &D) -> Option<R> {
        let guard = self.map.guard();
        self.map.get(data, &guard).map(R::strong_clone)
    }

    fn get_or_insert<CF>(&self, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        let guard = self.map.guard();
        if let Some(r) = self.map.get(&data, &guard) {
            return R::strong_clone(r);
        }
        creation_meta(&mut data);
        let obj = R::new(data);
        let key = R::strong_deref(&obj).clone();
        // `try_insert` is atomic: if another thread interned a structurally equal
        // node in the race window, it returns that existing value and we discard
        // the one we just built.
        match self.map.try_insert(key, R::strong_clone(&obj), &guard) {
            Ok(_) => obj,
            Err(e) => R::strong_clone(e.current),
        }
    }
}

pub struct BuildTableSharedFlurry<D, R, HB> {
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R, HB> BuildTableSharedFlurry<D, R, HB>
where
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

impl<D, R, HB> Clone for BuildTableSharedFlurry<D, R, HB>
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

impl<D, R, HB> Default for BuildTableSharedFlurry<D, R, HB>
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

impl<D, R, HB> BuildTable<D, R> for BuildTableSharedFlurry<D, R, HB>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    R: Reference<D> + Clone + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    type TableSharedType = TableSharedFlurry<D, R, HB>;

    fn build_tableshared(&self) -> TableSharedFlurry<D, R, HB> {
        TableSharedFlurry::with_hasher(self.hash_builder.clone())
    }
}
