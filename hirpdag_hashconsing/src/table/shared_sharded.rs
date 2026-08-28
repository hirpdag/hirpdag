use crate::reference::*;
use crate::table::*;
use array_init::array_init;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

/// Concurrent hash-consing table using `N_SHARDS` independent mutexes.
///
/// The shard is selected by the low bits of the hash, so threads operating on
/// structurally different nodes rarely contend.  `N_SHARDS` must be a power of
/// two: shard selection is a bitmask (`hash & (N_SHARDS - 1)`), so a non
/// power-of-two would only ever use the low shards.  See [`TableSharedSharded8`]
/// for the eight-shard alias used by the `hirpdag` macro presets.
pub struct TableSharedShardedN<const N_SHARDS: usize, D, R, T, HB = DefaultHasher>
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

/// Eight-shard [`TableSharedShardedN`].  This is the default `Table`
/// implementation used by the `hirpdag` macro.
pub type TableSharedSharded8<D, R, T, HB = DefaultHasher> = TableSharedShardedN<8, D, R, T, HB>;

impl<const N_SHARDS: usize, D, R, T, HB> TableSharedShardedN<N_SHARDS, D, R, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    HB: std::hash::BuildHasher + Default + Clone,
{
    fn get_shard(&self, hash: u64) -> &std::sync::Mutex<T> {
        let mask = (N_SHARDS - 1) as u64;
        let index = hash & mask;
        &self.inner[index as usize]
    }
}

#[inline]
fn make_hash<K: std::hash::Hash + ?Sized>(
    hash_builder: &impl std::hash::BuildHasher,
    val: &K,
) -> u64 {
    hash_builder.hash_one(val)
}

impl<const N_SHARDS: usize, D, R, T, HB> Table<D, R> for TableSharedShardedN<N_SHARDS, D, R, T, HB>
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

pub struct BuildTableSharedShardedN<const N_SHARDS: usize, D, R, T, TB, HB> {
    table_builder: TB,
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
    phantom_t: std::marker::PhantomData<T>,
}

/// Builder for [`TableSharedSharded8`].
pub type BuildTableSharedSharded8<D, R, T, TB, HB> = BuildTableSharedShardedN<8, D, R, T, TB, HB>;

impl<const N_SHARDS: usize, D, R, T, TB, HB> BuildTableSharedShardedN<N_SHARDS, D, R, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    TB: BuildThreadUnsafeTable<D, R, ThreadUnsafeTable = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    pub fn with_builders(table_builder: TB, hash_builder: HB) -> Self {
        Self {
            table_builder,
            hash_builder,

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
            phantom_t: std::marker::PhantomData,
        }
    }
}

impl<const N_SHARDS: usize, D, R, T, TB, HB> Clone
    for BuildTableSharedShardedN<N_SHARDS, D, R, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    TB: BuildThreadUnsafeTable<D, R, ThreadUnsafeTable = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    fn clone(&self) -> Self {
        Self {
            table_builder: self.table_builder.clone(),
            hash_builder: self.hash_builder.clone(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
            phantom_t: std::marker::PhantomData,
        }
    }
}

impl<const N_SHARDS: usize, D, R, T, TB, HB> Default
    for BuildTableSharedShardedN<N_SHARDS, D, R, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    TB: BuildThreadUnsafeTable<D, R, ThreadUnsafeTable = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    fn default() -> Self {
        Self {
            table_builder: TB::default(),
            hash_builder: HB::default(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
            phantom_t: std::marker::PhantomData,
        }
    }
}

impl<const N_SHARDS: usize, D, R, T, TB, HB> BuildTable<D, R>
    for BuildTableSharedShardedN<N_SHARDS, D, R, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: ThreadUnsafeTable<D, R>,
    TB: BuildThreadUnsafeTable<D, R, ThreadUnsafeTable = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    type TableSharedType = TableSharedShardedN<N_SHARDS, D, R, T, HB>;

    fn build_tableshared(&self) -> TableSharedShardedN<N_SHARDS, D, R, T, HB> {
        let shards: [std::sync::Mutex<T>; N_SHARDS] =
            array_init(|_| std::sync::Mutex::new(self.table_builder.build_table()));
        TableSharedShardedN::<N_SHARDS, D, R, T, HB> {
            inner: shards,
            hash_builder: self.hash_builder.clone(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}
