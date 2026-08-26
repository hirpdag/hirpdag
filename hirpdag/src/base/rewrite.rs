/// Implemented by types that can appear as fields in `#[hirpdag]` structs to support rewriting.
///
/// The macro-generated `default_rewrite` for each node type calls `hirpdag_rewrite` on every
/// field, then reconstructs the node.  Leaf types (numbers, strings) clone themselves;
/// child `HirpdagRef` fields delegate to the driver, which dispatches to the rewriter's rule
/// for that node type (and may serve the node from a memo cache instead).
///
/// `T` is the recursion driver (the generated `HirpdagRewriteDriver`), not the rewriter
/// itself, so the same field types work under every traversal strategy.
pub trait HirpdagRewritable<T> {
    /// Apply `driver` to this value and return the (potentially new) transformed value.
    fn hirpdag_rewrite(&self, driver: &T) -> Self;
}

use crate::base::basic_traits::IsNumber;
impl<T, P: IsNumber + Clone> HirpdagRewritable<T> for P {
    fn hirpdag_rewrite(&self, _driver: &T) -> Self {
        self.clone()
    }
}

impl<T> HirpdagRewritable<T> for String {
    fn hirpdag_rewrite(&self, _driver: &T) -> Self {
        self.clone()
    }
}

impl<T, D: HirpdagRewritable<T>> HirpdagRewritable<T> for Option<D> {
    fn hirpdag_rewrite(&self, driver: &T) -> Option<D> {
        self.as_ref().map(|ii| ii.hirpdag_rewrite(driver))
    }
}

impl<T, D: HirpdagRewritable<T>> HirpdagRewritable<T> for Vec<D> {
    fn hirpdag_rewrite(&self, driver: &T) -> Vec<D> {
        self.iter().map(|m| m.hirpdag_rewrite(driver)).collect()
    }
}
