//! Reference counting with counts stored separately from the data.
//!
//! `std::sync::Arc` places the reference counts in the same allocation as the
//! data. Every clone/drop dirties the cacheline holding the counts, which is
//! usually the same cacheline (or at least the same allocation) as the start of
//! the data. Other CPU cores reading the data then take coherence misses caused
//! purely by reference count traffic.
//!
//! Here the counts live in a separate, contiguous global pool of count slots.
//! Cachelines holding the immutable data are never dirtied by reference count
//! updates, so they can stay in the shared (read-only) cache state on all cores.
//!
//! The handle is two pointers (data + count slot) instead of Arc's one.
//!
//! Three slot layouts explore the space/performance trade-off:
//!
//! - [`RefSep`]: packed slots, two `usize` counters (16 bytes on 64-bit).
//!   Four slots share a 64-byte cacheline, so counts of unrelated objects can
//!   false-share.
//! - [`RefSepPad`]: slots padded to 64 bytes, one (strong, weak) pair per
//!   cacheline. No false sharing between objects, 4x the count memory.
//! - [`RefSepU32`]: packed `u32` counters (8 bytes). Eight slots per cacheline;
//!   densest, most false sharing. Counts must stay below `u32::MAX`.
//!
//! Slots are allocated from a global pool in contiguous chunks and recycled
//! through a free list. Chunks are never returned to the OS (the pool is an
//! arena); the peak number of live objects bounds the pool size.

use crate::reference::*;

const CHUNK_SLOTS: usize = 4096;

/// A (strong, weak) count pair which can be allocated from a global pool.
///
/// The weak count follows the `std::sync::Arc` convention: all strong handles
/// collectively hold one weak count, and each weak handle holds one. The slot
/// is returned to the pool when the weak count reaches zero.
pub trait CountSlot: Default + Send + Sync + Sized + 'static {
    /// Reset the slot for a freshly created object: strong = 1, weak = 1.
    fn init(&self);
    /// Increment the strong count.
    fn strong_inc(&self);
    /// Decrement the strong count. Returns true if it reached zero.
    fn strong_dec(&self) -> bool;
    /// Increment the strong count if it is nonzero. Returns false if zero.
    fn strong_upgrade(&self) -> bool;
    /// Increment the weak count.
    fn weak_inc(&self);
    /// Decrement the weak count. Returns true if it reached zero.
    fn weak_dec(&self) -> bool;
    /// The global pool which slots of this type are allocated from.
    fn pool() -> &'static SlotPool<Self>;
}

/// A global arena of count slots with a free list.
///
/// Slot allocation/free takes a mutex, but that only happens on object
/// creation and final release; clone/drop of handles touch only the atomics
/// in the slot itself.
pub struct SlotPool<S: 'static> {
    inner: std::sync::Mutex<SlotPoolInner<S>>,
}

struct SlotPoolInner<S: 'static> {
    chunks: Vec<Box<[S]>>,
    free: Vec<std::ptr::NonNull<S>>,
}

// NonNull<S> makes SlotPoolInner !Send. The pointers refer to slots in the
// owned chunks, which are Send (S: Send), so moving/sharing the pool across
// threads is sound.
unsafe impl<S: Send> Send for SlotPoolInner<S> {}

impl<S: CountSlot> SlotPool<S> {
    pub const fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(SlotPoolInner {
                chunks: Vec::new(),
                free: Vec::new(),
            }),
        }
    }

    fn alloc(&self) -> std::ptr::NonNull<S> {
        let mut guard = self.inner.lock().unwrap();
        if guard.free.is_empty() {
            let chunk: Box<[S]> = (0..CHUNK_SLOTS).map(|_| S::default()).collect();
            for slot in chunk.iter() {
                guard.free.push(std::ptr::NonNull::from(slot));
            }
            guard.chunks.push(chunk);
        }
        let slot = guard.free.pop().unwrap();
        // All mutation of slots goes through atomics, so a shared reference
        // derived from the pool's chunks is sufficient.
        unsafe { slot.as_ref() }.init();
        slot
    }

    fn free(&self, slot: std::ptr::NonNull<S>) {
        let mut guard = self.inner.lock().unwrap();
        guard.free.push(slot);
    }

    /// (allocated chunks, free slots) — for tests and leak diagnosis.
    pub fn stats(&self) -> (usize, usize) {
        let guard = self.inner.lock().unwrap();
        (guard.chunks.len(), guard.free.len())
    }
}

