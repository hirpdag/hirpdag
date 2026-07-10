use crate::reference::*;
use crate::table::ThreadUnsafeTable;

/// Purge dead map entries when the map grows past twice its size at the last
/// purge (but not below this floor). Amortized O(1) per insertion.
const PURGE_LEN_MIN: usize = 64;

pub struct TableHashmapFallbackWeak<
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R> + Default,
> {
    m: std::collections::HashMap<u64, RW>,
    // If the map slot for this hash is taken, use the vector.
    // This is a giant inefficient hack to at least be mostly correct.
    fallback: T,
    // Purge dead weak entries when the map reaches this size. Without
    // purging, workloads which re-create equivalent nodes under fresh hashes
    // (hashes include child creation IDs, which change when a dead node is
    // re-interned) grow the map without bound, and every dead weak entry
    // also pins its referent's count allocation.
    purge_at_len: usize,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R, RW, T> Default for TableHashmapFallbackWeak<D, R, RW, T>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R> + Default,
{
    fn default() -> Self {
        Self {
            m: std::collections::HashMap::default(),
            fallback: T::default(),
            purge_at_len: PURGE_LEN_MIN,

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}

impl<D, R, RW, T> ThreadUnsafeTable<D, R> for TableHashmapFallbackWeak<D, R, RW, T>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R> + Default,
{
    fn get(&self, hash: u64, data: &D) -> Option<R> {
        if let Some(v) = self.m.get(&hash) {
            if let Some(up) = RW::weak_upgrade(v) {
                if *R::strong_deref(&up) == *data {
                    return Some(up);
                }
            }
        }
        self.fallback.get(hash, data)
    }

    fn get_or_insert<CF>(&mut self, hash: u64, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        use std::collections::hash_map::Entry;
        let mut has_vacancy = false;
        match self.m.entry(hash) {
            Entry::Vacant(_ev) => {
                has_vacancy = true;
            }
            Entry::Occupied(eo) => {
                if let Some(up) = RW::weak_upgrade(eo.get()) {
                    if *R::strong_deref(&up) == data {
                        return up;
                    }
                } else {
                    has_vacancy = true;
                }
            }
        }

        if has_vacancy {
            let fallback_obj = self.fallback.get(hash, &data);
            if let Some(fobj) = fallback_obj {
                return fobj;
            }

            creation_meta(&mut data);
            let obj = R::new(data);
            let weak = RW::weak_downgrade(&obj);
            if self.m.len() >= self.purge_at_len {
                self.m.retain(|_, w| RW::weak_upgrade(w).is_some());
                self.purge_at_len = std::cmp::max(PURGE_LEN_MIN, self.m.len() * 2);
            }
            let entry = self.m.entry(hash);
            match entry {
                Entry::Vacant(ev) => {
                    ev.insert(weak);
                }
                Entry::Occupied(mut eo) => {
                    assert!(RW::weak_upgrade(eo.get()).is_none());
                    *eo.get_mut() = weak;
                }
            }
            obj
        } else {
            self.fallback.get_or_insert(hash, data, creation_meta)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestData;
    use crate::{RefArc, RefArcWeak, TableVecLinearWeak};

    type R = RefArc<TestData>;
    type RW = RefArcWeak<TestData>;
    type Fallback = TableVecLinearWeak<TestData, R, RW>;
    type T = TableHashmapFallbackWeak<TestData, R, RW, Fallback>;

    /// Mimic the benchmark pattern which triggered the leak: every iteration
    /// re-interns a fresh set of nodes after all strong references from the
    /// previous iteration have died. Because node hashes include child
    /// creation IDs, each cycle uses fresh hashes, so a dead entry is never
    /// overwritten and would accumulate forever without purging. The map must
    /// stay bounded rather than growing with the iteration count.
    #[test]
    fn dead_entries_are_purged_across_intern_cycles() {
        let mut table = T::default();

        let n = 1000u64;
        let mut len_after_warmup = 0;
        for iteration in 0..50u64 {
            let mut live: Vec<R> = Vec::with_capacity(n as usize);
            for k in 0..n {
                // Unique hash per (iteration, k) so re-interned nodes never
                // reuse a previous cycle's slot, matching the creation-ID
                // dependent hashing that caused the leak.
                let hash = iteration * n + k;
                let data = TestData::new(k as i32, 0, "purge_cycle".to_string());
                live.push(table.get_or_insert(hash, data, |_| {}));
            }
            drop(live);
            let len = table.m.len();
            if iteration == 5 {
                len_after_warmup = len;
            } else if iteration > 5 {
                assert!(
                    len <= len_after_warmup,
                    "map grew from {} to {} entries by iteration {}: dead entries are leaking",
                    len_after_warmup,
                    len,
                    iteration
                );
            }
        }
    }
}
