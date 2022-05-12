//! Reference counting implemented by std::rc::Rc.
//!
//! This adapts Rust's Rc to the hirpdag hashconsing reference interface.

use crate::reference::*;

pub type RefRc<D> = std::rc::Rc<D>;

impl<D> Reference<D> for RefRc<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn new(data: D) -> Self {
        std::rc::Rc::new(data)
    }

    fn strong_deref(ptr: &Self) -> &D {
        &*ptr
    }

    fn strong_clone(ptr: &Self) -> Self {
        ptr.clone()
    }

    fn strong_ptr_eq(a: &Self, b: &Self) -> bool {
        std::rc::Rc::<D>::ptr_eq(a, b)
    }
}

pub type RefRcWeak<D> = std::rc::Weak<D>;

impl<D> ReferenceWeak<D, RefRc<D>> for RefRcWeak<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn weak_upgrade(ptr: &Self) -> std::option::Option<RefRc<D>> {
        ptr.upgrade()
    }

    fn weak_downgrade(ptr: &RefRc<D>) -> Self {
        std::rc::Rc::<D>::downgrade(ptr)
    }
}