macro_rules! define_count_slot {
    ($(#[$attr:meta])* $name:ident, $atomic:ty, $pool:ident) => {
        $(#[$attr])*
        pub struct $name {
            strong: $atomic,
            weak: $atomic,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    strong: <$atomic>::new(0),
                    weak: <$atomic>::new(0),
                }
            }
        }

        static $pool: SlotPool<$name> = SlotPool::new();

        impl CountSlot for $name {
            fn init(&self) {
                self.strong.store(1, std::sync::atomic::Ordering::Relaxed);
                self.weak.store(1, std::sync::atomic::Ordering::Relaxed);
            }

            fn strong_inc(&self) {
                self.strong.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }

            fn strong_dec(&self) -> bool {
                if self.strong.fetch_sub(1, std::sync::atomic::Ordering::Release) == 1 {
                    std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
                    return true;
                }
                false
            }

            fn strong_upgrade(&self) -> bool {
                let mut n = self.strong.load(std::sync::atomic::Ordering::Relaxed);
                loop {
                    if n == 0 {
                        return false;
                    }
                    match self.strong.compare_exchange_weak(
                        n,
                        n + 1,
                        std::sync::atomic::Ordering::Acquire,
                        std::sync::atomic::Ordering::Relaxed,
                    ) {
                        Ok(_) => return true,
                        Err(actual) => n = actual,
                    }
                }
            }

            fn weak_inc(&self) {
                self.weak.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }

            fn weak_dec(&self) -> bool {
                if self.weak.fetch_sub(1, std::sync::atomic::Ordering::Release) == 1 {
                    std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
                    return true;
                }
                false
            }

            fn pool() -> &'static SlotPool<Self> {
                &$pool
            }
        }
    };
}

define_count_slot!(
    /// Packed slot: two `usize` counters, 16 bytes on 64-bit targets.
    SlotPacked,
    std::sync::atomic::AtomicUsize,
    SLOT_POOL_PACKED
);

define_count_slot!(
    /// Cacheline-padded slot: one (strong, weak) pair per 64-byte cacheline.
    #[repr(align(64))]
    SlotPadded,
    std::sync::atomic::AtomicUsize,
    SLOT_POOL_PADDED
);

define_count_slot!(
    /// Small packed slot: two `u32` counters, 8 bytes. Counts must stay below
    /// `u32::MAX`; this is an experiment in count density, not a general
    /// purpose implementation.
    SlotPackedU32,
    std::sync::atomic::AtomicU32,
    SLOT_POOL_PACKED_U32
);

/// Strong reference handle: a data pointer plus a count slot pointer.
pub struct RefSepGeneric<D, S: CountSlot> {
    data: std::ptr::NonNull<D>,
    slot: std::ptr::NonNull<S>,
}

unsafe impl<D: Send + Sync, S: CountSlot> Send for RefSepGeneric<D, S> {}
unsafe impl<D: Send + Sync, S: CountSlot> Sync for RefSepGeneric<D, S> {}

impl<D, S: CountSlot> RefSepGeneric<D, S> {
    fn slot(&self) -> &S {
        // The slot outlives all strong and weak handles (freed on weak == 0).
        unsafe { self.slot.as_ref() }
    }
}

impl<D, S: CountSlot> Drop for RefSepGeneric<D, S> {
    fn drop(&mut self) {
        if self.slot().strong_dec() {
            // Last strong reference: drop the data.
            unsafe {
                drop(std::boxed::Box::from_raw(self.data.as_ptr()));
            }
            // Release the weak count held collectively by strong references.
            if self.slot().weak_dec() {
                S::pool().free(self.slot);
            }
        }
    }
}

impl<D, S> Reference<D> for RefSepGeneric<D, S>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    S: CountSlot,
{
    fn new(data: D) -> Self {
        let data = std::boxed::Box::into_raw(std::boxed::Box::new(data));
        Self {
            // Box::into_raw is never null.
            data: unsafe { std::ptr::NonNull::new_unchecked(data) },
            slot: S::pool().alloc(),
        }
    }

    fn strong_deref(ptr: &Self) -> &D {
        unsafe { ptr.data.as_ref() }
    }

