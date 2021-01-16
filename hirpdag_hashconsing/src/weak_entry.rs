use crate::reference::*;

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
            hash: hash,
            weak: weak,

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

    pub fn get(&self, hash: u64, data: &T) -> Option<R> {
        if self.hash == hash {
            return self.get_data(data);
        }
        None
    }

    pub fn get_existing_near(&self, hash: u64, data: &T) -> Result<Option<R>, ()> {
        if self.hash == hash {
            return Ok(self.get_data(data));
        }
        Err(())
    }

    pub fn hash_cmp(&self, hash: &u64) -> std::cmp::Ordering {
        self.hash.cmp(hash)
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
