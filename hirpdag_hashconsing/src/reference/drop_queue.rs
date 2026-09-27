//! Dropping handles without recursing through the graph.
//!
//! Freeing a node drops its data, which drops the handles in its fields, which
//! free the nodes they were the last handle to, and so on down the graph: one
//! level of recursion, several stack frames, per level of depth. A chain a few
//! thousand nodes deep overflows a thread's stack.
//!
//! [`IterativeDrop`] bounds that recursion. Frees nest normally up to
//! [`DIRECT_DEPTH`] levels, so a shallow graph is freed exactly as without it,
//! plus a thread-local counter per free. Below that, a handle that is the last
//! one to its node is queued instead of dropped, and the drop that started the
//! queue frees the queued handles one at a time in a loop, each with a fresh
//! budget, so only one free in `DIRECT_DEPTH` pays for the queue. However deep
//! the graph, the stack holds at most twice `DIRECT_DEPTH` nested frees.
//!
//! A reference type opts in through
//! [`Reference::strong_is_unique`](crate::Reference::strong_is_unique), which
//! says whether a drop would free the data right away, and
//! [`Reference::strong_into_deferred_drop`](crate::Reference::strong_into_deferred_drop),
//! which erases the handle for the queue. A drop that would not free, which is
//! most of them, does nothing extra. "Would free" is only probable: the check
//! is a plain read of the count, so it races with other threads. If it says last but another handle appears (a
//! hash-consing table upgrading its weak reference), the queued drop only
//! decrements. If it says shared but the other handles go away first, this drop
//! frees the node directly, and the node's children start drains of their own:
//! a race costs one level of recursion, never an unbounded number.
//!
//! The queue is a pointer to a vector on the draining frame's stack, held in a
//! `const` thread-local with no destructor, so it stays usable while other
//! thread-locals are being destroyed at thread exit.

use crate::reference::Reference;

/// A strong handle erased to what it takes to drop it later: up to two words of
/// handle, and the function that drops them.
///
/// Not `Send`: the handle may be one (such as `Rc`) that must be dropped on the
/// thread that queued it, and the queue is per thread.
pub struct DeferredDrop {
    words: [usize; 2],
    drop_fn: unsafe fn([usize; 2]),
    _not_send: std::marker::PhantomData<*const ()>,
}

impl DeferredDrop {
    /// Wrap a handle erased to `words`, to be dropped by `drop_fn(words)`.
    ///
    /// # Safety
    ///
    /// Calling `drop_fn(words)` exactly once, later on this thread, must drop
    /// the handle and be otherwise sound. The caller gives up the handle: it
    /// must not also be dropped some other way.
    pub unsafe fn new(words: [usize; 2], drop_fn: unsafe fn([usize; 2])) -> Self {
        Self {
            words,
            drop_fn,
            _not_send: std::marker::PhantomData,
        }
    }

    fn run(self) {
        // Safety: `new`'s contract.
        unsafe { (self.drop_fn)(self.words) }
    }
}

/// How many frees may nest by plain recursion before the rest are queued, and
/// again below each queued free.
///
/// Queueing costs an erasure, a push, a pop and an indirect call, which is
/// measurable on free-heavy workloads; nesting costs stack. With 32, a graph
/// no taller than that (any balanced tree of fewer than four billion nodes)
/// never reaches the queue, a deeper one queues one free in 32, and the stack
/// holds at most 64 nested frees: about 2 KiB in a release build, and within
/// a 64 KiB thread in a debug build.
pub const DIRECT_DEPTH: u32 = 32;

