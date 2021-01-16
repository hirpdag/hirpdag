use crate::reference::*;
use crate::table::*;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

pub struct TableSharedMutex<D, R, T, HB = DefaultHasher>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
    HB: std::hash::BuildHasher + Default + Clone,
{
    inner: std::sync::Mutex<T>,
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
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

impl<D, R, T, HB> TableShared<D, R, T> for TableSharedMutex<D, R, T, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
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
}

pub struct BuildTableSharedMutex<D, R, T, TB, HB> {
    table_builder: TB,
    hash_builder: HB,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
    phantom_t: std::marker::PhantomData<T>,
}

impl<D, R, T, TB, HB> BuildTableSharedMutex<D, R, T, TB, HB>
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

impl<D, R, T, TB, HB> Clone for BuildTableSharedMutex<D, R, T, TB, HB>
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

impl<D, R, T, TB, HB> Default for BuildTableSharedMutex<D, R, T, TB, HB>
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
impl<D, R, T, TB, HB> BuildTableShared<D, R, T> for BuildTableSharedMutex<D, R, T, TB, HB>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
    TB: BuildTable<D, R, Table = T> + Default + Clone,
    HB: std::hash::BuildHasher + Default + Clone,
{
    type TableSharedType = TableSharedMutex<D, R, T, HB>;

    fn build_tableshared(&self) -> TableSharedMutex<D, R, T, HB> {
        TableSharedMutex::<D, R, T, HB> {
            inner: std::sync::Mutex::new(self.table_builder.build_table()),
            hash_builder: self.hash_builder.clone(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}
