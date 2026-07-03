// ==== Metadata Base

/// Total node count in a subtree (saturating u32; capped rather than overflowing).
pub type HirpdagMetaCountType = u32;
/// Height of a node's subtree — distance from the node to its deepest leaf (saturating u16).
pub type HirpdagMetaHeightType = u16;
/// Bitfield of user-defined flags propagated upward through the DAG via bitwise OR.
pub type HirpdagMetaFlagType = u16;

/// Aggregated structural metadata cached on every interned node.
///
/// Computed bottom-up at intern time via [`HirpdagComputeMeta`] and stored inside
/// [`HirpdagStorage`](crate::base::reference::HirpdagStorage).  Reading any field is O(1);
/// no DAG traversal is needed after the node is created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HirpdagMeta {
    count: HirpdagMetaCountType,
    height: HirpdagMetaHeightType,
    flags: HirpdagMetaFlagType,
}

impl HirpdagMeta {
    /// Returns the zero / leaf metadata: count=0, height=0, flags=0.
    ///
    /// Used as the initial accumulator and as the metadata for terminal values
    /// (numbers, strings) that contain no child nodes.
    pub fn zero() -> Self {
        Self {
            count: 0,
            height: 0,
            flags: 0,
        }
    }

    /// Wraps a child's metadata to represent one level higher in the DAG.
    ///
    /// Increments both count and height by 1 (saturating).  Call this when a node
    /// has exactly one child and no siblings to merge with.
    pub fn increment(mut self) -> Self {
        self.count = self.count.saturating_add(1);
        self.height = self.height.saturating_add(1);
        self
    }

    /// Sets additional flag bits (bitwise OR into the existing flags).
    pub fn add_flags(mut self, flag: HirpdagMetaFlagType) -> Self {
        self.flags |= flag;
        self
    }

    /// Merges the metadata of two sibling subtrees.
    ///
    /// Counts are summed (saturating), height takes the maximum, flags are unioned.
    /// Used as the fold step when iterating over a node's children.
    pub fn fold(self, other: Self) -> Self {
        let count = self.count.saturating_add(other.count);
        let height = std::cmp::max(self.height, other.height);
        let flags = self.flags | other.flags;
        Self {
            count,
            height,
            flags,
        }
    }

    /// Like [`fold`](Self::fold) but borrows `other` rather than taking ownership.
    ///
    /// Useful in iterator chains where `other` must remain valid (e.g. `fold_ref` as the
    /// accumulator callback in `Iterator::fold`).
    pub fn fold_ref(self, other: &Self) -> Self {
        let count = self.count.saturating_add(other.count);
        let height = std::cmp::max(self.height, other.height);
        let flags = self.flags | other.flags;
        Self {
            count,
            height,
            flags,
        }
    }
    pub fn get_count(&self) -> HirpdagMetaCountType {
        self.count
    }
    pub fn get_height(&self) -> HirpdagMetaHeightType {
        self.height
    }
    pub fn get_flags(&self) -> HirpdagMetaFlagType {
        self.flags
    }
}

impl std::iter::Sum for HirpdagMeta {
    fn sum<I>(iter: I) -> Self
    where
        I: Iterator<Item = Self>,
    {
        iter.fold(Self::zero(), Self::fold)
    }
}

impl<'a> std::iter::Sum<&'a HirpdagMeta> for HirpdagMeta {
    fn sum<I>(iter: I) -> Self
    where
        I: Iterator<Item = &'a Self>,
    {
        iter.fold(Self::zero(), Self::fold_ref)
    }
}

/// Implemented by every field type to compute its metadata contribution.
///
/// The macro-generated `hirpdag_compute_meta` for each struct folds together the results
/// from all fields.  Leaf types (numbers, strings) return [`HirpdagMeta::zero`]; child
/// `HirpdagRef` fields return their cached metadata.
pub trait HirpdagComputeMeta {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta;
}

impl HirpdagComputeMeta for String {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        HirpdagMeta::zero()
    }
}

impl HirpdagComputeMeta for &str {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        HirpdagMeta::zero()
    }
}

impl<T: HirpdagComputeMeta> HirpdagComputeMeta for Option<T> {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        self.as_ref()
            .map_or(HirpdagMeta::zero(), |m| m.hirpdag_compute_meta())
    }
}

impl<T: HirpdagComputeMeta> HirpdagComputeMeta for Vec<T> {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        self.iter().map(|m| m.hirpdag_compute_meta()).sum()
    }
}

use crate::base::basic_traits::IsNumber;
impl<P: IsNumber> HirpdagComputeMeta for P {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        HirpdagMeta::zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_meta_i32() {
        let i = 14i32;
        let meta = i.hirpdag_compute_meta();
        assert_eq!(meta, HirpdagMeta::zero());
    }

    #[test]
    fn test_meta_fold() {
        let meta1 = HirpdagMeta {
            count: 2,
            height: 2,
            flags: 3,
        };
        let meta2 = HirpdagMeta {
            count: 7,
            height: 3,
            flags: 5,
        };
        let meta = meta1.fold(meta2);
        assert_eq!(
            meta,
            HirpdagMeta {
                count: 9,
                height: 3,
                flags: 7
            }
        );
    }

    #[test]
    fn test_meta_fold_saturated() {
        let meta1 = HirpdagMeta {
            count: HirpdagMetaCountType::max_value() - 10,
            height: 2,
            flags: HirpdagMetaFlagType::max_value(),
        };
        let meta2 = HirpdagMeta {
            count: 17,
            height: 3,
            flags: 5,
        };
        let meta = meta1.fold(meta2);
        assert_eq!(
            meta,
            HirpdagMeta {
                count: HirpdagMetaCountType::max_value(),
                height: 3,
                flags: HirpdagMetaFlagType::max_value()
            }
        );
    }

    #[test]
    fn test_meta_increment() {
        let meta1 = HirpdagMeta {
            count: 9,
            height: 3,
            flags: 7,
        };
        let meta = meta1.increment();
        assert_eq!(
            meta,
            HirpdagMeta {
                count: 10,
                height: 4,
                flags: 7
            }
        );
    }

    #[test]
    fn test_meta_increment_saturated() {
        let meta1 = HirpdagMeta {
            count: HirpdagMetaCountType::max_value(),
            height: HirpdagMetaHeightType::max_value(),
            flags: 7,
        };
        let meta = meta1.increment();
        assert_eq!(
            meta,
            HirpdagMeta {
                count: HirpdagMetaCountType::max_value(),
                height: HirpdagMetaHeightType::max_value(),
                flags: 7
            }
        );
    }
}
