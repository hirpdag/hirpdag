//! The field types a `#[hirpdag]` type can hold.
//!
//! The code `#[hirpdag_module]` generates for a data type visits every field
//! four ways: it folds the field's [`HirpdagMeta`] when the node is interned,
//! rewrites it in `default_rewrite`, collects the nodes it references when
//! archiving, and converts it to and from its archived form.  A field type
//! therefore needs all four of [`HirpdagComputeMeta`], [`HirpdagRewritable`],
//! [`HirpdagCollect`] and [`HirpdagArchived`].
//!
//! Field types come in three kinds, and this module is where the first two are
//! written down:
//!
//! - **Leaves**, values with no node inside them: the integers, `bool`, `char`,
//!   `String` and `()`.  A leaf gets all four traits from one marker,
//!   [`HirpdagLeaf`], which a crate can implement for its own leaf types.
//! - **Containers** of field types: `Option`, `Vec`, and tuples of two to four
//!   elements.  Each container visits its contents; nesting is unlimited.
//! - **Nodes and payload enums**, the `#[hirpdag]` types themselves, whose
//!   implementations are generated.
//!
//! `Box` is not a container here.  `Box` is `#[fundamental]`, so a downstream
//! crate may implement [`HirpdagLeaf`] for `Box<TheirType>`, and an
//! implementation for `Box<T>` would then overlap the leaf implementations;
//! the compiler rejects it.  A node reference is already a pointer, so a boxed
//! field has nothing to add.
//!
//! A field of any other type is a compile error, reported at the field:
//!
//! ```compile_fail,E0277
//! use hirpdag::*;
//!
//! #[hirpdag_module]
//! mod datamodel {
//!     #[hirpdag]
//!     pub struct Node {
//!         pub value: Box<u32>,
//!     }
//! }
//! ```

use crate::base::meta::{HirpdagComputeMeta, HirpdagMeta};
use crate::base::rewrite::HirpdagRewritable;
use crate::base::serialize::{
    HirpdagArchived, HirpdagCollect, HirpdagDeserializeError, HirpdagNodeIndex,
    HirpdagSerializeError,
};

/// A field type that holds no hirpdag node: a leaf of the DAG.
///
/// Implementing this is all a type needs to be a field of a `#[hirpdag]` type:
/// a leaf has zero metadata, is cloned unchanged by a rewrite, references no
/// node for the archive to collect, and is archived as itself.
///
/// The supertraits are what the generated code asks of every field.  The data
/// types derive `Clone`, `Debug`, `Hash`, `Eq` and `Ord` over their fields, and
/// the archive hands a leaf to serde as it is.  A type from a crate using
/// hirpdag can derive the serde traits through hirpdag's re-export:
///
/// ```
/// use hirpdag::*;
///
/// #[hirpdag_module]
/// mod datamodel {
///     #[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
///     #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
///     #[serde(crate = "hirpdag::serde")]
///     pub enum Colour {
///         Red,
///         Green,
///     }
///
///     impl hirpdag::base::HirpdagLeaf for Colour {}
///
///     #[hirpdag]
///     pub struct Pixel {
///         pub colour: Colour,
///         pub lit: bool,
///     }
/// }
/// use datamodel::*;
///
/// let p = Pixel::new(Colour::Red, true);
/// assert_eq!(p, Pixel::new(Colour::Red, true));
/// ```
///
/// A type that does hold nodes must not be a leaf: its nodes would be neither
/// rewritten nor archived.  Make it a `#[hirpdag]` type instead.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a field of a `#[hirpdag]` type",
    label = "not a hirpdag field type",
    note = "fields are leaves (integers, `bool`, `char`, `String`, `()`), `#[hirpdag]` types, \
            or `Option`, `Vec` and tuples of fields",
    note = "a type defined in this crate that holds no hirpdag nodes becomes a leaf with \
            `impl hirpdag::base::HirpdagLeaf for TheType {{}}`"
)]
pub trait HirpdagLeaf:
    Clone
    + std::fmt::Debug
    + std::hash::Hash
    + Eq
    + Ord
    + serde::Serialize
    + serde::de::DeserializeOwned
{
}

