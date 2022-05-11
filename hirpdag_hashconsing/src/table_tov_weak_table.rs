use crate::reference::*;
use crate::table::Table;
use weak_table::traits::WeakElement;
use weak_table::traits::WeakKey;
use weak_table::WeakHashSet;

pub struct TableTovWeakTable<
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R> + WeakKey<Key = D> + WeakElement<Strong = R>,
> {
    m: WeakHashSet<RW>,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
}

impl<D, R, RW> Default for TableTovWeakTable<D, R, RW>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R> + WeakKey<Key = D> + WeakElement<Strong = R>,
{
    fn default() -> Self {
        Self {
            m: WeakHashSet::new(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
        }
    }
}

impl<D, R, RW> Table<D, R> for TableTovWeakTable<D, R, RW>
where
    D: std::hash::Hash + std::cmp::Eq + std::fmt::Debug,
    R: Reference<D>,
    RW: ReferenceWeak<D, R> + WeakKey<Key = D> + WeakElement<Strong = R>,
{
    fn get(&self, _hash: u64, data: &D) -> Option<R> {
        self.m.get(data)
    }

    fn get_or_insert<CF>(&mut self, _hash: u64, mut data: D, creation_meta: CF) -> R
    where
        CF: FnOnce(&mut D),
    {
        if let Some(r) = self.m.get(&data) {
            return r;
        }
        creation_meta(&mut data);
        let obj = R::new(data);
        self.m.insert(R::strong_clone(&obj));
        obj
    }
}
