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
//! [`Table`](crate::Table)), and strong references are retained (no
//! weak-reference GC of unreferenced nodes).

use crate::reference::*;
use crate::table::*;
use crossbeam_skiplist::SkipMap;

pub struct TableSharedSkipMap<D, R>
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
{
    map: SkipMap<D, R>,
}

impl<D, R> Default for TableSharedSkipMap<D, R>
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
{
    fn default() -> Self {
        Self {
            map: SkipMap::new(),
        }
    }
}

impl<D, R> TableShared<D, R> for TableSharedSkipMap<D, R>
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
{
    fn get(&self, data: &D) -> Option<R> {
        self.map.get(data).map(|e| R::strong_clone(e.value()))
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

impl<D, R> BuildTableShared<D, R> for BuildTableSharedSkipMap<D, R>
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
{
    type TableSharedType = TableSharedSkipMap<D, R>;

    fn build_tableshared(&self) -> TableSharedSkipMap<D, R> {
        TableSharedSkipMap::default()
    }
}
