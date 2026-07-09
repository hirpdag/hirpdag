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

mod reference_sepcount;
pub use crate::reference_sepcount::RefSep;
pub use crate::reference_sepcount::RefSepPad;
pub use crate::reference_sepcount::RefSepPadWeak;
pub use crate::reference_sepcount::RefSepU32;
pub use crate::reference_sepcount::RefSepU32Weak;
pub use crate::reference_sepcount::RefSepWeak;

mod reference_tlc;
pub use crate::reference_tlc::RefTlc;
pub use crate::reference_tlc::RefTlcWeak;

// Hashconsing table implementations.

mod table_vec_linear_weak;
pub use crate::table_vec_linear_weak::TableVecLinearWeak;

mod table_vec_sorted_weak;
pub use crate::table_vec_sorted_weak::TableVecSortedWeak;

mod table_hashmap_fallback_weak;
pub use crate::table_hashmap_fallback_weak::TableHashmapFallbackWeak;

mod tableshared_sharded;
pub use crate::tableshared_sharded::BuildTableSharedSharded;
pub use crate::tableshared_sharded::TableSharedSharded;

mod tableshared_mutex;
pub use crate::tableshared_mutex::BuildTableSharedMutex;
pub use crate::tableshared_mutex::TableSharedMutex;

// Table backends built on third-party collection crates, behind the opt-in
// `third-party-tables` feature. `TableTovWeakTable` is an inner `Table` (over
// the `weak-table` crate); the `TableShared*` wrappers store the interned
// mapping directly in a concurrent collection instead of delegating to an inner
// single-threaded `Table`.

#[cfg(feature = "third-party-tables")]
mod table_tov_weak_table;
#[cfg(feature = "third-party-tables")]
pub use crate::table_tov_weak_table::TableTovWeakTable;

#[cfg(feature = "third-party-tables")]
mod tableshared_dashmap;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_dashmap::BuildTableSharedDashMap;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_dashmap::TableSharedDashMap;

#[cfg(feature = "third-party-tables")]
mod tableshared_flurry;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_flurry::BuildTableSharedFlurry;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_flurry::TableSharedFlurry;

#[cfg(feature = "third-party-tables")]
mod tableshared_skipmap;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_skipmap::BuildTableSharedSkipMap;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_skipmap::TableSharedSkipMap;

#[cfg(feature = "third-party-tables")]
mod tableshared_arcswap;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_arcswap::BuildTableSharedArcSwap;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_arcswap::TableSharedArcSwap;

#[cfg(feature = "third-party-tables")]
mod tableshared_evmap;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_evmap::BuildTableSharedEvmap;
#[cfg(feature = "third-party-tables")]
pub use crate::tableshared_evmap::TableSharedEvmap;

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
            test_table_weak_all::<RefSep<TestData>, RefSepWeak<TestData>>();
            test_table_weak_all::<RefSepPad<TestData>, RefSepPadWeak<TestData>>();
            test_table_weak_all::<RefSepU32<TestData>, RefSepU32Weak<TestData>>();
            test_table_weak_all::<RefTlc<TestData>, RefTlcWeak<TestData>>();

            // TableTovWeakTable does not work on RefLeak
            #[cfg(feature = "third-party-tables")]
            {
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

    /// Tests for the [`TableShared`] implementations backed by third-party
    /// concurrent collections. These are exercised with `RefArc` (the only
    /// bundled reference that is `Send + Sync + Hash + Eq`), and include a
    /// multi-threaded stress test verifying that all threads observe the same
    /// interned pointer for each key (the core hash-consing guarantee).
    #[cfg(feature = "third-party-tables")]
    mod test_concurrent {
        use crate::test_terrible_hasher::TerribleHasher;
        use crate::test_utils::*;
        use crate::*;

        type Data = TestData;
        type Ref = RefArc<TestData>;
        type DefHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;
        type BadHasher = std::hash::BuildHasherDefault<TerribleHasher>;

        /// Hammer a table from several threads all interning the same key range
        /// and assert every thread agrees on the interned pointer per key, and
        /// that distinct keys intern to distinct pointers.
        fn concurrent_stress<TS>(table: TS)
        where
            TS: TableShared<Data, Ref> + Send + Sync + 'static,
        {
            let table = std::sync::Arc::new(table);
            let n = 300usize;
            let n_threads = 8usize;

            let mut handles = Vec::new();
            for _ in 0..n_threads {
                let t = table.clone();
                handles.push(std::thread::spawn(move || {
                    let mut v: Vec<Ref> = Vec::new();
                    populate_linear(&mut v, &*t, 0..n);
                    v
                }));
            }
            let results: Vec<Vec<Ref>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

            let first = &results[0];
            for other in &results[1..] {
                assert_eq!(first.len(), other.len());
                for i in 0..n {
                    assert!(
                        Ref::strong_ptr_eq(&first[i], &other[i]),
                        "threads disagree on interned pointer for key {}",
                        i
                    );
                }
            }
            // Corresponding keys equal, non-corresponding keys distinct.
            assert_match_and_unique(first, first);
        }

        #[test]
        fn dashmap() {
            let b = BuildTableSharedDashMap::<Data, Ref, DefHasher>::default();
            test_tableshared::<Ref, TableSharedDashMap<Data, Ref, DefHasher>, _>(b);

            let b_bad =
                BuildTableSharedDashMap::<Data, Ref, BadHasher>::with_hasher(BadHasher::default());
            test_tableshared::<Ref, TableSharedDashMap<Data, Ref, BadHasher>, _>(b_bad);

            concurrent_stress(TableSharedDashMap::<Data, Ref, DefHasher>::default());
        }

        #[test]
        fn flurry() {
            let b = BuildTableSharedFlurry::<Data, Ref, DefHasher>::default();
            test_tableshared::<Ref, TableSharedFlurry<Data, Ref, DefHasher>, _>(b);

            let b_bad =
                BuildTableSharedFlurry::<Data, Ref, BadHasher>::with_hasher(BadHasher::default());
            test_tableshared::<Ref, TableSharedFlurry<Data, Ref, BadHasher>, _>(b_bad);

            concurrent_stress(TableSharedFlurry::<Data, Ref, DefHasher>::default());
        }

        #[test]
        fn skipmap() {
            let b = BuildTableSharedSkipMap::<Data, Ref>::default();
            test_tableshared::<Ref, TableSharedSkipMap<Data, Ref>, _>(b);

            concurrent_stress(TableSharedSkipMap::<Data, Ref>::default());
        }

        #[test]
        fn arcswap() {
            let b = BuildTableSharedArcSwap::<Data, Ref, DefHasher>::default();
            test_tableshared::<Ref, TableSharedArcSwap<Data, Ref, DefHasher>, _>(b);

            concurrent_stress(TableSharedArcSwap::<Data, Ref, DefHasher>::default());
        }

        #[test]
        fn evmap() {
            let b = BuildTableSharedEvmap::<Data, Ref>::default();
            test_tableshared::<Ref, TableSharedEvmap<Data, Ref>, _>(b);

            concurrent_stress(TableSharedEvmap::<Data, Ref>::default());
        }
    }
}
