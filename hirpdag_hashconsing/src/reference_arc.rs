//! Reference counting implemented by std::sync::Arc.
//!
//! This adapts Rust's Arc to the hirpdag hashconsing reference interface.

use crate::reference::*;

pub type RefArc<D> = std::sync::Arc<D>;

impl<D> Reference<D> for RefArc<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn new(data: D) -> Self {
        std::sync::Arc::new(data)
    }

    fn strong_deref(ptr: &Self) -> &D {
        &*ptr
    }

    fn strong_clone(ptr: &Self) -> Self {
        ptr.clone()
    }

    fn strong_ptr_eq(a: &Self, b: &Self) -> bool {
        std::sync::Arc::<D>::ptr_eq(a, b)
    }
}

pub type RefArcWeak<D> = std::sync::Weak<D>;

impl<D> ReferenceWeak<D, RefArc<D>> for RefArcWeak<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn weak_upgrade(ptr: &Self) -> std::option::Option<RefArc<D>> {
        ptr.upgrade()
    }

    fn weak_downgrade(ptr: &RefArc<D>) -> Self {
        std::sync::Arc::<D>::downgrade(ptr)
    }
}
