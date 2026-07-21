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
use crate::table::weak_holder::WeakEntryStrong;
use crate::table::*;
use flurry::HashMap;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

pub struct TableSharedFlurry<D, V, HB = DefaultHasher>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    V: Clone + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    map: HashMap<D, V, HB>,
}

impl<D, V, HB> Default for TableSharedFlurry<D, V, HB>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    V: Clone + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn default() -> Self {
        Self::with_hasher(HB::default())
    }
}

impl<D, V, HB> TableSharedFlurry<D, V, HB>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    V: Clone + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    pub fn with_hasher(hash_builder: HB) -> Self {
        Self {
            map: HashMap::with_hasher(hash_builder),
        }
    }
}

/// Private map plumbing, shared by the strong ([`Table`]) and weak
/// ([`NonPurgingTable`]) views so the raw `flurry` calls live in one place.
impl<D, V, HB> TableSharedFlurry<D, V, HB>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    V: Clone + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn map_get(&self, data: &D) -> Option<V> {
        let guard = self.map.guard();
        self.map.get(data, &guard).cloned()
    }

    fn map_retain<F>(&self, mut keep: F)
    where
        F: FnMut(&V) -> bool,
    {
        let guard = self.map.guard();
        self.map.retain(|_k, v| keep(v), &guard);
    }

    fn map_len(&self) -> usize {
        self.map.len()
    }
}

impl<D, R, WR, HB> Table<D, R, WR> for TableSharedFlurry<D, R, HB>
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

    #[cfg(feature = "reset-tables")]
    fn reset(&self) {
        let guard = self.map.guard();
        self.map.clear(&guard);
    }
}

/// Weak-reference (non-purging) view: stores weak handles; the purge adapter
/// drives eviction.
impl<D, R, WR, HB> NonPurgingTable<D, R, WR> for TableSharedFlurry<D, WeakEntryStrong<D, R, WR>, HB>
where
    D: std::hash::Hash
        + std::cmp::Eq
        + std::cmp::Ord
        + std::fmt::Debug
        + Clone
        + Send
        + Sync
        + 'static,
    R: Reference<D>,
    WR: ReferenceWeak<D, R> + Send + Sync + 'static,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn get(&self, data: &D) -> Option<R> {
        self.map_get(data).and_then(|entry| entry.upgrade())
    }

    fn get_or_insert<CF>(&self, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        let guard = self.map.guard();
        if let Some(existing) = self.map.get(&data, &guard).and_then(|e| e.upgrade()) {
            return existing;
        }
        creation_meta(&mut data);
        let obj = R::new(data);
        let key = R::strong_deref(&obj).clone();
        loop {
            match self
                .map
                .try_insert(key.clone(), WeakEntryStrong::downgrade(&obj), &guard)
            {
                // We installed our weak into a vacant slot: our node is canonical.
                Ok(_) => return obj,
                Err(e) => {
                    if let Some(existing) = e.current.upgrade() {
                        return existing;
                    }
                    // The slot holds a dead entry. `compute_if_present` runs
                    // under the bin lock, so removing it only if it is *still*
                    // dead is atomic; then retry the insert.
                    self.map.compute_if_present(
                        &key,
                        |_k, v| if v.is_alive() { Some(v.clone()) } else { None },
                        &guard,
                    );
                }
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

impl<D, R, WR, HB> BuildTable<D, R, WR> for BuildTableSharedFlurry<D, R, HB>
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
    WR: ReferenceWeak<D, R>,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    type TableSharedType = TableSharedFlurry<D, R, HB>;

    fn build_tableshared(&self) -> TableSharedFlurry<D, R, HB> {
        TableSharedFlurry::with_hasher(self.hash_builder.clone())
    }
}
