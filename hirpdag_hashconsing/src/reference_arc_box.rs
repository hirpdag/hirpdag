//! Split reference counting: Arc<Box<D>>.
//!
//! This separates the allocation for reference counts (inside the Arc) from the
//! allocation for data (inside the Box). In standard Arc<D>, both the counts and
//! the data live in a single allocation, so writes to the reference count can
//! invalidate cache lines that also contain the data.
//!
//! With Arc<Box<D>>, the Arc's allocation only holds the counts and a Box pointer,
//! while the data lives in a separate allocation. Cache lines holding data are
//! never dirtied by reference count updates.
//!
//! Trade-off: an extra pointer indirection is required when accessing data.
//!
//! See: https://users.rust-lang.org/t/why-does-arc-use-one-contiguous-allocation-for-data-and-counters/113319
//! See: https://ddanilov.me/shared-ptr-is-evil/

use crate::reference::*;

pub struct RefArcBox<D>(std::sync::Arc<std::boxed::Box<D>>);

impl<D> Reference<D> for RefArcBox<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn new(data: D) -> Self {
        Self(std::sync::Arc::new(std::boxed::Box::new(data)))
    }

    fn strong_deref(ptr: &Self) -> &D {
        &**ptr.0
    }

    fn strong_clone(ptr: &Self) -> Self {
        Self(ptr.0.clone())
    }

    fn strong_ptr_eq(a: &Self, b: &Self) -> bool {
        std::sync::Arc::ptr_eq(&a.0, &b.0)
    }
}

pub struct RefArcBoxWeak<D>(std::sync::Weak<std::boxed::Box<D>>);

impl<D> ReferenceWeak<D, RefArcBox<D>> for RefArcBoxWeak<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn weak_upgrade(ptr: &Self) -> std::option::Option<RefArcBox<D>> {
        ptr.0.upgrade().map(RefArcBox)
    }

    fn weak_downgrade(ptr: &RefArcBox<D>) -> Self {
        Self(std::sync::Arc::downgrade(&ptr.0))
    }
}
