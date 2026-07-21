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

    /// Clone a weak reference handle. The new handle refers to the same data and
    /// keeps the weak count alive independently of the original.
    ///
    /// Unlike [`weak_downgrade`](Self::weak_downgrade) this does not require a
    /// live strong reference: a weak handle can be duplicated whether or not its
    /// referent is still alive. This is what lets a weak reference be stored as
    /// the *value* of a concurrent map (see
    /// [`NonPurgingTable`](crate::NonPurgingTable)), which must be able to clone
    /// values out on lookup.
    fn weak_clone(ptr: &Self) -> Self;

    /// A stable identity for this weak handle, suitable for hashing and equality.
    ///
    /// The returned value is derived from the address of the referent allocation
    /// and stays constant for the whole life of the weak handle — crucially, it
    /// does *not* change when the referent is dropped. This makes it sound to use
    /// as a `Hash`/`Eq` key (the [`WeakEntryStrong`](crate::WeakEntryStrong) weak
    /// holder relies on this), whereas hashing the referent's data would be
    /// unstable once the data dies.
    ///
    /// Under hash-consing, pointer identity coincides with structural identity —
    /// two structurally equal nodes share one allocation — so this is also the
    /// correct notion of equality for interned values.
    fn weak_ptr_id(ptr: &Self) -> usize;
}

// Reference-counting implementations.
pub(crate) mod arc;
pub(crate) mod leak;
pub(crate) mod rc;
pub(crate) mod sepcount;
pub(crate) mod tlc;
