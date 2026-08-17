//! Copy-on-write hash-consing table using [`arc_swap::ArcSwap`] (RCU pattern).
//!
//! This demonstrates the atomic-pointer-swap ("read-copy-update") approach: the
//! whole map lives behind a single `ArcSwap<HashMap>`. Readers atomically
//! `load()` the current `Arc` and never block or take a lock. Writers clone the
//! entire map, insert into the copy, and atomically `store()` the new `Arc`;
//! a writer-side [`Mutex`](std::sync::Mutex) serializes writers so no update is
//! lost.
//!
//! The trade-off is explicit: **reads are lock-free and wait-free, but every
//! insert is `O(n)`** because it copies the whole map. This backend is therefore
//! suited to read-mostly workloads with relatively few distinct interned nodes.
//!
//! As with the other concurrent wrappers, strong references are retained (no
//! weak-reference GC) and there is no inner single-threaded
//! [`ThreadUnsafeTable`](crate::ThreadUnsafeTable).

use crate::reference::*;
use crate::table::weak_holder::WeakEntryStrong;
use crate::table::*;
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

pub struct TableSharedArcSwap<D, V, HB = DefaultHasher>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    V: Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    map: ArcSwap<HashMap<D, V, HB>>,
    /// Serializes writers so concurrent copy-update-swap cycles do not clobber
    /// each other. Readers never take this lock.
    write_lock: Mutex<()>,
}

impl<D, V, HB> Default for TableSharedArcSwap<D, V, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    V: Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn default() -> Self {
        Self::with_hasher(HB::default())
    }
}

impl<D, V, HB> TableSharedArcSwap<D, V, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    V: Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    pub fn with_hasher(hash_builder: HB) -> Self {
        let empty = HashMap::with_hasher(hash_builder);
        Self {
            map: ArcSwap::from_pointee(empty),
            write_lock: Mutex::new(()),
        }
    }
}

/// Private map plumbing, shared by the strong ([`Table`]) and weak
/// ([`NonPurgingTable`]) views so the raw copy-on-write calls live in one place.
impl<D, V, HB> TableSharedArcSwap<D, V, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    V: Clone + Send + Sync,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    fn map_get(&self, data: &D) -> Option<V> {
        self.map.load().get(data).cloned()
    }

    fn map_retain<F>(&self, mut keep: F)
    where
        F: FnMut(&V) -> bool,
    {
        let _wguard = self.write_lock.lock().unwrap();
        let current = self.map.load();
        let mut new_map: HashMap<D, V, HB> = (**current).clone();
        new_map.retain(|_k, v| keep(v));
        self.map.store(Arc::new(new_map));
    }

    fn map_len(&self) -> usize {
        self.map.load().len()
    }
}

impl<D, R, WR, HB> Table<D, R, WR> for TableSharedArcSwap<D, R, HB>
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
        // Lock-free fast path.
        if let Some(r) = self.map.load().get(&data) {
            return R::strong_clone(r);
        }
        // Serialize writers, then re-check: another writer may have interned this
        // node between the fast-path load and acquiring the lock.
        let _wguard = self.write_lock.lock().unwrap();
        let current = self.map.load();
        if let Some(r) = current.get(&data) {
            return R::strong_clone(r);
        }
        creation_meta(&mut data);
        let obj = R::new(data);
        let key = R::strong_deref(&obj).clone();
        let mut new_map: HashMap<D, R, HB> = (**current).clone();
        new_map.insert(key, R::strong_clone(&obj));
        self.map.store(Arc::new(new_map));
        obj
    }

    #[cfg(feature = "reset-tables")]
    fn reset(&self) {
        let _wguard = self.write_lock.lock().unwrap();
        self.map
            .store(Arc::new(HashMap::with_hasher(HB::default())));
    }
}

/// Weak-reference (non-purging) view: stores weak handles; the purge adapter
/// drives eviction.
impl<D, R, WR, HB> NonPurgingTable<D, R, WR>
    for TableSharedArcSwap<D, WeakEntryStrong<D, R, WR>, HB>
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
        if let Some(existing) = self.map_get(&data).and_then(|entry| entry.upgrade()) {
            return existing;
        }
        // The RCU write lock (inherent to arc-swap, not added by the purge
        // adapter) serializes copy-update-swap cycles; re-check under it.
        let _wguard = self.write_lock.lock().unwrap();
        let current = self.map.load();
        if let Some(existing) = current.get(&data).and_then(|entry| entry.upgrade()) {
            return existing;
        }
        creation_meta(&mut data);
        let obj = R::new(data);
        let mut new_map: HashMap<D, WeakEntryStrong<D, R, WR>, HB> = (**current).clone();
        // `insert` overwrites any dead entry left under this key.
        new_map.insert(
            R::strong_deref(&obj).clone(),
            WeakEntryStrong::downgrade(&obj),
        );
        self.map.store(Arc::new(new_map));
        obj
    }

    fn retain_alive(&self) {
        self.map_retain(|entry| entry.is_alive());
    }

    fn len(&self) -> usize {
        self.map_len()
    }
}

pub struct BuildTableSharedArcSwap<D, R, HB> {
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R, HB> BuildTableSharedArcSwap<D, R, HB>
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

impl<D, R, HB> Clone for BuildTableSharedArcSwap<D, R, HB>
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

impl<D, R, HB> Default for BuildTableSharedArcSwap<D, R, HB>
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

impl<D, R, WR, HB> BuildTable<D, R, WR> for BuildTableSharedArcSwap<D, R, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync,
    R: Reference<D> + Clone + Send + Sync,
    WR: ReferenceWeak<D, R>,
    HB: std::hash::BuildHasher + Default + Clone + Send + Sync,
{
    type TableSharedType = TableSharedArcSwap<D, R, HB>;

    fn build_tableshared(&self) -> TableSharedArcSwap<D, R, HB> {
        TableSharedArcSwap::with_hasher(self.hash_builder.clone())
    }
}
