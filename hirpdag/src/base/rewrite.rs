/// Implemented by types that can appear as fields in `#[hirpdag]` structs to support rewriting.
///
/// The macro-generated `default_rewrite` for each node type calls `hirpdag_rewrite` on every
/// field, then reconstructs the node.  Leaf types (numbers, strings) clone themselves;
/// child `HirpdagRef` fields delegate to the rewriter.
pub trait HirpdagRewritable<T> {
    /// Apply `rewriter` to this value and return the (potentially new) transformed value.
    fn hirpdag_rewrite(&self, rewriter: &T) -> Self;
}

/// Supplies the memoization cache a rewriter uses, generic over the module's
/// `HirpdagRewriteCache` type `C`.
///
/// Every `HirpdagRewriter` requires this (it is a supertrait), which is what
/// makes memoization the default: the generated `HirpdagRewriter` trait reads
/// the cache from here.
///
/// A rewriter that owns a `HirpdagRewriteCache` field gets this for free with
/// `#[derive(HirpdagMemoize)]` — the derive finds the field and returns it. To
/// disable memoization, implement it by hand returning `None` (no cache field
/// needed).
pub trait HirpdagMemoize<C> {
    /// The cache to memoize through, or `None` to disable memoization.
    fn hirpdag_memoize_cache(&self) -> Option<&C>;
}

use crate::base::basic_traits::IsNumber;
impl<T, P: IsNumber + Clone> HirpdagRewritable<T> for P {
    fn hirpdag_rewrite(&self, _rewriter: &T) -> Self {
        self.clone()
    }
}

impl<T> HirpdagRewritable<T> for String {
    fn hirpdag_rewrite(&self, _rewriter: &T) -> Self {
        self.clone()
    }
}

impl<T, D: HirpdagRewritable<T>> HirpdagRewritable<T> for Option<D> {
    fn hirpdag_rewrite(&self, rewriter: &T) -> Option<D> {
        self.as_ref().map(|ii| ii.hirpdag_rewrite(rewriter))
    }
}

impl<T, D: HirpdagRewritable<T>> HirpdagRewritable<T> for Vec<D> {
    fn hirpdag_rewrite(&self, rewriter: &T) -> Vec<D> {
        self.iter().map(|m| m.hirpdag_rewrite(rewriter)).collect()
    }
}
