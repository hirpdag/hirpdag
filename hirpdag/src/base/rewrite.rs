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

/// The stack the generated drivers keep free before each rule call.
///
/// A rule's own frames, and whatever it calls before it next recurses through
/// the driver, have to fit in this.
pub const HIRPDAG_STACK_RED_ZONE: usize = 128 * 1024;

/// The size of each stack segment allocated when the red zone is reached.
pub const HIRPDAG_STACK_SEGMENT: usize = 2 * 1024 * 1024;

/// Run `f` with at least [`HIRPDAG_STACK_RED_ZONE`] bytes of stack available,
/// continuing on a fresh heap-allocated segment of [`HIRPDAG_STACK_SEGMENT`]
/// bytes if the current stack is running low.
///
/// A rewrite rule recurses through the driver inside its own body, so the
/// stack a rewrite needs grows with the depth of the graph. The generated
/// `HirpdagRewriteDirect` and `HirpdagRewriteMemoized` call this around every
/// rule, which lets a rewrite of any depth run on any thread. A hand-written
/// `HirpdagRewriteDriver` should do the same. See
/// `docs/adr/0013-stacker-for-rewrite.md`.
#[inline]
pub fn hirpdag_ensure_stack<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(HIRPDAG_STACK_RED_ZONE, HIRPDAG_STACK_SEGMENT, f)
}
