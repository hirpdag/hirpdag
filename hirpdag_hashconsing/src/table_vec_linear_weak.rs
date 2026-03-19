use crate::reference::*;
use crate::table::Table;
use crate::weak_entry::*;

pub struct TableVecLinearWeak<D, R, RW> {
    v: std::vec::Vec<WeakEntry<D, R, RW>>,
    // GC runs when v.len() reaches this threshold.
    // After each GC the threshold doubles to amortize cleanup cost.
    gc_threshold: usize,
}

impl<D, R, RW> Default for TableVecLinearWeak<D, R, RW>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
{
    fn default() -> Self {
        Self {
            v: Vec::new(),
            gc_threshold: 16,
        }
    }
}

impl<D, R, RW> Table<D, R> for TableVecLinearWeak<D, R, RW>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
{
    fn get(&self, hash: u64, data: &D) -> Option<R> {
        // Linear search
        self.v.iter().filter_map(|x| x.get(hash, data)).next()
    }

    fn get_or_insert<CF>(&mut self, hash: u64, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        if let Some(existing_obj) = self.get(hash, &data) {
            return existing_obj;
        }

        // Remove dead entries when the vec reaches the GC threshold.
        // After cleanup, double the threshold so that GC is amortized O(1) per insert.
        if self.v.len() >= self.gc_threshold {
            self.v.retain(|e| e.is_alive());
            self.gc_threshold = (self.v.len() * 2).max(16);
        }

        creation_meta(&mut data);
        let obj = R::new(data);
        let weak = RW::weak_downgrade(&obj);

        self.v.push(WeakEntry::new(hash, weak));

        obj
    }
}
