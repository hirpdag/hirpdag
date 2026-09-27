use crate::reference::*;

/// A weak reference wrapped so it can be stored as the *value* of a concurrent
/// backend's [`NonPurgingTable`](crate::NonPurgingTable) view.
///
/// The concurrent third-party backends store their values in a concurrent map
/// and hand them back by clone on lookup, so a stored value must be `Clone` (and
/// `Send + Sync` to cross threads; some backends additionally need `Hash + Eq`).
/// A bare [`ReferenceWeak`] handle offers none of these uniformly. `WeakEntryStrong`
/// supplies them:
///
/// * `Clone` duplicates the weak handle via [`ReferenceWeak::weak_clone`] (taking
///   an extra weak count) rather than requiring the referent to be live.
/// * `Hash` / `Eq` use [`ReferenceWeak::weak_ptr_id`] — a *stable* pointer
///   identity that does not change when the referent dies. Hashing the referent's
///   data instead would be unsound: the value's hash would change the moment its
///   node was dropped, corrupting the containing map. Under hash-consing pointer
///   identity is also the correct notion of equality (equal nodes share one
///   allocation).
/// * `Send + Sync` are derived automatically, and hold whenever the underlying
///   weak type is `Send + Sync` (all the thread-safe reference backends).
///
/// This is what lets [`TableAmortizedPurge`](crate::TableAmortizedPurge) reuse a
/// strong-only concurrent map as a purging weak-key hash-consing table.
pub struct WeakEntryStrong<D, R, WR> {
    weak: WR,

    // `fn() -> (D, R)` is unconditionally `Send + Sync` and does not add drop
    // glue, so the wrapper's auto traits depend only on `WR`.
    phantom: std::marker::PhantomData<fn() -> (D, R)>,
}

impl<D, R, WR> WeakEntryStrong<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
{
    /// Wrap a fresh weak downgrade of `strong`.
    pub fn downgrade(strong: &R) -> Self {
        Self {
            weak: WR::weak_downgrade(strong),
            phantom: std::marker::PhantomData,
        }
    }

    /// Try to recover a strong reference, or `None` if the referent has died.
    pub fn upgrade(&self) -> Option<R> {
        WR::weak_upgrade(&self.weak)
    }

    /// Whether the referent is still alive.
    pub fn is_alive(&self) -> bool {
        WR::weak_upgrade(&self.weak).is_some()
    }
}

impl<D, R, WR> Clone for WeakEntryStrong<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
{
    fn clone(&self) -> Self {
        Self {
            weak: WR::weak_clone(&self.weak),
            phantom: std::marker::PhantomData,
        }
    }
}

impl<D, R, WR> std::hash::Hash for WeakEntryStrong<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        WR::weak_ptr_id(&self.weak).hash(state);
    }
}

impl<D, R, WR> std::cmp::PartialEq for WeakEntryStrong<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
{
    fn eq(&self, other: &Self) -> bool {
        WR::weak_ptr_id(&self.weak) == WR::weak_ptr_id(&other.weak)
    }
}

impl<D, R, WR> std::cmp::Eq for WeakEntryStrong<D, R, WR>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
{
}
