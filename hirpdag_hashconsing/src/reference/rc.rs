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
        ptr
    }

    fn strong_clone(ptr: &Self) -> Self {
        ptr.clone()
    }

    fn strong_ptr_eq(a: &Self, b: &Self) -> bool {
        std::rc::Rc::<D>::ptr_eq(a, b)
    }

    #[inline]
    fn strong_is_unique(ptr: &Self) -> bool {
        std::rc::Rc::strong_count(ptr) == 1
    }

    fn strong_into_deferred_drop(ptr: Self) -> drop_queue::DeferredDrop {
        unsafe fn drop_rc<D>(words: [usize; 2]) {
            drop(std::rc::Rc::from_raw(words[0] as *const D));
        }
        let raw = std::rc::Rc::into_raw(ptr) as usize;
        // Safety: `drop_rc` turns the raw pointer back into the handle
        // `into_raw` consumed, once, on this thread (`DeferredDrop` is not
        // `Send`).
        unsafe { drop_queue::DeferredDrop::new([raw, 0], drop_rc::<D>) }
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
