use crate::reference::*;

/// The storage unit inside vector-backed hash-consing tables.
///
/// Caches the precomputed hash alongside a weak reference to the interned value.  The hash
/// enables cheap filtering before the equality check, and the weak reference allows the entry
/// to be evicted (via [`is_alive`](Self::is_alive)) once all strong references are dropped.
pub struct WeakEntry<T, R, RW> {
    hash: u64,
    weak: RW,

    phantom_t: std::marker::PhantomData<T>,
    phantom_r: std::marker::PhantomData<R>,
}

#[allow(dead_code)]
impl<T, R, RW> WeakEntry<T, R, RW>
where
    T: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<T>,
    RW: ReferenceWeak<T, R>,
{
    pub fn new(hash: u64, weak: RW) -> Self {
        Self {
            hash,
            weak,

            phantom_t: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }

    fn get_data(&self, data: &T) -> Option<R> {
        if let Some(up) = RW::weak_upgrade(&self.weak) {
            if *R::strong_deref(&up) == *data {
                return Some(up);
            }
        }
        None
    }

    /// Probe this entry: returns `Some(strong)` if the hash matches and the data is equal.
    pub fn get(&self, hash: u64, data: &T) -> Option<R> {
        if self.hash == hash {
            return self.get_data(data);
        }
        None
    }

    /// Probe this entry for use in sorted tables.
    ///
    /// Returns `Ok(Some(strong))` on a hit, `Ok(None)` if the hash matches but data differs
    /// (keep scanning neighbours), or `Err(())` if the hash differs (stop the linear scan —
    /// no further entries in this direction can match).
    pub fn get_existing_near(&self, hash: u64, data: &T) -> Result<Option<R>, ()> {
        if self.hash == hash {
            return Ok(self.get_data(data));
        }
        Err(())
    }

    pub fn hash_cmp(&self, hash: &u64) -> std::cmp::Ordering {
        self.hash.cmp(hash)
    }

    // Returns true if the weak reference is still live (the data hasn't been dropped).
    pub fn is_alive(&self) -> bool {
        RW::weak_upgrade(&self.weak).is_some()
    }
}

impl<T, R, RW> std::cmp::PartialOrd for WeakEntry<T, R, RW>
where
    T: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<T>,
    RW: ReferenceWeak<T, R>,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let hash_cmp = self.hash.cmp(&other.hash);
        match hash_cmp {
            std::cmp::Ordering::Equal => None,
            _ => Some(hash_cmp),
        }
    }
}

impl<T, R, RW> std::cmp::PartialEq for WeakEntry<T, R, RW>
where
    T: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<T>,
    RW: ReferenceWeak<T, R>,
{
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}
