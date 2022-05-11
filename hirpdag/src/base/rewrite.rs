pub trait HirpdagRewritable<T> {
    fn hirpdag_rewrite(&self, rewriter: &T) -> Self;
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
