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
            let m = self.v[x].get_existing_near(hash, &data);
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
        Self { v: Vec::new() }
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
        match result {
            Ok(idx) => {
                // Linear search up and down
                if let Some(p) = self.linear_search_around(idx, hash, data) {
                    return Some(p);
                }
            }
            Err(_) => {}
        };
        None
    }

    fn get_or_insert<CF>(&mut self, hash: u64, mut data: D, creation_meta: CF) -> R
    where
        D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
        CF: FnOnce(&mut D),
    {
        // Binary search
        let result = self.v.binary_search_by(|probe| probe.hash_cmp(&hash));
        let index = match result {
            Ok(idx) => {
                // Linear search up and down
                if let Some(p) = self.linear_search_around(idx, hash, &data) {
                    return p;
                }
                idx
            }
            Err(idx) => idx,
        };

        creation_meta(&mut data);
        let obj = R::new(data);
        let weak = RW::weak_downgrade(&obj);
        self.v.insert(index, WeakEntry::new(hash, weak));

        return obj;
    }
}
