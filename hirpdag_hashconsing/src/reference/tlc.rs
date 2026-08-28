//! Reference counting with thread-local buffering of count updates.
//!
//! This explores the TODO idea of thread-local reference counts which are
//! periodically flushed back to the main counter.
//!
//! Buffering *increments* thread-locally is unsound with cross-thread handle
//! transfer: a handle whose increment is still buffered on thread A can be
//! moved to thread B, and B's drop could take the shared count to zero and
//! free the data while A's buffered increment (and A's own live handle) still
//! exist. Buffering *decrements* is safe: it only delays the count reaching
//! zero, so objects are freed late, never early.
//!
//! Therefore each thread keeps a map of deferred decrements:
//!
//! - Dropping a strong handle adds a deferred decrement instead of touching
//!   the shared atomic counter.
//! - Cloning a strong handle first tries to consume a deferred decrement for
//!   the same object (the new handle takes over the shared count the dropped
//!   handle held); only on a miss does it touch the shared counter.
//! - Upgrading a weak handle can do the same: a deferred decrement proves the
//!   object is still alive, so the upgrade can succeed without a CAS.
//! - The map is flushed (applying the pending decrements to the shared
//!   counters) after a bounded number of buffered operations, and when the
//!   thread exits.
//!
//! In clone/drop-heavy workloads (e.g. rewrites) most count updates pair up
//! thread-locally and never touch the shared cacheline. The cost is a
//! thread-local hash map operation per clone/drop, and objects staying alive
//! slightly longer than their last handle.

use crate::reference::*;

/// Flush the deferred decrement map after this many buffered operations.
const FLUSH_OPS: usize = 4096;
/// Flush the deferred decrement map if it grows beyond this many objects.
const FLUSH_ENTRIES: usize = 1024;

struct TlcInner<D> {
    strong: std::sync::atomic::AtomicUsize,
    weak: std::sync::atomic::AtomicUsize,
    data: std::mem::ManuallyDrop<D>,
}

/// Decrement the strong count of the `TlcInner<D>` at `addr` by `n`,
/// dropping the data (and possibly the allocation) if it reaches zero.
///
/// Type-erased so deferred decrements for different `D` can share one map.
///
/// Safety: `addr` must point to a live `TlcInner<D>` whose strong count is at
/// least `n` (guaranteed because each deferred decrement corresponds to a
/// dropped handle whose count has not yet been applied).
unsafe fn release_strong<D>(addr: usize, n: usize) {
    let inner = addr as *mut TlcInner<D>;
    if (*inner)
        .strong
        .fetch_sub(n, std::sync::atomic::Ordering::Release)
        == n
    {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        std::mem::ManuallyDrop::drop(&mut (*inner).data);
        // Release the weak count held collectively by strong references.
        if (*inner)
            .weak
            .fetch_sub(1, std::sync::atomic::Ordering::Release)
            == 1
        {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            drop(std::boxed::Box::from_raw(inner));
        }
    }
}

struct DeferredEntry {
    count: usize,
    release: unsafe fn(usize, usize),
}

struct DeferredDecs {
    map: std::collections::HashMap<usize, DeferredEntry>,
    ops_since_flush: usize,
}

impl DeferredDecs {
    /// Drain all buffered decrements. The caller must apply them with
    /// [`apply_pending`] *after* releasing the `RefCell` borrow: applying a
    /// decrement can drop data whose fields are themselves `RefTlc` handles,
    /// and their drops re-enter the thread-local map.
    fn take_pending(&mut self) -> Vec<(usize, DeferredEntry)> {
        self.ops_since_flush = 0;
        self.map.drain().collect()
    }

    /// Buffer one deferred decrement for `addr`. Returns drained decrements
    /// for the caller to apply if a flush threshold was reached.
    fn defer(
        &mut self,
        addr: usize,
        release: unsafe fn(usize, usize),
    ) -> Option<Vec<(usize, DeferredEntry)>> {
        self.map
            .entry(addr)
            .or_insert(DeferredEntry { count: 0, release })
            .count += 1;
        self.ops_since_flush += 1;
        if self.ops_since_flush >= FLUSH_OPS || self.map.len() >= FLUSH_ENTRIES {
            Some(self.take_pending())
        } else {
            None
        }
    }

    /// Consume one deferred decrement for `addr` if present, cancelling it
    /// against an increment. Returns true on success.
    fn consume(&mut self, addr: usize) -> bool {
        if let std::collections::hash_map::Entry::Occupied(mut entry) = self.map.entry(addr) {
            entry.get_mut().count -= 1;
            if entry.get().count == 0 {
                entry.remove();
            }
            true
        } else {
            false
        }
    }
}

fn apply_pending(pending: Vec<(usize, DeferredEntry)>) {
    for (addr, entry) in pending {
        // Safety: each buffered count corresponds to a dropped handle whose
        // shared count has not yet been decremented, which also keeps the
        // object alive.
        unsafe { (entry.release)(addr, entry.count) };
    }
}

impl Drop for DeferredDecs {
    fn drop(&mut self) {
        // Runs at thread exit. Child handle drops triggered here cannot
        // re-enter the map: LocalKey::try_with fails during destruction, so
        // they fall back to direct shared-counter decrements.
        apply_pending(self.take_pending());
    }
}

