//! Memoization tables keyed by hirpdag nodes.
//!
//! A hash-consed node is a stable key: it hashes and compares by interned
//! identity in `O(1)`, whatever the size of the graph below it.  That makes a
//! map from node to result the natural place to keep anything derived from a
//! node — a rewritten node, an analysis result, an annotation — instead of
//! recomputing it once per path that reaches the node, and instead of trying
//! to store it *inside* the node, which hash-consing forbids (see
//! `book/src/ch04-00-techniques.md`).
//!
//! [`HirpdagMemoizeMap`] is one such table, shared across threads.  The
//! interior mutability lives here, outside the nodes: filling the table takes
//! `&self`, so one table can be handed to every thread working on the same
//! graph and each thread sees what the others have already computed.
//!
//! `#[hirpdag_module]` generates a `HirpdagMemoizeCache` holding one table per
//! data type in the module, implementing [`HirpdagMemoize`] for each of them.
//! `HirpdagRewriteMemoized` memoizes rewrite rules through that cache; any
//! other traversal can use it — or its own [`HirpdagMemoizeMap`] — the same way.

/// Number of independent shard locks.  Power-of-two so shard selection is a bitmask (no modulo).
const N_SHARDS: usize = 8;

type DefaultHasher = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

/// A concurrent memoization table: a map from key to computed value that
/// several threads can read and fill at once.
///
/// Like the hash-consing tables, the map is split into [`N_SHARDS`]
/// independently locked shards chosen by the low bits of the key's hash, so
/// threads working on different nodes rarely contend.  No lock is ever held
/// while a value is being computed, so a computation is free to recurse into
/// the same table for the keys it depends on.
///
/// Entries are written only by [`get_or_else`](Self::get_or_else), and only
/// when the key is absent: a memoized value is what the computation *does*
/// return for that key, so an entry that could be replaced by a different one
/// would mean the callers that already read the old value disagree with the
/// ones that read the new. Whatever a key resolves to first, it keeps until
/// the table is [`clear`](Self::clear)ed.
pub struct HirpdagMemoizeMap<K, V, HB = DefaultHasher>
where
    K: std::hash::Hash + std::cmp::Eq + Clone,
    V: Clone,
    HB: std::hash::BuildHasher,
{
    shards: [std::sync::Mutex<std::collections::HashMap<K, V>>; N_SHARDS],
    hash_builder: HB,
}

impl<K, V, HB> HirpdagMemoizeMap<K, V, HB>
where
    K: std::hash::Hash + std::cmp::Eq + Clone,
    V: Clone,
    HB: std::hash::BuildHasher + Default,
{
    pub fn new() -> Self {
        Self {
            shards: std::array::from_fn(
                |_| std::sync::Mutex::new(std::collections::HashMap::new()),
            ),
            hash_builder: HB::default(),
        }
    }
}

impl<K, V, HB> Default for HirpdagMemoizeMap<K, V, HB>
where
    K: std::hash::Hash + std::cmp::Eq + Clone,
    V: Clone,
    HB: std::hash::BuildHasher + Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V, HB> HirpdagMemoizeMap<K, V, HB>
where
    K: std::hash::Hash + std::cmp::Eq + Clone,
    V: Clone,
    HB: std::hash::BuildHasher,
{
    fn get_shard(&self, key: &K) -> &std::sync::Mutex<std::collections::HashMap<K, V>> {
        let hash = self.hash_builder.hash_one(key);
        let mask = (N_SHARDS - 1) as u64;
        &self.shards[(hash & mask) as usize]
    }

    /// The value remembered for `key`, if there is one.
    pub fn get(&self, key: &K) -> Option<V> {
        self.get_shard(key).lock().unwrap().get(key).cloned()
    }

    /// The value remembered for `key`, computing and remembering it first if
    /// this is the first time the table has seen the key.
    ///
    /// This is the only way to fill the table, and it never overwrites an
    /// entry — see the note on the type.
    ///
    /// `compute` runs with no lock held, so it may recurse into this table (a
    /// rewrite rule descending into a node's children does exactly that).  The
    /// cost is that two threads racing on the same key can both compute it;
    /// whichever result lands first is the one kept and returned to both, so
    /// callers still agree on a single value.
    pub fn get_or_else<F>(&self, key: &K, compute: F) -> V
    where
        F: FnOnce() -> V,
    {
        if let Some(hit) = self.get(key) {
            return hit;
        }
        let value = compute();
        self.get_shard(key)
            .lock()
            .unwrap()
            .entry(key.clone())
            .or_insert(value)
            .clone()
    }

    /// Forget everything the table has remembered.
    pub fn clear(&self) {
        for shard in &self.shards {
            shard.lock().unwrap().clear();
        }
    }

    /// How many keys the table currently remembers.
    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| shard.lock().unwrap().len())
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A cache that memoizes values of type `T`, keyed by `T`.
///
/// Implemented by the generated `HirpdagMemoizeCache` once per data type in the
/// module, so `cache.get_or_else(&node, || ..)` finds the table for the node's
/// type.  Code that is generic over the node type can take
/// `&impl HirpdagMemoize<T>`.
pub trait HirpdagMemoize<T>
where
    T: std::hash::Hash + std::cmp::Eq + Clone,
{
    /// The table this cache keeps for values of type `T`.
    fn hirpdag_memoize_map(&self) -> &HirpdagMemoizeMap<T, T>;

    /// The value remembered for `key`, if there is one.
    fn get(&self, key: &T) -> Option<T> {
        self.hirpdag_memoize_map().get(key)
    }

    /// The value remembered for `key`, computing and remembering it first if
    /// this is the first time the cache has seen the key.  The only way to fill
    /// the cache; see [`HirpdagMemoizeMap::get_or_else`].
    fn get_or_else<F>(&self, key: &T, compute: F) -> T
    where
        F: FnOnce() -> T,
    {
        self.hirpdag_memoize_map().get_or_else(key, compute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_or_else_computes_once_per_key() {
        let map: HirpdagMemoizeMap<u32, String> = HirpdagMemoizeMap::new();
        let mut computed = 0;
        for _ in 0..4 {
            let value = map.get_or_else(&7, || {
                computed += 1;
                "seven".to_string()
            });
            assert_eq!(value, "seven");
        }
        assert_eq!(computed, 1);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&7), Some("seven".to_string()));
        assert_eq!(map.get(&8), None);

        map.clear();
        assert!(map.is_empty());
        assert_eq!(map.get(&7), None);
    }

    #[test]
    fn nested_get_or_else_does_not_deadlock() {
        // A computation recursing into the same table is the whole point: a
        // rewrite rule descends into its node's children this way. Walk a chain
        // of keys so the nesting crosses shards.
        let map: HirpdagMemoizeMap<u32, u32> = HirpdagMemoizeMap::new();
        fn sum_to(map: &HirpdagMemoizeMap<u32, u32>, n: u32) -> u32 {
            map.get_or_else(&n, || if n == 0 { 0 } else { n + sum_to(map, n - 1) })
        }
        assert_eq!(sum_to(&map, 32), 32 * 33 / 2);
        assert_eq!(map.len(), 33);
    }

    #[test]
    fn shared_between_threads() {
        let map: HirpdagMemoizeMap<u32, u32> = HirpdagMemoizeMap::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for key in 0..256 {
                        assert_eq!(map.get_or_else(&key, || key * 2), key * 2);
                    }
                });
            }
        });
        assert_eq!(map.len(), 256);
    }
}
