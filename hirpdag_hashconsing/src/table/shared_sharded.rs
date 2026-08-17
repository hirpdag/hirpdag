use crate::reference::*;
use crate::table::*;
use array_init::array_init;

/// Number of independent shard locks.  Power-of-two so shard selection is a bitmask (no modulo).
const N_SHARDS: usize = 8;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

/// Concurrent hash-consing table using [`N_SHARDS`] independent mutexes.
///
/// The shard is selected by the low bits of the hash, so threads operating on
/// structurally different nodes rarely contend.  This is the default `Table`
/// implementation used by the `hirpdag` macro.
pub struct TableSharedSharded<D, R, WR, T, HB = DefaultHasher>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R, WR>,
    HB: std::hash::BuildHasher + Default + Clone,
{
    inner: [std::sync::Mutex<T>; N_SHARDS],
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
    phantom_wr: std::marker::PhantomData<WR>,
}

impl<D, R, WR, T, HB> TableSharedSharded<D, R, WR, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R, WR>,
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

impl<D, R, WR, T, HB> Table<D, R, WR> for TableSharedSharded<D, R, WR, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R, WR>,
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

pub struct BuildTableSharedSharded<D, R, WR, T, TB, HB> {
    table_builder: TB,
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
    phantom_wr: std::marker::PhantomData<WR>,
    phantom_t: std::marker::PhantomData<T>,
}

impl<D, R, WR, T, TB, HB> BuildTableSharedSharded<D, R, WR, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R, WR>,
    TB: BuildThreadUnsafeTable<D, R, WR, ThreadUnsafeTable = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    pub fn with_builders(table_builder: TB, hash_builder: HB) -> Self {
        Self {
            table_builder,
            hash_builder,

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
            phantom_wr: std::marker::PhantomData,
            phantom_t: std::marker::PhantomData,
        }
    }
}

impl<D, R, WR, T, TB, HB> Clone for BuildTableSharedSharded<D, R, WR, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R, WR>,
    TB: BuildThreadUnsafeTable<D, R, WR, ThreadUnsafeTable = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    fn clone(&self) -> Self {
        Self {
            table_builder: self.table_builder.clone(),
            hash_builder: self.hash_builder.clone(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
            phantom_wr: std::marker::PhantomData,
            phantom_t: std::marker::PhantomData,
        }
    }
}

impl<D, R, WR, T, TB, HB> Default for BuildTableSharedSharded<D, R, WR, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R, WR>,
    TB: BuildThreadUnsafeTable<D, R, WR, ThreadUnsafeTable = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    fn default() -> Self {
        Self {
            table_builder: TB::default(),
            hash_builder: HB::default(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
            phantom_wr: std::marker::PhantomData,
            phantom_t: std::marker::PhantomData,
        }
    }
}

impl<D, R, WR, T, TB, HB> BuildTable<D, R, WR> for BuildTableSharedSharded<D, R, WR, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    WR: ReferenceWeak<D, R>,
    T: ThreadUnsafeTable<D, R, WR>,
    TB: BuildThreadUnsafeTable<D, R, WR, ThreadUnsafeTable = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    type TableSharedType = TableSharedSharded<D, R, WR, T, HB>;

    fn build_tableshared(&self) -> TableSharedSharded<D, R, WR, T, HB> {
        let shards: [std::sync::Mutex<T>; N_SHARDS] =
            array_init(|_| std::sync::Mutex::new(self.table_builder.build_table()));
        TableSharedSharded::<D, R, WR, T, HB> {
            inner: shards,
            hash_builder: self.hash_builder.clone(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
            phantom_wr: std::marker::PhantomData,
        }
    }
}
