//! Hashconsing
//!
//! This module provides interfaces for reference counting and hashconsing,
//! and several composable implementations to experiment with.

// Hashconsing Interface

mod reference;
pub use crate::reference::Reference;
pub use crate::reference::ReferenceWeak;
mod table;
pub use crate::table::BuildTable;
pub use crate::table::BuildTableDefault;
pub use crate::table::BuildTableShared;
pub use crate::table::BuildTableSharedDefault;
pub use crate::table::Table;
pub use crate::table::TableShared;

// Hashconsing reference implementations.

mod reference_arc;
pub use crate::reference_arc::RefArc;
pub use crate::reference_arc::RefArcWeak;

mod reference_rc;
pub use crate::reference_rc::RefRc;
pub use crate::reference_rc::RefRcWeak;

mod reference_leak;
pub use crate::reference_leak::RefLeak;
pub use crate::reference_leak::RefLeakWeak;

// Hashconsing table implementations.

mod table_vec_linear_weak;
pub use crate::table_vec_linear_weak::TableVecLinearWeak;

mod table_vec_sorted_weak;
pub use crate::table_vec_sorted_weak::TableVecSortedWeak;

mod table_hashmap_fallback_weak;
pub use crate::table_hashmap_fallback_weak::TableHashmapFallbackWeak;

mod table_tov_weak_table;
pub use crate::table_tov_weak_table::TableTovWeakTable;

mod tableshared_sharded;
pub use crate::tableshared_sharded::BuildTableSharedSharded;
pub use crate::tableshared_sharded::TableSharedSharded;

mod tableshared_mutex;
pub use crate::tableshared_mutex::BuildTableSharedMutex;
pub use crate::tableshared_mutex::TableSharedMutex;

// Internal

mod weak_entry;

#[cfg(test)]
mod test_utils;

#[cfg(test)]
mod test_terrible_hasher;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_terrible_hasher::TerribleHasher;
    use crate::test_utils::*;

    mod test_rc {
        use super::*;

        fn test_tableshared_sharded<R, T, HB>(hash_builder: HB)
        where
            R: Reference<TestData>,
            T: Table<TestData, R> + Default,
            HB: std::hash::BuildHasher + Default + Clone,
        {
            let table_builder = BuildTableDefault::<T>::default();
            let tsb =
                BuildTableSharedSharded::<TestData, R, T, BuildTableDefault<T>, HB>::with_builders(
                    table_builder,
                    hash_builder,
                );

            test_tableshared::<
                R,
                T,
                TableSharedSharded<TestData, R, T, HB>,
                BuildTableSharedSharded<TestData, R, T, BuildTableDefault<T>, HB>,
            >(tsb);
        }

        fn test_tableshared_mutex<R, T, HB>(hash_builder: HB)
        where
            R: Reference<TestData>,
            T: Table<TestData, R> + Default,
            HB: std::hash::BuildHasher + Default + Clone,
        {
            let table_builder = BuildTableDefault::<T>::default();
            let tsb =
                BuildTableSharedMutex::<TestData, R, T, BuildTableDefault<T>, HB>::with_builders(
                    table_builder,
                    hash_builder,
                );

            test_tableshared::<
                R,
                T,
                TableSharedMutex<TestData, R, T, HB>,
                BuildTableSharedMutex<TestData, R, T, BuildTableDefault<T>, HB>,
            >(tsb);
        }
        fn test_tableshared_all<R, T>()
        where
            R: Reference<TestData>,
            T: Table<TestData, R> + Default,
        {
            let hash_builder = std::hash::BuildHasherDefault::<
                std::collections::hash_map::DefaultHasher,
            >::default();

            test_tableshared_sharded::<
                R,
                T,
                std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>,
            >(hash_builder);

            let hash_builder = std::hash::BuildHasherDefault::<TerribleHasher>::default();

            test_tableshared_sharded::<R, T, std::hash::BuildHasherDefault<TerribleHasher>>(
                hash_builder,
            );

            let hash_builder = std::hash::BuildHasherDefault::<
                std::collections::hash_map::DefaultHasher,
            >::default();

            test_tableshared_mutex::<
                R,
                T,
                std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>,
            >(hash_builder);

            let hash_builder = std::hash::BuildHasherDefault::<TerribleHasher>::default();

            test_tableshared_mutex::<R, T, std::hash::BuildHasherDefault<TerribleHasher>>(
                hash_builder,
            );
        }

        fn test_table_weak_all<R, RW>()
        where
            R: Reference<TestData>,
            RW: ReferenceWeak<TestData, R>,
        {
            test_tableshared_all::<R, TableVecLinearWeak<TestData, R, RW>>();
            test_tableshared_all::<R, TableVecSortedWeak<TestData, R, RW>>();
            test_tableshared_all::<R, TableVecSortedWeak<TestData, R, RW>>();
            test_tableshared_all::<
                R,
                TableHashmapFallbackWeak<TestData, R, RW, TableVecLinearWeak<TestData, R, RW>>,
            >();
        }

        #[test]
        fn test_ref_all() {
            test_table_weak_all::<RefRc<TestData>, RefRcWeak<TestData>>();
            test_table_weak_all::<RefArc<TestData>, RefArcWeak<TestData>>();
            test_table_weak_all::<RefLeak<TestData>, RefLeakWeak<TestData>>();

            // TableTovWeakTable does not work on RefLeak
            test_tableshared_all::<
                RefRc<TestData>,
                TableTovWeakTable<TestData, RefRc<TestData>, RefRcWeak<TestData>>,
            >();
            test_tableshared_all::<
                RefArc<TestData>,
                TableTovWeakTable<TestData, RefArc<TestData>, RefArcWeak<TestData>>,
            >();
        }
    }
}
