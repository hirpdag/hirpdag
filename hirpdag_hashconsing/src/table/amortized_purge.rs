//! Adapter turning a [`NonPurgingTable`] into a purging [`Table`].
//!
//! A [`NonPurgingTable`] stores weak references but never evicts dead ones, so
//! on its own it would leak: entries whose referent has been dropped accumulate,
//! and workloads that re-intern equivalent nodes under fresh keys (node hashes
//! fold in child creation IDs, which change when a dead node is re-created) grow
//! the map without bound.
//!
//! [`TableAmortizedPurge`] adds the missing purging without adding any
//! synchronization of its own — the inner table is already concurrent:
//!
//! * `get` and `get_or_insert` delegate straight to the backend, whose
//!   `get_or_insert` is atomic via its native concurrency primitive (dashmap's
//!   per-shard entry lock, skipmap's `compare_insert`, flurry's `try_insert` /
//!   `compute_if_present`, arc-swap's inherent writer serialization).
//! * Dead entries are purged **amortized**: when the map grows past twice its
//!   size at the previous purge (never below [`PURGE_LEN_MIN`]), a single
//!   `retain_alive` sweep drops every dead entry, giving O(1) amortized work per
//!   insertion. The threshold is a plain [`AtomicUsize`]; a `compare_exchange`
//!   lets exactly one writer run each sweep while the others carry on, so there
//!   is no lock and no writer serialization beyond whatever the backend already
//!   does.

use crate::reference::*;
use crate::table::{BuildTable, NonPurgingTable, Table};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Purge dead entries once the map grows past twice its size at the last purge,
/// but never below this floor. Amortized O(1) per insertion.
const PURGE_LEN_MIN: usize = 64;

/// Sentinel parked in `purge_at_len` by the writer currently sweeping, so other
/// writers see an unreachable threshold and skip purging until it publishes the
/// real next threshold.
const PURGE_CLAIMED: usize = usize::MAX;

pub struct TableAmortizedPurge<D, R, WR, S>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    S: NonPurgingTable<D, R, WR>,
{
    inner: S,
    /// Amortized-purge threshold: the map is swept for dead entries once it
    /// reaches this length. Lock-free — no writer serialization.
    purge_at_len: AtomicUsize,

    // `fn() -> (...)` keeps the marker unconditionally `Send + Sync` with the
    // right variance; the tuple is a marker, not a real value.
    #[allow(clippy::type_complexity)]
    phantom: std::marker::PhantomData<fn() -> (D, R, WR)>,
}

impl<D, R, WR, S> Default for TableAmortizedPurge<D, R, WR, S>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    S: NonPurgingTable<D, R, WR> + Default,
{
    fn default() -> Self {
        Self::with_table(S::default())
    }
}

impl<D, R, WR, S> TableAmortizedPurge<D, R, WR, S>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    S: NonPurgingTable<D, R, WR>,
{
    /// Wrap an existing (possibly pre-configured) non-purging table.
    pub fn with_table(inner: S) -> Self {
        Self {
            inner,
            purge_at_len: AtomicUsize::new(PURGE_LEN_MIN),
            phantom: std::marker::PhantomData,
        }
    }

    /// The number of entries currently held by the underlying table, including
    /// any dead entries not yet purged.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether the underlying table currently holds no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Sweep dead entries if the map has grown past the threshold. Lock-free:
    /// `compare_exchange` elects a single sweeper; other writers skip.
    fn maybe_purge(&self) {
        let threshold = self.purge_at_len.load(Ordering::Relaxed);
        if self.inner.len() < threshold {
            return;
        }
        // Claim the sweep. If another writer claimed it first, leave it to them.
        if self
            .purge_at_len
            .compare_exchange(
                threshold,
                PURGE_CLAIMED,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            self.inner.retain_alive();
            let next = std::cmp::max(PURGE_LEN_MIN, self.inner.len().saturating_mul(2));
            self.purge_at_len.store(next, Ordering::Relaxed);
        }
    }
}

impl<D, R, WR, S> Table<D, R, WR> for TableAmortizedPurge<D, R, WR, S>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    S: NonPurgingTable<D, R, WR>,
{
    fn get(&self, data: &D) -> Option<R> {
        self.inner.get(data)
    }

    fn get_or_insert<CF>(&self, data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        // The backend's own concurrency makes this atomic; we add only the
        // amortized dead-entry sweep afterwards.
        let obj = self.inner.get_or_insert(data, creation_meta);
        self.maybe_purge();
        obj
    }
}

/// Builder for [`TableAmortizedPurge`] over a default-constructed inner
/// [`NonPurgingTable`].
pub struct BuildTableAmortizedPurge<D, R, WR, S> {
    #[allow(clippy::type_complexity)]
    phantom: std::marker::PhantomData<fn() -> (D, R, WR, S)>,
}

impl<D, R, WR, S> BuildTableAmortizedPurge<D, R, WR, S> {
    pub fn new() -> Self {
        Self {
            phantom: std::marker::PhantomData,
        }
    }
}

impl<D, R, WR, S> Clone for BuildTableAmortizedPurge<D, R, WR, S> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<D, R, WR, S> Default for BuildTableAmortizedPurge<D, R, WR, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D, R, WR, S> BuildTable<D, R, WR> for BuildTableAmortizedPurge<D, R, WR, S>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    S: NonPurgingTable<D, R, WR> + Default,
{
    type TableSharedType = TableAmortizedPurge<D, R, WR, S>;

    fn build_tableshared(&self) -> TableAmortizedPurge<D, R, WR, S> {
        TableAmortizedPurge::default()
    }
}
