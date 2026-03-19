use crate::reference::*;
use crate::table::Table;
use crate::weak_entry::*;

pub struct TableVecSortedWeak<D, R, RW> {
    // Vector will be sorted by hash.
    // Entries with equivalent hash will be contiguous, but not sorted further.
    // We can do a binary search to find an entry with the given hash,
    // or the next position if not present.
    // The binary search will not necessarily find the first position with an equivalent hash, so
    // we need to do a linear scan in both directions while the hash is equivalent.
    v: Vec<WeakEntry<D, R, RW>>,
    // GC runs when v.len() reaches this threshold.
    // After each GC the threshold doubles to amortize cleanup cost.
    gc_threshold: usize,
}

impl<D, R, RW> TableVecSortedWeak<D, R, RW>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
{
    fn linear_search<Range>(&self, range: Range, hash: u64, data: &D) -> Option<R>
    where
        // Range is necessary because the reverse iterator adapter is a different type.
        Range: Iterator<Item = usize>,
    {
        for x in range {
            let m = self.v[x].get_existing_near(hash, data);
            match m {
                Err(()) => {
                    break;
                }
                Ok(y) => {
                    if let Some(p) = y {
                        return Some(p);
                    }
                }
            }
        }
        None
    }
    fn linear_search_up(&self, idx: usize, hash: u64, data: &D) -> Option<R> {
        self.linear_search(idx..self.v.len(), hash, data)
    }
    fn linear_search_down(&self, idx: usize, hash: u64, data: &D) -> Option<R> {
        self.linear_search((0..idx).rev(), hash, data)
    }
    fn linear_search_around(&self, idx: usize, hash: u64, data: &D) -> Option<R> {
        // Start with up, because up will include idx.
        self.linear_search_up(idx, hash, data)
            .or_else(|| self.linear_search_down(idx, hash, data))
    }
}

impl<D, R, RW> Default for TableVecSortedWeak<D, R, RW>
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

impl<D, R, RW> Table<D, R> for TableVecSortedWeak<D, R, RW>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
{
    fn get(&self, hash: u64, data: &D) -> Option<R> {
        // Binary search
        let result = self.v.binary_search_by(|probe| probe.hash_cmp(&hash));
        if let Ok(idx) = result {
            // Linear search up and down
            if let Some(p) = self.linear_search_around(idx, hash, data) {
                return Some(p);
            }
        }
        None
    }

    fn get_or_insert<CF>(&mut self, hash: u64, mut data: D, creation_meta: CF) -> R
    where
        D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
        CF: FnOnce(&mut D),
    {
        // Binary search for an existing entry or the correct insertion point.
        let result = self.v.binary_search_by(|probe| probe.hash_cmp(&hash));
        let insert_index = match result {
            Ok(idx) => {
                // Linear search up and down for a live matching entry.
                if let Some(p) = self.linear_search_around(idx, hash, &data) {
                    return p;
                }
                idx
            }
            Err(idx) => idx,
        };

        // Remove dead entries when the vec reaches the GC threshold.
        // retain preserves sorted order. After cleanup double the threshold
        // so that GC is amortized O(1) per insert.
        let insert_index = if self.v.len() >= self.gc_threshold {
            self.v.retain(|e| e.is_alive());
            self.gc_threshold = (self.v.len() * 2).max(16);
            match self.v.binary_search_by(|probe| probe.hash_cmp(&hash)) {
                Ok(idx) => idx,
                Err(idx) => idx,
            }
        } else {
            insert_index
        };

        creation_meta(&mut data);
        let obj = R::new(data);
        let weak = RW::weak_downgrade(&obj);
        self.v.insert(insert_index, WeakEntry::new(hash, weak));

        obj
    }
}
