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
pub use crate::table::BuildThreadUnsafeTable;
pub use crate::table::BuildThreadUnsafeTableDefault;
pub use crate::table::Table;
pub use crate::table::ThreadUnsafeTable;

// Hashconsing reference implementations (see the `reference` module).

pub use crate::reference::arc::RefArc;
pub use crate::reference::arc::RefArcWeak;

pub use crate::reference::rc::RefRc;
pub use crate::reference::rc::RefRcWeak;

pub use crate::reference::leak::RefLeak;
pub use crate::reference::leak::RefLeakWeak;

pub use crate::reference::sepcount::RefSep;
pub use crate::reference::sepcount::RefSepPad;
pub use crate::reference::sepcount::RefSepPadWeak;
pub use crate::reference::sepcount::RefSepU32;
pub use crate::reference::sepcount::RefSepU32Weak;
pub use crate::reference::sepcount::RefSepWeak;

pub use crate::reference::tlc::RefTlc;
pub use crate::reference::tlc::RefTlcWeak;

// Hashconsing table implementations (see the `table` module).

pub use crate::table::vec_linear_threadunsafe::TableVecLinearWeak;

pub use crate::table::vec_sorted_threadunsafe::TableVecSortedWeak;

pub use crate::table::hashmap_fallback_threadunsafe::TableHashmapFallbackWeak;

pub use crate::table::shared_sharded::BuildTableSharedSharded;
pub use crate::table::shared_sharded::TableSharedSharded;

pub use crate::table::shared_mutex::BuildTableSharedMutex;
pub use crate::table::shared_mutex::TableSharedMutex;

// Table backends built on third-party collection crates, behind the opt-in
// `third-party-tables` feature. `TableTovWeakTable` is an inner `ThreadUnsafeTable`
// (over the `weak-table` crate); the `*_strong` backends store the interned
// mapping directly in a concurrent collection instead of delegating to an inner
// single-threaded `ThreadUnsafeTable`.

#[cfg(feature = "third-party-tables")]
pub use crate::table::tov_weak_table_threadunsafe::TableTovWeakTable;

#[cfg(feature = "third-party-tables")]
pub use crate::table::dashmap_strong::BuildTableSharedDashMap;
#[cfg(feature = "third-party-tables")]
pub use crate::table::dashmap_strong::TableSharedDashMap;

#[cfg(feature = "third-party-tables")]
pub use crate::table::flurry_strong::BuildTableSharedFlurry;
#[cfg(feature = "third-party-tables")]
pub use crate::table::flurry_strong::TableSharedFlurry;

#[cfg(feature = "third-party-tables")]
pub use crate::table::skipmap_strong::BuildTableSharedSkipMap;
#[cfg(feature = "third-party-tables")]
pub use crate::table::skipmap_strong::TableSharedSkipMap;

#[cfg(feature = "third-party-tables")]
pub use crate::table::arcswap_strong::BuildTableSharedArcSwap;
#[cfg(feature = "third-party-tables")]
pub use crate::table::arcswap_strong::TableSharedArcSwap;

#[cfg(feature = "third-party-tables")]
pub use crate::table::evmap_strong::BuildTableSharedEvmap;
#[cfg(feature = "third-party-tables")]
pub use crate::table::evmap_strong::TableSharedEvmap;

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
            T: ThreadUnsafeTable<TestData, R> + Default,
            HB: std::hash::BuildHasher + Default + Clone,
        {
            let table_builder = BuildThreadUnsafeTableDefault::<T>::default();
            let tsb =
                BuildTableSharedSharded::<TestData, R, T, BuildThreadUnsafeTableDefault<T>, HB>::with_builders(
                    table_builder,
                    hash_builder,
                );

            test_tableshared::<
                R,
                TableSharedSharded<TestData, R, T, HB>,
                BuildTableSharedSharded<TestData, R, T, BuildThreadUnsafeTableDefault<T>, HB>,
            >(tsb);
        }

        fn test_tableshared_mutex<R, T, HB>(hash_builder: HB)
        where
            R: Reference<TestData>,
            T: ThreadUnsafeTable<TestData, R> + Default,
            HB: std::hash::BuildHasher + Default + Clone,
        {
            let table_builder = BuildThreadUnsafeTableDefault::<T>::default();
            let tsb =
                BuildTableSharedMutex::<TestData, R, T, BuildThreadUnsafeTableDefault<T>, HB>::with_builders(
                    table_builder,
                    hash_builder,
                );

            test_tableshared::<
                R,
                TableSharedMutex<TestData, R, T, HB>,
                BuildTableSharedMutex<TestData, R, T, BuildThreadUnsafeTableDefault<T>, HB>,
            >(tsb);
        }
        fn test_tableshared_all<R, T>()
        where
            R: Reference<TestData>,
            T: ThreadUnsafeTable<TestData, R> + Default,
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

    /// Tests for the [`Table`] implementations backed by third-party
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
            TS: Table<Data, Ref> + Send + Sync + 'static,
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
