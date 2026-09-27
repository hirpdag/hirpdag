//! Interfaces for reference handles.
//!
//! ReferenceWeak is separate because it is conceivable to implement hashconsing without it.

/// Strong reference handle type.
pub trait Reference<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    /// Move the data into a new strong reference.
    fn new(data: D) -> Self;

    /// Borrow the referenced data.
    fn strong_deref(ptr: &Self) -> &D;

    /// Clone the reference handle. The new resulting handle will refer to the same data.
    fn strong_clone(ptr: &Self) -> Self;

    /// Check if two reference handles refer to the same data.
    fn strong_ptr_eq(a: &Self, b: &Self) -> bool;

    /// Whether dropping this handle would (probably) free the data right
    /// away, so its drop is worth routing through [`drop_queue`].
    ///
    /// True for the last strong handle of a type that frees on the spot. The
    /// check may race with other threads in either direction; [`drop_queue`]
    /// explains why that is harmless.
    ///
    /// The default says no, so a reference type that does not implement this
    /// drops exactly as it would without
    /// [`IterativeDrop`](drop_queue::IterativeDrop), recursion included. A type
    /// that never frees on drop (`RefLeak`, and `RefTlc`, which frees at its
    /// next flush) should keep the default: its drops then cost nothing extra.
    #[inline]
    fn strong_is_unique(_ptr: &Self) -> bool {
        false
    }

    /// Erase this handle to a [`DeferredDrop`] that drops it later, on this
    /// thread. Only called for a handle [`strong_is_unique`] said yes to, and
    /// only for frees nested too deep to run on the spot.
    ///
    /// The default boxes the handle. Types with a cheaper erased form, such as
    /// a single pointer, should override it.
    ///
    /// [`DeferredDrop`]: drop_queue::DeferredDrop
    /// [`strong_is_unique`]: Reference::strong_is_unique
    fn strong_into_deferred_drop(ptr: Self) -> drop_queue::DeferredDrop
    where
        Self: Sized,
    {
        unsafe fn drop_boxed<R>(words: [usize; 2]) {
            drop(Box::from_raw(words[0] as *mut R));
        }
        let raw = Box::into_raw(Box::new(ptr)) as usize;
        // Safety: `drop_boxed` turns the raw pointer back into the box
        // `into_raw` consumed, once.
        unsafe { drop_queue::DeferredDrop::new([raw, 0], drop_boxed::<Self>) }
    }
}

/// Weak reference handle type.
///
/// For HashconsingRef implementations which support both strong and weak refs.
pub trait ReferenceWeak<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    /// Get a strong reference handle from a weak reference handle.
    ///
    /// This may fail (returning None) if there is no strong reference in existance.
    fn weak_upgrade(ptr: &Self) -> std::option::Option<R>;

    /// Get a weak reference handle from a strong reference handle.
    fn weak_downgrade(ptr: &R) -> Self;
}

// Reference-counting implementations.
pub(crate) mod arc;
pub(crate) mod leak;
pub(crate) mod rc;
pub(crate) mod sepcount;
pub(crate) mod tlc;

// Dropping handles without recursing through the graph.
pub mod drop_queue;