    fn strong_clone(ptr: &Self) -> Self {
        ptr.slot().strong_inc();
        Self {
            data: ptr.data,
            slot: ptr.slot,
        }
    }

    fn strong_ptr_eq(a: &Self, b: &Self) -> bool {
        a.data == b.data
    }
}

/// Weak reference handle.
///
/// The data pointer may dangle once all strong references are gone; it is only
/// dereferenced after a successful upgrade proves the data is still alive.
pub struct RefSepGenericWeak<D, S: CountSlot> {
    data: std::ptr::NonNull<D>,
    slot: std::ptr::NonNull<S>,
}

unsafe impl<D: Send + Sync, S: CountSlot> Send for RefSepGenericWeak<D, S> {}
unsafe impl<D: Send + Sync, S: CountSlot> Sync for RefSepGenericWeak<D, S> {}

impl<D, S: CountSlot> Drop for RefSepGenericWeak<D, S> {
    fn drop(&mut self) {
        let slot = unsafe { self.slot.as_ref() };
        if slot.weak_dec() {
            S::pool().free(self.slot);
        }
    }
}

impl<D, S> ReferenceWeak<D, RefSepGeneric<D, S>> for RefSepGenericWeak<D, S>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    S: CountSlot,
{
    fn weak_upgrade(ptr: &Self) -> std::option::Option<RefSepGeneric<D, S>> {
        let slot = unsafe { ptr.slot.as_ref() };
        if slot.strong_upgrade() {
            Some(RefSepGeneric {
                data: ptr.data,
                slot: ptr.slot,
            })
        } else {
            None
        }
    }

    fn weak_downgrade(ptr: &RefSepGeneric<D, S>) -> Self {
        ptr.slot().weak_inc();
        Self {
            data: ptr.data,
            slot: ptr.slot,
        }
    }
}

/// Separate contiguous counts, packed 16-byte slots.
pub type RefSep<D> = RefSepGeneric<D, SlotPacked>;
pub type RefSepWeak<D> = RefSepGenericWeak<D, SlotPacked>;

/// Separate contiguous counts, one slot per 64-byte cacheline.
pub type RefSepPad<D> = RefSepGeneric<D, SlotPadded>;
pub type RefSepPadWeak<D> = RefSepGenericWeak<D, SlotPadded>;

/// Separate contiguous counts, packed 8-byte slots with `u32` counters.
pub type RefSepU32<D> = RefSepGeneric<D, SlotPackedU32>;
pub type RefSepU32Weak<D> = RefSepGenericWeak<D, SlotPackedU32>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Table;
    use crate::test_utils::TestData;

    // Dedicated slot type so this test's pool is not shared with other tests
    // running in parallel.
    define_count_slot!(
        SlotTestOnly,
        std::sync::atomic::AtomicUsize,
        SLOT_POOL_TEST_ONLY
    );

    /// Mimic the benchmark pattern: every iteration re-interns the same set of
    /// nodes after all strong references from the previous iteration died.
    /// Slots must be recycled through the pool, not leaked.
    #[test]
    fn slots_recycled_across_intern_cycles() {
        type R = RefSepGeneric<TestData, SlotTestOnly>;
        type RW = RefSepGenericWeak<TestData, SlotTestOnly>;
        type T = crate::TableHashmapFallbackWeak<
            TestData,
            R,
            RW,
            crate::TableVecLinearWeak<TestData, R, RW>,
        >;
        let mut table = T::default();

        let n = 1000usize;
        let mut chunks_after_warmup = 0;
        for iteration in 0..50 {
            let mut live: Vec<R> = Vec::with_capacity(n);
            for k in 0..n {
                let data = TestData::new(k as i32, 0, "slots_recycled".to_string());
                live.push(table.get_or_insert(k as u64, data, |_| {}));
            }
            drop(live);
            let (chunks, _free) = SlotTestOnly::pool().stats();
            if iteration == 5 {
                chunks_after_warmup = chunks;
            } else if iteration > 5 {
                assert!(
                    chunks <= chunks_after_warmup,
                    "slot pool grew from {} to {} chunks by iteration {}: slots are leaking",
                    chunks_after_warmup,
                    chunks,
                    iteration
                );
            }
        }
    }
}
