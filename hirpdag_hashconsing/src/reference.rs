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
}
