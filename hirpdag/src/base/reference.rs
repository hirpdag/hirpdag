// Reference Handles

use crate::base::meta::HirpdagComputeMeta;
use crate::base::meta::HirpdagMeta;
use crate::base::meta::HirpdagMetaFlagType;
use hirpdag_hashconsing;
use hirpdag_hashconsing::BuildTableShared;
use hirpdag_hashconsing::Reference;
use hirpdag_hashconsing::Table;
use hirpdag_hashconsing::TableShared;

pub struct HirpdagRef<D: HirpdagStruct, R: Reference<HirpdagStorage<D>>>(
    R,
    std::marker::PhantomData<D>,
);

impl<D, R> std::hash::Hash for HirpdagRef<D, R>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        R::strong_deref(&self.0).hash(state)
    }
}

impl<D, R> Clone for HirpdagRef<D, R>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
{
    fn clone(&self) -> Self {
        HirpdagRef(R::strong_clone(&self.0), std::marker::PhantomData)
    }
}

impl<D, R> std::ops::Deref for HirpdagRef<D, R>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
{
    type Target = D;
    fn deref(&self) -> &D {
        &R::strong_deref(&self.0).hirpdag_data
    }
}

impl<D, R> std::fmt::Debug for HirpdagRef<D, R>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        R::strong_deref(&self.0).hirpdag_data.fmt(f)
    }
}

impl<D, R> std::cmp::PartialEq for HirpdagRef<D, R>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
{
    fn eq(&self, other: &Self) -> bool {
        R::strong_ptr_eq(&self.0, &other.0)
    }
}
impl<D, R> std::cmp::Eq for HirpdagRef<D, R>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
{
}

impl<D, R> HirpdagRef<D, R>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
{
    pub fn hirpdag_get_meta(&self) -> &HirpdagMeta {
        &R::strong_deref(&self.0).hirpdag_meta
    }
}

impl<D, R> HirpdagComputeMeta for HirpdagRef<D, R>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
{
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        self.hirpdag_get_meta().clone()
    }
}

impl<D, R> HirpdagComputeMeta for &HirpdagRef<D, R>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
{
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        self.hirpdag_get_meta().clone()
    }
}

// ==== Hashcons Storage Base

pub struct HirpdagStorage<D: HirpdagStruct> {
    hirpdag_meta: HirpdagMeta,
    hirpdag_data: D,
}

impl<D> std::hash::Hash for HirpdagStorage<D>
where
    D: HirpdagStruct,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hirpdag_data.hash(state);
    }
}

impl<D> std::cmp::PartialEq for HirpdagStorage<D>
where
    D: HirpdagStruct,
{
    fn eq(&self, other: &Self) -> bool {
        self.hirpdag_data == other.hirpdag_data
    }
}
impl<D> std::cmp::Eq for HirpdagStorage<D> where D: HirpdagStruct {}

impl<D> std::cmp::Ord for HirpdagStorage<D>
where
    D: HirpdagStruct,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.hirpdag_data.cmp(&other.hirpdag_data)
    }
}
impl<D> std::cmp::PartialOrd for HirpdagStorage<D>
where
    D: HirpdagStruct,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.hirpdag_data.cmp(&other.hirpdag_data))
    }
}

impl<D> std::fmt::Debug for HirpdagStorage<D>
where
    D: HirpdagStruct,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.hirpdag_data.fmt(f)
    }
}

pub struct HirpdagHashconsTable<
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
    T: Table<HirpdagStorage<D>, R>,
    TS: TableShared<HirpdagStorage<D>, R, T>,
> {
    table: TS,

    phantom_d: std::marker::PhantomData<D>,
    phantom_r: std::marker::PhantomData<R>,
    phantom_t: std::marker::PhantomData<T>,
}

impl<D, R, T, TS> HirpdagHashconsTable<D, R, T, TS>
where
    D: HirpdagStruct,
    R: Reference<HirpdagStorage<D>>,
    T: Table<HirpdagStorage<D>, R>,
    TS: TableShared<HirpdagStorage<D>, R, T>,
{
    pub fn new<TSB>(tableshared_builder: TSB) -> Self
    where
        TSB: BuildTableShared<HirpdagStorage<D>, R, T, TableSharedType = TS> + Default,
    {
        Self {
            table: tableshared_builder.build_tableshared(),

            phantom_d: std::marker::PhantomData,
            phantom_r: std::marker::PhantomData,
            phantom_t: std::marker::PhantomData,
        }
    }

    pub fn hirpdag_hashcons(&self, data: D) -> HirpdagRef<D, R> {
        let storage = HirpdagStorage::<D> {
            hirpdag_meta: HirpdagMeta::zero(),
            hirpdag_data: data,
        };
        let compute_hirpdag_meta = |s: &mut HirpdagStorage<D>| {
            let meta = s.hirpdag_data.hirpdag_compute_meta();
            s.hirpdag_meta = meta;
        };

        HirpdagRef(
            self.table.get_or_insert(storage, compute_hirpdag_meta),
            std::marker::PhantomData,
        )
    }
}

/// This trait is implemented by generated structures containing the
/// fields of a Hirpdag generated structure.
pub trait HirpdagStruct:
    std::hash::Hash
    + std::fmt::Debug
    + Clone
    + std::marker::Sized
    + HirpdagComputeMeta
    + std::cmp::PartialEq
    + std::cmp::Eq
    + std::cmp::PartialOrd
    + std::cmp::Ord
{
    type ReferenceStorageStruct: Reference<HirpdagStorage<Self>>;

    fn hirpdag_hashcons(self) -> HirpdagRef<Self, Self::ReferenceStorageStruct>;

    /// Computes the flags for the current HirpdagStruct.
    fn hirpdag_flags(&self) -> HirpdagMetaFlagType {
        0
    }
}
