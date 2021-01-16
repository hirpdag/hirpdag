//! No reference counting.
//!
//! This implementation does not track reference counts, and never frees anything.
//!
//! This is useful as a comparison to understand the cost of reference counting.

use crate::reference::*;

pub type RefLeak<D> = std::mem::ManuallyDrop<std::boxed::Box<D>>;

fn get_raw_pointer<D>(ptr: &RefLeak<D>) -> *const D {
    &***ptr as *const D
}

fn duplicate<D>(ptr: &RefLeak<D>) -> RefLeak<D> {
    let ptr_raw = get_raw_pointer(ptr);
    // We are using unsafe to create multiple Box<D> which refer to the same memory.
    // This will be safe because:
    // - All instances are immutable.
    // - All instances are under ManuallyDrop, and never manually dropped.
    unsafe { std::mem::ManuallyDrop::new(std::boxed::Box::from_raw(ptr_raw as *mut _)) }
}

impl<D> Reference<D> for RefLeak<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn new(data: D) -> Self {
        std::mem::ManuallyDrop::new(std::boxed::Box::new(data))
    }

    fn strong_deref(ptr: &Self) -> &D {
        &***ptr
    }

    fn strong_clone(ptr: &Self) -> Self {
        duplicate(ptr)
    }

    fn strong_ptr_eq(a: &Self, b: &Self) -> bool {
        get_raw_pointer(a) == get_raw_pointer(b)
    }
}

pub type RefLeakWeak<D> = RefLeak<D>;

impl<D> ReferenceWeak<D, RefLeak<D>> for RefLeakWeak<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn weak_upgrade(ptr: &Self) -> std::option::Option<RefLeak<D>> {
        Some(duplicate(ptr))
    }

    fn weak_downgrade(ptr: &RefLeak<D>) -> Self {
        duplicate(ptr)
    }
}
