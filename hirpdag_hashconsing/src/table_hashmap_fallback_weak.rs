use crate::reference::*;
use crate::table::Table;

/// A hasher that passes `u64` keys through without re-hashing them.
///
/// The keys stored in `TableHashmapFallbackWeak`'s inner `HashMap` are already
/// 64-bit content hashes, so running them through a second hasher (SipHash by
/// default) is wasted work.  This identity hasher skips that second pass.
#[derive(Default)]
struct U64IdentityHasher(u64);

impl std::hash::Hasher for U64IdentityHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        // Fall back to a basic mixing for non-u64 writes (shouldn't happen in practice).
        for &b in bytes {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(b as u64);
        }
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = i;
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

type BuildU64IdentityHasher = std::hash::BuildHasherDefault<U64IdentityHasher>;

pub struct TableHashmapFallbackWeak<
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
    T: Table<D, R> + Default,
> {
    m: std::collections::HashMap<u64, RW, BuildU64IdentityHasher>,
    // If the map slot for this hash is taken, use the vector.
    // This is a giant inefficient hack to at least be mostly correct.
    fallback: T,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R, RW, T> Default for TableHashmapFallbackWeak<D, R, RW, T>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
    T: Table<D, R> + Default,
{
    fn default() -> Self {
        Self {
            m: std::collections::HashMap::default(),
            fallback: T::default(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}

impl<D, R, RW, T> Table<D, R> for TableHashmapFallbackWeak<D, R, RW, T>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R>,
    T: Table<D, R> + Default,
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
