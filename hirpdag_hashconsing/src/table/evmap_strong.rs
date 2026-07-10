//! Hash-consing table using [`evmap`], an eventually-consistent map built on the
//! left-right (double-buffering) pattern.
//!
//! evmap keeps two copies of the map. Readers operate lock-free on one copy via
//! a [`ReadHandle`](evmap::handles::ReadHandle); the single writer mutates the
//! other copy and calls `publish()` to atomically swap which copy readers see.
//! Reads never block writes and writes never block reads.
//!
//! To make this correct for hash-consing we `publish()` after every insert (so a
//! node is visible immediately) and re-check under the writer lock before
//! inserting (so two threads cannot intern structurally-equal duplicates).
//!
//! ## A note on this wrapper's `Sync` strategy
//!
//! evmap's `ReadHandle` is deliberately `!Sync` (each reader thread is meant to
//! own its own clone), and evmap 11 does not expose a public `ReadHandleFactory`
//! accessor to hand out fresh handles from `&self`. To satisfy the
//! [`Table`] contract (`&self`, shared across threads) we therefore guard
//! the read handle with its own `Mutex`, separate from the writer's `Mutex`.
//! Reads and writes still never block *each other* (distinct locks + evmap's
//! double buffering), which preserves the essential left-right property, but
//! concurrent readers do contend on the read mutex here. A deployment wanting
//! fully lock-free reads would distribute per-thread `ReadHandle` clones instead.
//!
//! As with the other concurrent wrappers, strong references are retained (no
//! weak-reference GC) and there is no inner single-threaded
//! [`ThreadUnsafeTable`](crate::ThreadUnsafeTable). The stored value type `R` must be
//! [`Hash`](std::hash::Hash) + `Eq` because evmap keeps values in a set-like bag.

use crate::reference::*;
use crate::table::*;
use std::sync::Mutex;

pub struct TableSharedEvmap<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync + 'static,
    R: Reference<D> + std::hash::Hash + std::cmp::Eq + Clone + Send + Sync + 'static,
{
    write: Mutex<evmap::handles::WriteHandle<D, R>>,
    read: Mutex<evmap::handles::ReadHandle<D, R>>,
}

impl<D, R> Default for TableSharedEvmap<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync + 'static,
    R: Reference<D> + std::hash::Hash + std::cmp::Eq + Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        // Safety: `new_assert_stable` requires that `Hash`/`Eq` for `D` and `R`
        // are deterministic and stable across clones of a key. Both hold here:
        // `D`'s `Hash`/`Eq` are the user's structural implementations and `R`
        // (a reference handle) hashes/compares by the data it points at.
        let (write, read) = unsafe { evmap::new_assert_stable::<D, R>() };
        Self {
            write: Mutex::new(write),
            read: Mutex::new(read),
        }
    }
}

impl<D, R> Table<D, R> for TableSharedEvmap<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync + 'static,
    R: Reference<D> + std::hash::Hash + std::cmp::Eq + Clone + Send + Sync + 'static,
{
    fn get(&self, data: &D) -> Option<R> {
        let read = self.read.lock().unwrap();
        read.get_one(data).map(|v| R::strong_clone(&v))
    }

    fn get_or_insert<CF>(&self, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        // Lock-free-ish fast path (contends only with other readers). The lookup
        // result is turned into an owned `R` before the read guard is released.
        let hit = {
            let read = self.read.lock().unwrap();
            read.get_one(&data).map(|v| R::strong_clone(&v))
        };
        if let Some(r) = hit {
            return r;
        }
        // Writer path. `WriteHandle` derefs to a `ReadHandle` over the last
        // published state, so re-checking here — while holding the writer lock —
        // is authoritative against every previously published (inserted) node.
        let mut write = self.write.lock().unwrap();
        let hit = write.get_one(&data).map(|v| R::strong_clone(&v));
        if let Some(r) = hit {
            return r;
        }
        creation_meta(&mut data);
        let obj = R::new(data);
        let key = R::strong_deref(&obj).clone();
        write.insert(key, R::strong_clone(&obj));
        write.publish();
        obj
    }
}

pub struct BuildTableSharedEvmap<D, R> {
    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R> BuildTableSharedEvmap<D, R> {
    pub fn new() -> Self {
        Self {
            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}

impl<D, R> Clone for BuildTableSharedEvmap<D, R> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<D, R> Default for BuildTableSharedEvmap<D, R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D, R> BuildTable<D, R> for BuildTableSharedEvmap<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug + Clone + Send + Sync + 'static,
    R: Reference<D> + std::hash::Hash + std::cmp::Eq + Clone + Send + Sync + 'static,
{
    type TableSharedType = TableSharedEvmap<D, R>;

    fn build_tableshared(&self) -> TableSharedEvmap<D, R> {
        TableSharedEvmap::default()
    }
}
