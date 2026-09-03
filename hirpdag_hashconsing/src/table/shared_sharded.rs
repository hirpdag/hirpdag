use crate::reference::*;
use crate::table::*;

/// Number of independent shard locks.  Power-of-two so shard selection is a bitmask (no modulo).
const N_SHARDS: usize = 8;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

/// Concurrent hash-consing table using [`N_SHARDS`] independent mutexes.
///
/// The shard is selected by the low bits of the hash, so threads operating on
/// structurally different nodes rarely contend.  This is the default `Table`
/// implementation used by the `hirpdag` macro.
pub struct TableSharedSharded<D, R, T, HB = DefaultHasher>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    HB: std::hash::BuildHasher + Default + Clone,
{
    inner: [std::sync::Mutex<T>; N_SHARDS],
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R, T, HB> TableSharedSharded<D, R, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    HB: std::hash::BuildHasher + Default + Clone,
{
    /// An empty table, one freshly built inner table per shard, hashing with
    /// `hash_builder`.
    ///
    /// [`Default`] covers the usual case; this is for a hasher carrying state
    /// its type cannot produce by default, such as a specific seed.
    pub fn with_hasher(hash_builder: HB) -> Self
    where
        T: Default,
    {
        Self {
            inner: std::array::from_fn(|_| std::sync::Mutex::new(T::default())),
            hash_builder,

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }

    fn get_shard(&self, hash: u64) -> &std::sync::Mutex<T> {
        let mask = (N_SHARDS - 1) as u64;
        let index = hash & mask;
        &self.inner[index as usize]
    }
}

impl<D, R, T, HB> Default for TableSharedSharded<D, R, T, HB>
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

impl<D, R, T, HB> Table<D, R> for TableSharedSharded<D, R, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    HB: std::hash::BuildHasher + Default + Clone,
{
    fn get(&self, data: &D) -> Option<R> {
        let hash = make_hash(&self.hash_builder, &data);

        let shard = self.get_shard(hash);
        let guard = shard.lock().unwrap();
        guard.get(hash, data)
    }

    fn get_or_insert<CF>(&self, data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        let hash = make_hash(&self.hash_builder, &data);

        let shard = self.get_shard(hash);
        let mut guard = shard.lock().unwrap();
        guard.get_or_insert(hash, data, creation_meta)
    }

    #[cfg(feature = "reset-tables")]
    fn reset(&self) {
        for shard in &self.inner {
            shard.lock().unwrap().reset();
        }
    }
}
