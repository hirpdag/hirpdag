// ==== Metadata Base

pub type HirpdagMetaCountType = u32;
pub type HirpdagMetaHeightType = u16;
pub type HirpdagMetaFlagType = u16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HirpdagMeta {
    count: HirpdagMetaCountType,
    height: HirpdagMetaHeightType,
    flags: HirpdagMetaFlagType,
}

impl HirpdagMeta {
    pub fn zero() -> Self {
        Self {
            count: 0,
            height: 0,
            flags: 0,
        }
    }
    pub fn increment(mut self) -> Self {
        self.count = self.count.saturating_add(1);
        self.height = self.height.saturating_add(1);
        self
    }
    pub fn add_flags(mut self, flag: HirpdagMetaFlagType) -> Self {
        self.flags = self.flags | flag;
        self
    }
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
    pub fn fold_ref<'a>(self, other: &'a Self) -> Self {
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

pub trait HirpdagComputeMeta {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta;
}

impl HirpdagComputeMeta for String {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        HirpdagMeta::zero()
    }
}

impl<'a> HirpdagComputeMeta for &'a str {
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
