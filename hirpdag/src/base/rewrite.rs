/// Implemented by types that can appear as fields in `#[hirpdag]` structs to support rewriting.
///
/// The macro-generated `default_rewrite` for each node type calls `hirpdag_rewrite` on every
/// field, then reconstructs the node.  Leaves ([`HirpdagLeaf`](crate::base::HirpdagLeaf))
/// clone themselves; child `HirpdagRef` fields delegate to the driver, which dispatches to
/// the rewriter's rule for that node type (and may serve the node from a memo cache
/// instead).  The leaf and container implementations are in [`field`](crate::base::field).
///
/// `T` is the recursion driver (the generated `HirpdagRewriteDriver`), not the rewriter
/// itself, so the same field types work under every traversal strategy.
pub trait HirpdagRewritable<T> {
    /// Apply `driver` to this value and return the (potentially new) transformed value.
    fn hirpdag_rewrite(&self, driver: &T) -> Self;
}