thread_local! {
    static TLC_DEFERRED: std::cell::RefCell<DeferredDecs> =
        std::cell::RefCell::new(DeferredDecs {
            map: std::collections::HashMap::new(),
            ops_since_flush: 0,
        });
}

/// Consume a deferred decrement for `addr` on this thread, if one exists.
fn tlc_consume(addr: usize) -> bool {
    // try_with: the thread-local may already be destroyed during thread exit;
    // fall back to the shared counter in that case.
    TLC_DEFERRED
        .try_with(|d| d.borrow_mut().consume(addr))
        .unwrap_or(false)
}

/// Buffer a deferred decrement for `addr` on this thread.
/// Returns false if the thread-local buffer is unavailable (thread exit).
fn tlc_defer(addr: usize, release: unsafe fn(usize, usize)) -> bool {
    let deferred = TLC_DEFERRED.try_with(|d| d.borrow_mut().defer(addr, release));
    match deferred {
        Ok(pending) => {
            // Apply outside the borrow: dropped data may contain RefTlc
            // fields whose drops re-enter the map.
            if let Some(pending) = pending {
                apply_pending(pending);
            }
            true
        }
        Err(_) => false,
    }
}

/// Strong reference handle with thread-local deferred decrements.
pub struct RefTlc<D> {
    ptr: std::ptr::NonNull<TlcInner<D>>,
}

unsafe impl<D: Send + Sync> Send for RefTlc<D> {}
unsafe impl<D: Send + Sync> Sync for RefTlc<D> {}

impl<D> RefTlc<D> {
    fn inner(&self) -> &TlcInner<D> {
        unsafe { self.ptr.as_ref() }
    }

    fn addr(&self) -> usize {
        self.ptr.as_ptr() as usize
    }
}

impl<D> Drop for RefTlc<D> {
    fn drop(&mut self) {
        if !tlc_defer(self.addr(), release_strong::<D>) {
            unsafe { release_strong::<D>(self.addr(), 1) };
        }
    }
}

impl<D> Reference<D> for RefTlc<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn new(data: D) -> Self {
        let inner = std::boxed::Box::into_raw(std::boxed::Box::new(TlcInner {
            strong: std::sync::atomic::AtomicUsize::new(1),
            weak: std::sync::atomic::AtomicUsize::new(1),
            data: std::mem::ManuallyDrop::new(data),
        }));
        Self {
            // Box::into_raw is never null.
            ptr: unsafe { std::ptr::NonNull::new_unchecked(inner) },
        }
    }

    fn strong_deref(ptr: &Self) -> &D {
        &ptr.inner().data
    }

    fn strong_clone(ptr: &Self) -> Self {
        // Cancel against a deferred decrement if possible: the new handle
        // takes over the shared count held by a previously dropped handle.
        if !tlc_consume(ptr.addr()) {
            ptr.inner()
                .strong
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Self { ptr: ptr.ptr }
    }

    fn strong_ptr_eq(a: &Self, b: &Self) -> bool {
        a.ptr == b.ptr
    }
}

/// Weak reference handle for [`RefTlc`].
///
/// Weak count updates are not buffered; they are far less frequent (one
/// downgrade per interned object, upgrades only on hashcons table hits).
pub struct RefTlcWeak<D> {
    ptr: std::ptr::NonNull<TlcInner<D>>,
}

unsafe impl<D: Send + Sync> Send for RefTlcWeak<D> {}
unsafe impl<D: Send + Sync> Sync for RefTlcWeak<D> {}

impl<D> Drop for RefTlcWeak<D> {
    fn drop(&mut self) {
        let inner = unsafe { self.ptr.as_ref() };
        if inner
            .weak
            .fetch_sub(1, std::sync::atomic::Ordering::Release)
            == 1
        {
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            unsafe { drop(std::boxed::Box::from_raw(self.ptr.as_ptr())) };
        }
    }
}

impl<D> ReferenceWeak<D, RefTlc<D>> for RefTlcWeak<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn weak_upgrade(ptr: &Self) -> std::option::Option<RefTlc<D>> {
        let addr = ptr.ptr.as_ptr() as usize;
        // A deferred decrement on this thread proves the object is alive
        // (its shared count has not been applied yet), so the upgrade can
        // take over that count without touching the shared atomic.
        if tlc_consume(addr) {
            return Some(RefTlc { ptr: ptr.ptr });
        }
        let inner = unsafe { ptr.ptr.as_ref() };
        let mut n = inner.strong.load(std::sync::atomic::Ordering::Relaxed);
        loop {
            if n == 0 {
                return None;
            }
            match inner.strong.compare_exchange_weak(
                n,
                n + 1,
                std::sync::atomic::Ordering::Acquire,
                std::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => return Some(RefTlc { ptr: ptr.ptr }),
                Err(actual) => n = actual,
            }
        }
    }

    fn weak_downgrade(ptr: &RefTlc<D>) -> Self {
        ptr.inner()
            .weak
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self { ptr: ptr.ptr }
    }

    fn weak_clone(ptr: &Self) -> Self {
        // Take an additional weak count. The allocation is kept alive by the
        // weak count until the last weak handle drops, so this is sound whether
        // or not the strong data is still live.
        let inner = unsafe { ptr.ptr.as_ref() };
        inner
            .weak
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self { ptr: ptr.ptr }
    }

    fn weak_ptr_id(ptr: &Self) -> usize {
        ptr.ptr.as_ptr() as usize
    }
}