impl HirpdagLeaf for i8 {}
impl HirpdagLeaf for i16 {}
impl HirpdagLeaf for i32 {}
impl HirpdagLeaf for i64 {}
impl HirpdagLeaf for i128 {}
impl HirpdagLeaf for isize {}
impl HirpdagLeaf for u8 {}
impl HirpdagLeaf for u16 {}
impl HirpdagLeaf for u32 {}
impl HirpdagLeaf for u64 {}
impl HirpdagLeaf for u128 {}
impl HirpdagLeaf for usize {}
impl HirpdagLeaf for bool {}
impl HirpdagLeaf for char {}
impl HirpdagLeaf for String {}
impl HirpdagLeaf for () {}

// ==== Leaves

impl<P: HirpdagLeaf> HirpdagComputeMeta for P {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        HirpdagMeta::zero()
    }
}

impl<T, P: HirpdagLeaf> HirpdagRewritable<T> for P {
    fn hirpdag_rewrite(&self, _driver: &T) -> Self {
        self.clone()
    }
}

impl<C, P: HirpdagLeaf> HirpdagCollect<C> for P {
    fn hirpdag_collect(&self, _ctx: &mut C) {}
}

impl<R: ?Sized, P: HirpdagLeaf> HirpdagArchived<R> for P {
    type Archive = P;
    fn hirpdag_to_archive(&self, _index: &HirpdagNodeIndex) -> Result<P, HirpdagSerializeError> {
        Ok(self.clone())
    }
    fn hirpdag_from_archive(archived: P, _nodes: &R) -> Result<P, HirpdagDeserializeError> {
        Ok(archived)
    }
}

// ==== Option

impl<T: HirpdagComputeMeta> HirpdagComputeMeta for Option<T> {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        self.as_ref()
            .map_or(HirpdagMeta::zero(), |m| m.hirpdag_compute_meta())
    }
}

impl<T, D: HirpdagRewritable<T>> HirpdagRewritable<T> for Option<D> {
    fn hirpdag_rewrite(&self, driver: &T) -> Option<D> {
        self.as_ref().map(|ii| ii.hirpdag_rewrite(driver))
    }
}

impl<C, T: HirpdagCollect<C>> HirpdagCollect<C> for Option<T> {
    fn hirpdag_collect(&self, ctx: &mut C) {
        if let Some(inner) = self {
            inner.hirpdag_collect(ctx);
        }
    }
}

impl<R: ?Sized, T: HirpdagArchived<R>> HirpdagArchived<R> for Option<T> {
    type Archive = Option<T::Archive>;
    fn hirpdag_to_archive(
        &self,
        index: &HirpdagNodeIndex,
    ) -> Result<Self::Archive, HirpdagSerializeError> {
        self.as_ref()
            .map(|v| v.hirpdag_to_archive(index))
            .transpose()
    }
    fn hirpdag_from_archive(
        archived: Self::Archive,
        nodes: &R,
    ) -> Result<Self, HirpdagDeserializeError> {
        archived
            .map(|v| T::hirpdag_from_archive(v, nodes))
            .transpose()
    }
}

// ==== Vec

impl<T: HirpdagComputeMeta> HirpdagComputeMeta for Vec<T> {
    fn hirpdag_compute_meta(&self) -> HirpdagMeta {
        self.iter().map(|m| m.hirpdag_compute_meta()).sum()
    }
}

impl<T, D: HirpdagRewritable<T>> HirpdagRewritable<T> for Vec<D> {
    fn hirpdag_rewrite(&self, driver: &T) -> Vec<D> {
        self.iter().map(|m| m.hirpdag_rewrite(driver)).collect()
    }
}

impl<C, T: HirpdagCollect<C>> HirpdagCollect<C> for Vec<T> {
    fn hirpdag_collect(&self, ctx: &mut C) {
        for item in self {
            item.hirpdag_collect(ctx);
        }
    }
}

impl<R: ?Sized, T: HirpdagArchived<R>> HirpdagArchived<R> for Vec<T> {
    type Archive = Vec<T::Archive>;
    fn hirpdag_to_archive(
        &self,
        index: &HirpdagNodeIndex,
    ) -> Result<Self::Archive, HirpdagSerializeError> {
        self.iter().map(|v| v.hirpdag_to_archive(index)).collect()
    }
    fn hirpdag_from_archive(
        archived: Self::Archive,
        nodes: &R,
    ) -> Result<Self, HirpdagDeserializeError> {
        archived
            .into_iter()
            .map(|v| T::hirpdag_from_archive(v, nodes))
            .collect()
    }
}

// ==== Tuples
//
// A tuple is its elements side by side: metadata folds like a node's fields,
// a rewrite rewrites each element, and the archived form is the tuple of the
// archived elements.

