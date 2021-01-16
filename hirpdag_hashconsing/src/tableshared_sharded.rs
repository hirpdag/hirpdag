use crate::reference::*;
use crate::table::*;
use array_init::array_init;

const N_SHARDS: usize = 8;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

pub struct TableSharedSharded<D, R, T, HB = DefaultHasher>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
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
    T: Table<D, R>,
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
    use std::hash::Hasher;
    let mut state = hash_builder.build_hasher();
    val.hash(&mut state);
    state.finish()
}

impl<D, R, T, HB> TableShared<D, R, T> for TableSharedSharded<D, R, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
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
}

pub struct BuildTableSharedSharded<D, R, T, TB, HB> {
    table_builder: TB,
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
    phantom_t: std::marker::PhantomData<T>,
}

impl<D, R, T, TB, HB> BuildTableSharedSharded<D, R, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
    TB: BuildTable<D, R, Table = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    pub fn with_builders(table_builder: TB, hash_builder: HB) -> Self {
        Self {
            table_builder: table_builder,
            hash_builder: hash_builder,

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
            phantom_t: std::marker::PhantomData,
        }
    }
}

impl<D, R, T, TB, HB> Clone for BuildTableSharedSharded<D, R, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
    TB: BuildTable<D, R, Table = T> + Default + Clone,
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

impl<D, R, T, TB, HB> Default for BuildTableSharedSharded<D, R, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
    TB: BuildTable<D, R, Table = T> + Default + Clone,
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

impl<D, R, T, TB, HB> BuildTableShared<D, R, T> for BuildTableSharedSharded<D, R, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
    TB: BuildTable<D, R, Table = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    type TableSharedType = TableSharedSharded<D, R, T, HB>;

    fn build_tableshared(&self) -> TableSharedSharded<D, R, T, HB> {
        let shards: [std::sync::Mutex<T>; N_SHARDS] =
            array_init(|_| std::sync::Mutex::new(self.table_builder.build_table()));
        TableSharedSharded::<D, R, T, HB> {
            inner: shards,
            hash_builder: self.hash_builder.clone(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}
