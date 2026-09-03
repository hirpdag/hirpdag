use crate::reference::*;
use crate::table::*;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

pub struct TableSharedMutex<D, R, T, HB = DefaultHasher>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    HB: std::hash::BuildHasher + Default + Clone,
{
    inner: std::sync::Mutex<T>,
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R, T, HB> TableSharedMutex<D, R, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    HB: std::hash::BuildHasher + Default + Clone,
{
    /// An empty table wrapping a freshly built inner table, hashing with
    /// `hash_builder`.
    ///
    /// [`Default`] covers the usual case; this is for a hasher carrying state
    /// its type cannot produce by default, such as a specific seed.
    pub fn with_hasher(hash_builder: HB) -> Self
    where
        T: Default,
    {
        Self {
            inner: std::sync::Mutex::new(T::default()),
            hash_builder,

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}

impl<D, R, T, HB> Default for TableSharedMutex<D, R, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R> + Default,
    HB: std::hash::BuildHasher + Default + Clone,
{
    fn default() -> Self {
        Self::with_hasher(HB::default())
    }
}

#[inline]
fn make_hash<K: std::hash::Hash + ?Sized>(
    hash_builder: &impl std::hash::BuildHasher,
    val: &K,
) -> u64 {
    hash_builder.hash_one(val)
}

impl<D, R, T, HB> Table<D, R> for TableSharedMutex<D, R, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    HB: std::hash::BuildHasher + Default + Clone,
{
    fn get(&self, data: &D) -> Option<R> {
        let hash = make_hash(&self.hash_builder, &data);

        let guard = self.inner.lock().unwrap();
        guard.get(hash, data)
    }

    fn get_or_insert<CF>(&self, data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        let hash = make_hash(&self.hash_builder, &data);

        let mut guard = self.inner.lock().unwrap();
        guard.get_or_insert(hash, data, creation_meta)
    }

    #[cfg(feature = "reset-tables")]
    fn reset(&self) {
        self.inner.lock().unwrap().reset();
    }
}