macro_rules! hirpdag_field_tuple {
    ($($t:ident $i:tt),+) => {
        impl<$($t: HirpdagComputeMeta),+> HirpdagComputeMeta for ($($t,)+) {
            fn hirpdag_compute_meta(&self) -> HirpdagMeta {
                [$(self.$i.hirpdag_compute_meta()),+].into_iter().sum()
            }
        }

        impl<D, $($t: HirpdagRewritable<D>),+> HirpdagRewritable<D> for ($($t,)+) {
            fn hirpdag_rewrite(&self, driver: &D) -> Self {
                ($(self.$i.hirpdag_rewrite(driver),)+)
            }
        }

        impl<C, $($t: HirpdagCollect<C>),+> HirpdagCollect<C> for ($($t,)+) {
            fn hirpdag_collect(&self, ctx: &mut C) {
                $(self.$i.hirpdag_collect(ctx);)+
            }
        }

        impl<R: ?Sized, $($t: HirpdagArchived<R>),+> HirpdagArchived<R> for ($($t,)+) {
            type Archive = ($($t::Archive,)+);
            fn hirpdag_to_archive(
                &self,
                index: &HirpdagNodeIndex,
            ) -> Result<Self::Archive, HirpdagSerializeError> {
                Ok(($(self.$i.hirpdag_to_archive(index)?,)+))
            }
            fn hirpdag_from_archive(
                archived: Self::Archive,
                nodes: &R,
            ) -> Result<Self, HirpdagDeserializeError> {
                Ok(($($t::hirpdag_from_archive(archived.$i, nodes)?,)+))
            }
        }
    };
}

hirpdag_field_tuple!(T0 0, T1 1);
hirpdag_field_tuple!(T0 0, T1 1, T2 2);
hirpdag_field_tuple!(T0 0, T1 1, T2 2, T3 3);

#[cfg(test)]
mod tests {
    //! Leaves and containers through the four field traits, with stand-ins for
    //! what the generated code supplies: a driver, a collect context and a node
    //! table.  Nodes themselves are covered end to end in
    //! `test_suite/tests/field_types.rs`.

    use super::*;

    /// A rewrite driver that no leaf may consult.
    struct NoDriver;

    /// A collect context, which leaves and containers of leaves only pass
    /// along.
    struct NoCtx;

    fn round_trip<T>(value: &T) -> T
    where
        T: HirpdagArchived<[()]> + PartialEq + std::fmt::Debug,
    {
        let index = HirpdagNodeIndex::default();
        let archived = value.hirpdag_to_archive(&index).expect("encode");
        let bytes = postcard::to_stdvec(&archived).expect("postcard");
        let decoded = postcard::from_bytes(&bytes).expect("postcard");
        T::hirpdag_from_archive(decoded, &[][..]).expect("decode")
    }

    fn check_field<T>(value: T)
    where
        T: HirpdagComputeMeta
            + HirpdagRewritable<NoDriver>
            + HirpdagCollect<NoCtx>
            + HirpdagArchived<[()]>
            + PartialEq
            + std::fmt::Debug,
    {
        assert_eq!(value.hirpdag_compute_meta(), HirpdagMeta::zero());
        assert_eq!(value.hirpdag_rewrite(&NoDriver), value);
        value.hirpdag_collect(&mut NoCtx);
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn leaves() {
        check_field(-7i8);
        check_field(i128::MIN);
        check_field(u64::MAX);
        check_field(usize::MAX);
        check_field(true);
        check_field('λ');
        check_field("text".to_string());
        check_field(());
    }

    #[test]
    fn containers() {
        check_field(Some(3u32));
        check_field(None::<u32>);
        check_field(vec![1u8, 2, 3]);
        check_field(Vec::<String>::new());
        check_field((1u8, 'x'));
        check_field((false, "s".to_string(), 9i64));
        check_field((1u8, 2u16, 3u32, 4u64));
    }

    #[test]
    fn containers_nest() {
        check_field(Some(vec![(true, Some('a')), (false, None)]));
        check_field(vec![vec![(1u32, ()), (2, ())]]);
    }

    #[test]
    fn a_crate_can_add_a_leaf() {
        #[derive(
            Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
        )]
        enum Colour {
            Red,
            Green,
        }
        impl HirpdagLeaf for Colour {}

        check_field(Colour::Red);
        check_field(vec![(Colour::Green, 1u8)]);
    }
}
