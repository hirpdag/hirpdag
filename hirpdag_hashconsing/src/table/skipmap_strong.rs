//! Concurrent hash-consing table backed by [`crossbeam_skiplist::SkipMap`].
//!
//! A skip list is a probabilistically balanced, *ordered* map. `SkipMap` is a
//! lock-free concurrent implementation using epoch-based reclamation. Unlike the
//! hash-map backends it keeps entries sorted by key, so it needs no hasher — the
//! key type only has to be [`Ord`]. Lookups are `O(log n)` rather than `O(1)`,
//! which is the trade-off for ordered iteration and lock-free progress.
//!
//! As with the other concurrent wrappers, the interned mapping is stored
//! directly in the structure (there is no inner single-threaded
//! [`ThreadUnsafeTable`](crate::ThreadUnsafeTable)), and strong references are retained (no
//! weak-reference GC of unreferenced nodes).

use crate::reference::*;
use crate::table::weak_holder::WeakEntryStrong;
use crate::table::*;
use crossbeam_skiplist::SkipMap;

pub struct TableSharedSkipMap<D, V>
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
{
    map: SkipMap<D, V>,
}

impl<D, V> Default for TableSharedSkipMap<D, V>
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
{
    fn default() -> Self {
        Self {
            map: SkipMap::new(),
        }
    }
}

/// Private map plumbing, shared by the strong ([`Table`]) and weak
/// ([`NonPurgingTable`]) views so the raw `SkipMap` calls live in one place.
impl<D, V> TableSharedSkipMap<D, V>
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
{
    fn map_get(&self, data: &D) -> Option<V> {
        self.map.get(data).map(|e| e.value().clone())
    }

    fn map_retain<F>(&self, mut keep: F)
    where
        F: FnMut(&V) -> bool,
    {
        // SkipMap has no bulk retain; collect the dead keys then remove them.
        // Iteration and removal are both lock-free and safe to interleave.
        let mut dead: Vec<D> = Vec::new();
        for e in self.map.iter() {
            if !keep(e.value()) {
                dead.push(e.key().clone());
            }
        }
        for k in dead {
            self.map.remove(&k);
        }
    }

    fn map_len(&self) -> usize {
        self.map.len()
    }
}

impl<D, R, WR> Table<D, R, WR> for TableSharedSkipMap<D, R>
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
{
    fn get(&self, data: &D) -> Option<R> {
        self.map_get(data)
    }

    fn get_or_insert<CF>(&self, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        if let Some(e) = self.map.get(&data) {
            return R::strong_clone(e.value());
        }
        // The closure runs only when this thread performs the insertion, so on a
        // fast-path hit (and usually on a lost race too) we neither build a node
        // nor consume a creation id.
        let key = data.clone();
        let entry = self.map.get_or_insert_with(key, move || {
            creation_meta(&mut data);
            R::new(data)
        });
        R::strong_clone(entry.value())
    }

    #[cfg(feature = "reset-tables")]
    fn reset(&self) {
        // Logically empties the map. Note: crossbeam-skiplist reclaims removed
        // nodes lazily via crossbeam-epoch, so the memory is not necessarily
        // freed synchronously here; a peak-heap memory benchmark of this backend
        // will therefore be noisier than one of a promptly-freeing table.
        self.map.clear();
    }
}

/// Weak-reference (non-purging) view: stores weak handles; the purge adapter
/// drives eviction.
impl<D, R, WR> NonPurgingTable<D, R, WR> for TableSharedSkipMap<D, WeakEntryStrong<D, R, WR>>
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
{
    fn get(&self, data: &D) -> Option<R> {
        self.map_get(data).and_then(|entry| entry.upgrade())
    }

    fn get_or_insert<CF>(&self, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        if let Some(existing) = self.map_get(&data).and_then(|entry| entry.upgrade()) {
            return existing;
        }
        creation_meta(&mut data);
        let obj = R::new(data);
        let key = R::strong_deref(&obj).clone();
        loop {
            // Insert our weak if the slot is absent or dead; keep an existing
            // live entry. `compare_insert` re-evaluates the predicate under the
            // final CAS, so a node that became live is never clobbered.
            let entry =
                self.map
                    .compare_insert(key.clone(), WeakEntryStrong::downgrade(&obj), |current| {
                        !current.is_alive()
                    });
            if let Some(existing) = entry.value().upgrade() {
                return existing;
            }
            // The observed entry died between compare and upgrade; retry (the
            // predicate will now replace it with our node).
        }
    }

    fn retain_alive(&self) {
        self.map_retain(|entry| entry.is_alive());
    }

    fn len(&self) -> usize {
        self.map_len()
    }
}

pub struct BuildTableSharedSkipMap<D, R> {
    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R> BuildTableSharedSkipMap<D, R> {
    pub fn new() -> Self {
        Self {
            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}

impl<D, R> Clone for BuildTableSharedSkipMap<D, R> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<D, R> Default for BuildTableSharedSkipMap<D, R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D, R, WR> BuildTable<D, R, WR> for BuildTableSharedSkipMap<D, R>
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
{
    type TableSharedType = TableSharedSkipMap<D, R>;

    fn build_tableshared(&self) -> TableSharedSkipMap<D, R> {
        TableSharedSkipMap::default()
    }
}
