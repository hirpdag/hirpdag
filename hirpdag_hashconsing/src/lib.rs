//! Hashconsing
//!
//! This module provides interfaces for reference counting and hashconsing,
//! and several composable implementations to experiment with.

// Hashconsing Interface

mod reference;
pub use crate::reference::drop_queue;
pub use crate::reference::drop_queue::IterativeDrop;
pub use crate::reference::Reference;
pub use crate::reference::ReferenceWeak;
mod table;
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

pub use crate::table::shared_sharded::TableSharedSharded;

pub use crate::table::shared_mutex::TableSharedMutex;

// Table backends built on third-party collection crates, behind the opt-in
// `third-party-tables` feature. `TableTovWeakTable` is an inner `ThreadUnsafeTable`
// (over the `weak-table` crate); the `*_strong` backends store the interned
// mapping directly in a concurrent collection instead of delegating to an inner
// single-threaded `ThreadUnsafeTable`.

#[cfg(feature = "third-party-tables")]
pub use crate::table::tov_weak_table_threadunsafe::TableTovWeakTable;

#[cfg(feature = "third-party-tables")]
pub use crate::table::dashmap_strong::TableSharedDashMap;

#[cfg(feature = "third-party-tables")]
pub use crate::table::flurry_strong::TableSharedFlurry;

#[cfg(feature = "third-party-tables")]
pub use crate::table::skipmap_strong::TableSharedSkipMap;