std::thread_local! {
    // Frees nested on this thread's stack right now, up to `DIRECT_DEPTH`.
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    // The draining frame's queue while a drain runs on this thread, else null.
    static QUEUE: std::cell::Cell<*mut Vec<DeferredDrop>> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Clears the queue pointer and restores the nesting depth when the drain
/// ends, including by unwinding.
///
/// Handles still queued when a drop panics are leaked, not dropped: dropping
/// them while unwinding could panic again and abort.
struct DrainGuard(u32);

impl Drop for DrainGuard {
    fn drop(&mut self) {
        let _ = QUEUE.try_with(|q| q.set(std::ptr::null_mut()));
        let _ = DEPTH.try_with(|d| d.set(self.0));
    }
}

/// Restores the nesting depth when a direct free ends, including by
/// unwinding.
struct DepthGuard(u32);

impl Drop for DepthGuard {
    fn drop(&mut self) {
        let _ = DEPTH.try_with(|d| d.set(self.0));
    }
}

/// Free `handle` now if fewer than [`DIRECT_DEPTH`] frees are nested on this
/// thread's stack; otherwise queue it, or start a drain if none is running.
///
/// Freeing a node can drop more handles, and each of them lands back here.
/// Within the budget they nest; past it they are queued, and the call that
/// started the drain frees them in a loop.
pub fn drop_iteratively(handle: DeferredDrop) {
    let depth = match DEPTH.try_with(|d| d.get()) {
        Ok(depth) => depth,
        // Not expected for a `const` thread-local without a destructor, but if
        // it is ever unavailable, dropping directly is still correct.
        Err(_) => return handle.run(),
    };
    if depth < DIRECT_DEPTH {
        DEPTH.with(|d| d.set(depth + 1));
        let guard = DepthGuard(depth);
        handle.run();
        drop(guard);
        return;
    }

    let queue = match QUEUE.try_with(|q| q.get()) {
        Ok(queue) => queue,
        // Not expected for a `const` thread-local without a destructor, but if
        // it is ever unavailable, dropping directly is still correct.
        Err(_) => return handle.run(),
    };
    if !queue.is_null() {
        // Safety: a non-null pointer is the live vector of the drain running
        // further up this thread's stack, which touches it only between runs.
        unsafe { (*queue).push(handle) };
        return;
    }

    let mut pending: Vec<DeferredDrop> = Vec::new();
    let queue: *mut Vec<DeferredDrop> = &mut pending;
    QUEUE.with(|q| q.set(queue));
    let guard = DrainGuard(depth);
    // Each queued handle is freed with a fresh budget: the next
    // `DIRECT_DEPTH` levels below it nest directly, and only the handles past
    // them are queued. So one free in `DIRECT_DEPTH` goes through the queue,
    // and the stack holds at most twice `DIRECT_DEPTH` nested frees.
    DEPTH.with(|d| d.set(0));
    handle.run();
    // Safety: `queue` points at `pending`, which outlives the loop; nothing
    // else borrows it while `pop` runs, and `run` reaches it only through the
    // thread-local, never while this borrow is live.
    while let Some(next) = unsafe { (*queue).pop() } {
        next.run();
    }
    drop(guard);
}

/// A strong handle whose drop does not recurse through the graph.
///
/// Wrap every handle stored inside a node's data (every parent-to-child edge)
/// in one of these. Dropping the last handle of a reference type that
/// implements [`Reference::strong_is_unique`] counts the nesting, and past
/// [`DIRECT_DEPTH`] goes through the drain loop; every other drop is a plain
/// drop, exactly as without the wrapper.
pub struct IterativeDrop<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    handle: std::mem::ManuallyDrop<R>,
    _data: std::marker::PhantomData<D>,
}

impl<D, R> IterativeDrop<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    #[inline]
    pub fn new(handle: R) -> Self {
        Self {
            handle: std::mem::ManuallyDrop::new(handle),
            _data: std::marker::PhantomData,
        }
    }

    #[inline]
    pub fn get(&self) -> &R {
        &self.handle
    }
}

impl<D, R> Drop for IterativeDrop<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    #[inline]
    fn drop(&mut self) {
        // Safety: `handle` is not used again; this is the only place it is
        // taken, and `drop` runs once.
        let handle = unsafe { std::mem::ManuallyDrop::take(&mut self.handle) };
        // A drop that frees nothing now cannot nest: drop it as usual. That is
        // every drop of a shared handle, and every drop of a type that keeps
        // the default `strong_is_unique`, which then costs nothing extra.
        if !R::strong_is_unique(&handle) {
            drop(handle);
            return;
        }
        // Within the budget, let the free nest by plain recursion, counting
        // the nesting: no erasure, no indirect call, no queue.
        let depth = DEPTH.try_with(|d| d.get()).unwrap_or(DIRECT_DEPTH);
        if depth < DIRECT_DEPTH {
            DEPTH.with(|d| d.set(depth + 1));
            let guard = DepthGuard(depth);
            drop(handle);
            drop(guard);
            return;
        }
        drop_iteratively(R::strong_into_deferred_drop(handle));
    }
}
