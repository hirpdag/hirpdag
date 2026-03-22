//! Cache-line separated reference counting via alignment padding.
//!
//! Standard Arc<D> lays out memory as:
//!   [strong_count | weak_count | data...]
//!
//! If the data is small, the counts and data may share a cache line. Every
//! reference count update then dirty-pings the cache line holding the data,
//! causing unnecessary traffic to readers of the data on other cores.
//!
//! By wrapping D in CacheLineAligned<D> (repr(align(64))), the compiler is
//! forced to insert padding between the counts and the start of the data field
//! so that the data begins at a 64-byte cache line boundary:
//!
//!   [strong | weak | ----padding----][data...]
//!    ^--- cache line 0 ---^           ^--- cache line 1+ ---^
//!
//! This guarantees that reads of data never observe false-sharing with
//! reference count writes.
//!
//! Trade-off: wastes up to 48 bytes of padding per allocation; slightly larger
//! total footprint than Arc<D> for small types.
//!
//! See: https://ddanilov.me/shared-ptr-is-evil/

use crate::reference::*;

const CACHE_LINE_SIZE: usize = 64;

#[repr(align(64))]
struct CacheLineAligned<D>(D);

pub struct RefArcCacheLine<D>(std::sync::Arc<CacheLineAligned<D>>);

impl<D> Reference<D> for RefArcCacheLine<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn new(data: D) -> Self {
        let _ = CACHE_LINE_SIZE; // document the assumed cache line size
        Self(std::sync::Arc::new(CacheLineAligned(data)))
    }

    fn strong_deref(ptr: &Self) -> &D {
        &ptr.0 .0
    }

    fn strong_clone(ptr: &Self) -> Self {
        Self(ptr.0.clone())
    }

    fn strong_ptr_eq(a: &Self, b: &Self) -> bool {
        std::sync::Arc::ptr_eq(&a.0, &b.0)
    }
}

pub struct RefArcCacheLineWeak<D>(std::sync::Weak<CacheLineAligned<D>>);

impl<D> ReferenceWeak<D, RefArcCacheLine<D>> for RefArcCacheLineWeak<D>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
{
    fn weak_upgrade(ptr: &Self) -> std::option::Option<RefArcCacheLine<D>> {
        ptr.0.upgrade().map(RefArcCacheLine)
    }

    fn weak_downgrade(ptr: &RefArcCacheLine<D>) -> Self {
        Self(std::sync::Arc::downgrade(&ptr.0))
    }
}