#[cfg(feature = "third-party-tables")]
pub use crate::table::arcswap_strong::TableSharedArcSwap;

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
            test_tableshared::<R, TableSharedSharded<TestData, R, T, HB>>(|| {
                TableSharedSharded::with_hasher(hash_builder.clone())
            });
        }

        fn test_tableshared_mutex<R, T, HB>(hash_builder: HB)
        where
            R: Reference<TestData>,
            T: ThreadUnsafeTable<TestData, R> + Default,
            HB: std::hash::BuildHasher + Default + Clone,
        {
            test_tableshared::<R, TableSharedMutex<TestData, R, T, HB>>(|| {
                TableSharedMutex::with_hasher(hash_builder.clone())
            });
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
            test_tableshared_all::<
                R,
                TableHashmapFallbackWeak<TestData, R, RW, TableVecLinearWeak<TestData, R, RW>>,
            >();
            // The inner table of the `arc_hash_sorted` preset.
            test_tableshared_all::<
                R,
                TableHashmapFallbackWeak<TestData, R, RW, TableVecSortedWeak<TestData, R, RW>>,
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

    /// Reference lifecycle: the data is dropped exactly once, when the last
    /// strong handle goes, and a weak handle stops upgrading at that point.
    ///
    /// The table tests only check pointer identity, so they would pass for a
    /// reference that never frees anything, or frees twice. `RefSep*` and
    /// `RefTlc` do their own counting in unsafe code; this pins it down.
    mod test_lifecycle {
        use crate::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        /// Data that counts how many times it has been dropped.
        #[derive(Debug)]
        struct Tracked {
            id: u32,
            drops: Arc<AtomicUsize>,
        }

        impl Tracked {
            fn new(id: u32) -> (Self, Arc<AtomicUsize>) {
                let drops = Arc::new(AtomicUsize::new(0));
                let data = Self {
                    id,
                    drops: drops.clone(),
                };
                (data, drops)
            }
        }

        impl Drop for Tracked {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
            }
        }

        impl std::hash::Hash for Tracked {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.id.hash(state);
            }
        }

        impl PartialEq for Tracked {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id
            }
        }

        impl Eq for Tracked {}

        /// Same-thread lifecycle, for references whose drop takes effect
        /// immediately.
        fn freed_after_last_strong<R, RW>()
        where
            R: Reference<Tracked>,
            RW: ReferenceWeak<Tracked, R>,
        {
            let (data, drops) = Tracked::new(1);
            let a = R::new(data);
            let b = R::strong_clone(&a);
            let weak = RW::weak_downgrade(&a);
            assert!(R::strong_ptr_eq(&a, &b));

            drop(a);
            assert_eq!(drops.load(Ordering::SeqCst), 0, "freed with a handle left");
            let up = RW::weak_upgrade(&weak).expect("upgrade while a handle is left");
            assert!(R::strong_ptr_eq(&up, &b));
            drop(up);
            drop(b);

            assert_eq!(drops.load(Ordering::SeqCst), 1, "not freed exactly once");
            assert!(RW::weak_upgrade(&weak).is_none(), "upgraded a freed value");
            drop(weak);
            assert_eq!(drops.load(Ordering::SeqCst), 1, "weak drop freed the data");
        }

        #[test]
        fn arc() {
            freed_after_last_strong::<RefArc<Tracked>, RefArcWeak<Tracked>>();
        }

        #[test]
        fn rc() {
            freed_after_last_strong::<RefRc<Tracked>, RefRcWeak<Tracked>>();
        }

        #[test]
        fn sep() {
            freed_after_last_strong::<RefSep<Tracked>, RefSepWeak<Tracked>>();
        }

        #[test]
        fn seppad() {
            freed_after_last_strong::<RefSepPad<Tracked>, RefSepPadWeak<Tracked>>();
        }

        #[test]
        fn sepu32() {
            freed_after_last_strong::<RefSepU32<Tracked>, RefSepU32Weak<Tracked>>();
        }

        /// `RefLeak` never frees, and its weak handles always upgrade.
        #[test]
        fn leak_never_frees() {
            type R = RefLeak<Tracked>;
            type RW = RefLeakWeak<Tracked>;
            let (data, drops) = Tracked::new(1);
            // `RefLeak` is a `ManuallyDrop`, whose inherent `new` would shadow
            // the trait's.
            let weak = {
                let a = <R as Reference<Tracked>>::new(data);
                <RW as ReferenceWeak<Tracked, R>>::weak_downgrade(&a)
            };
            assert!(<RW as ReferenceWeak<Tracked, R>>::weak_upgrade(&weak).is_some());
            assert_eq!(drops.load(Ordering::SeqCst), 0);
        }

        /// `RefTlc` defers decrements to a per-thread buffer that is flushed
        /// after a bounded number of operations or at thread exit, so the
        /// handles are dropped on a thread that then exits. Clones and drops
        /// in between cancel against the buffer; handles also cross threads,
        /// which is the case the design argues is sound.
        #[test]
        fn tlc_freed_once_after_flush() {
            let (data, drops) = Tracked::new(1);
            let a = RefTlc::new(data);
            let weak = RefTlcWeak::weak_downgrade(&a);

            std::thread::spawn(move || {
                let clones: Vec<_> = (0..100).map(|_| RefTlc::strong_clone(&a)).collect();
                drop(a);
                let up = RefTlcWeak::weak_upgrade(&weak).expect("upgrade while alive");
                let moved = RefTlc::strong_clone(&up);
                std::thread::spawn(move || drop(moved)).join().unwrap();
                drop(up);
                drop(clones);
                // Still referenced by this thread's deferred decrements.
                assert_eq!(drops.load(Ordering::SeqCst), 0);
                (weak, drops)
            })
            .join()
            .map(|(weak, drops)| {
                assert_eq!(drops.load(Ordering::SeqCst), 1, "not freed exactly once");
                assert!(RefTlcWeak::weak_upgrade(&weak).is_none());
            })
            .unwrap();
        }
    }

    /// Dropping a deep graph through [`IterativeDrop`] handles, for every
    /// reference type: nothing overflows a small stack, and every node is
    /// freed exactly once (none leaked, none twice).
    mod test_deep_drop {
        use crate::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        const DEPTH: usize = 100_000;
        // Recursive drop managed about 2,000 levels on this in release.
        const SMALL_STACK: usize = 64 * 1024;

        /// Names the reference type a chain is built from. A marker type
        /// ties the knot: `R = Arc<Link<R>>` would be an infinite type.
        trait Family: Sized + 'static {
            type R: Reference<Link<Self>>;
        }

        /// A node of a chain: its child reached through `next`, and, when
        /// `twice`, through `again` as well, so the child's last handle is
        /// the second of two in one node.
        struct Link<F: Family> {
            id: usize,
            // Held only to be dropped with the link.
            #[allow(dead_code)]
            next: Option<IterativeDrop<Link<F>, F::R>>,
            #[allow(dead_code)]
            again: Option<IterativeDrop<Link<F>, F::R>>,
            drops: Arc<AtomicUsize>,
        }

        macro_rules! family {
            ($name:ident, $reference:ident) => {
                struct $name;
                impl Family for $name {
                    type R = $reference<Link<$name>>;
                }
            };
        }

        family!(ArcF, RefArc);
        family!(RcF, RefRc);
        family!(SepF, RefSep);
        family!(SepPadF, RefSepPad);
        family!(SepU32F, RefSepU32);
        family!(TlcF, RefTlc);
        family!(LeakF, RefLeak);

        impl<F: Family> Drop for Link<F> {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::Relaxed);
            }
        }

        impl<F: Family> std::fmt::Debug for Link<F> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "Link({})", self.id)
            }
        }

        impl<F: Family> std::hash::Hash for Link<F> {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.id.hash(state);
            }
        }

        impl<F: Family> PartialEq for Link<F> {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id
            }
        }

        impl<F: Family> Eq for Link<F> {}

        /// A chain of `DEPTH` links, returned as its head.
        fn chain<F: Family>(twice: bool, drops: &Arc<AtomicUsize>) -> F::R {
            let mut head: Option<F::R> = None;
            for id in 0..DEPTH {
                let next = head.take();
                let again = if twice {
                    next.as_ref()
                        .map(|n| IterativeDrop::new(F::R::strong_clone(n)))
                } else {
                    None
                };
                head = Some(F::R::new(Link {
                    id,
                    next: next.map(IterativeDrop::new),
                    again,
                    drops: drops.clone(),
                }));
            }
            head.unwrap()
        }

        /// Build and drop a chain on a small stack, and count the frees once
        /// the thread has exited (`RefTlc` frees at its flush or at exit).
        fn freed_on_small_stack<F: Family>(twice: bool) -> usize {
            let drops = Arc::new(AtomicUsize::new(0));
            let counted = drops.clone();
            std::thread::Builder::new()
                .stack_size(SMALL_STACK)
                .spawn(move || drop(IterativeDrop::new(chain::<F>(twice, &counted))))
                .unwrap()
                .join()
                .unwrap();
            drops.load(Ordering::Relaxed)
        }

        fn every_link_freed_once<F: Family>() {
            assert_eq!(freed_on_small_stack::<F>(false), DEPTH);
            assert_eq!(freed_on_small_stack::<F>(true), DEPTH);
        }

        #[test]
        fn arc() {
            every_link_freed_once::<ArcF>();
        }

        #[test]
        fn rc() {
            every_link_freed_once::<RcF>();
        }

        #[test]
        fn sep() {
            every_link_freed_once::<SepF>();
            every_link_freed_once::<SepPadF>();
            every_link_freed_once::<SepU32F>();
        }

        #[test]
        fn tlc() {
            every_link_freed_once::<TlcF>();
        }

        #[test]
        fn leak_never_frees() {
            assert_eq!(freed_on_small_stack::<LeakF>(false), 0);
        }

        /// Several threads hold the same chain and drop their handles at
        /// once. Each drop's "last handle?" check races with the others; the
        /// chain must still be freed exactly once, on small stacks.
        ///
        /// Every handle is dropped on a worker thread that then exits: a
        /// `RefTlc` drop on the test's own thread would sit in its buffer
        /// until that thread flushed.
        fn concurrent<F: Family>()
        where
            F::R: Send,
        {
            let drops = Arc::new(AtomicUsize::new(0));
            let head = chain::<F>(true, &drops);
            let mut handles: Vec<_> = (0..7)
                .map(|_| IterativeDrop::new(F::R::strong_clone(&head)))
                .collect();
            handles.push(IterativeDrop::new(head));
            let barrier = Arc::new(std::sync::Barrier::new(handles.len()));
            let threads: Vec<_> = handles
                .into_iter()
                .map(|handle| {
                    let barrier = barrier.clone();
                    std::thread::Builder::new()
                        .stack_size(SMALL_STACK)
                        .spawn(move || {
                            barrier.wait();
                            drop(handle);
                        })
                        .unwrap()
                })
                .collect();
            for thread in threads {
                thread.join().unwrap();
            }
            assert_eq!(drops.load(Ordering::Relaxed), DEPTH);
        }

        #[test]
        fn concurrent_drops() {
            concurrent::<ArcF>();
            concurrent::<SepF>();
            concurrent::<TlcF>();
        }

        /// `RefTlc` used to free a chain one level per flush (every 4096
        /// buffered drops), so on a thread that kept running a deep chain
        /// stayed allocated. One flush now frees all of it.
        #[test]
        fn tlc_flush_frees_a_whole_chain() {
            #[derive(Hash, PartialEq, Eq, Debug)]
            struct Other;
            std::thread::spawn(|| {
                let drops = Arc::new(AtomicUsize::new(0));
                drop(IterativeDrop::new(chain::<TlcF>(false, &drops)));
                assert_eq!(drops.load(Ordering::Relaxed), 0, "freed before a flush");
                let other = <RefTlc<Other> as Reference<Other>>::new(Other);
                for _ in 0..4096 {
                    drop(RefTlc::strong_clone(&other));
                }
                assert_eq!(drops.load(Ordering::Relaxed), DEPTH, "one flush");
            })
            .join()
            .unwrap();
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
            test_tableshared::<Ref, TableSharedDashMap<Data, Ref, DefHasher>>(Default::default);
            test_tableshared::<Ref, TableSharedDashMap<Data, Ref, BadHasher>>(|| {
                TableSharedDashMap::with_hasher(BadHasher::default())
            });

            concurrent_stress(TableSharedDashMap::<Data, Ref, DefHasher>::default());
        }

        #[test]
        fn flurry() {
            test_tableshared::<Ref, TableSharedFlurry<Data, Ref, DefHasher>>(Default::default);
            test_tableshared::<Ref, TableSharedFlurry<Data, Ref, BadHasher>>(|| {
                TableSharedFlurry::with_hasher(BadHasher::default())
            });

            concurrent_stress(TableSharedFlurry::<Data, Ref, DefHasher>::default());
        }

        #[test]
        fn skipmap() {
            test_tableshared::<Ref, TableSharedSkipMap<Data, Ref>>(Default::default);

            concurrent_stress(TableSharedSkipMap::<Data, Ref>::default());
        }

        #[test]
        fn arcswap() {
            test_tableshared::<Ref, TableSharedArcSwap<Data, Ref, DefHasher>>(Default::default);

            concurrent_stress(TableSharedArcSwap::<Data, Ref, DefHasher>::default());
        }
    }
}
