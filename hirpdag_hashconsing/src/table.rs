use crate::reference::*;

pub trait Table<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    fn get(&self, hash: u64, data: &D) -> Option<R>;

    fn get_or_insert<CF>(&mut self, hash: u64, data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D);
}

pub trait BuildTable<D, R>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
{
    type Table: Table<D, R>;

    fn build_table(&self) -> Self::Table;
}

pub struct BuildTableDefault<T>(std::marker::PhantomData<T>);

impl<D, R, T> BuildTable<D, R> for BuildTableDefault<T>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Default + Table<D, R>,
{
    type Table = T;

    fn build_table(&self) -> T {
        T::default()
    }
}

impl<T> Default for BuildTableDefault<T>
where
    T: Default,
{
    fn default() -> Self {
        Self {
            0: std::marker::PhantomData,
        }
    }
}

impl<T> Clone for BuildTableDefault<T> {
    fn clone(&self) -> Self {
        Self {
            0: std::marker::PhantomData,
        }
    }
}

pub trait TableShared<D, R, T>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
{
    fn get(&self, data: &D) -> Option<R>;

    fn get_or_insert<CF>(&self, data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D);
}

pub trait BuildTableShared<D, R, T>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
{
    type TableSharedType: TableShared<D, R, T>;

    fn build_tableshared(&self) -> Self::TableSharedType;
}

pub struct BuildTableSharedDefault<TS>(std::marker::PhantomData<TS>);

impl<D, R, T, TS> BuildTableShared<D, R, T> for BuildTableSharedDefault<TS>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    T: Table<D, R>,
    TS: TableShared<D, R, T> + Default,
{
    type TableSharedType = TS;

    fn build_tableshared(&self) -> TS {
        TS::default()
    }
}
