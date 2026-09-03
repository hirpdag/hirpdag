#![forbid(unsafe_code)]

//! The identifiers `#[hirpdag_module]` generates for one data type.
//!
//! Every generated item is named after the type it belongs to: a struct `Foo`
//! gets `HirpdagStructFoo`, `HIRPDAG_TABLE_Foo`, `FooBuilder`, `rewrite_Foo`
//! and so on. This module is the only place that convention is written down;
//! the expansion code reads names from here rather than deriving them again.
//!
//! Every derived identifier carries the span of the declaration it was derived
//! from, so an error mentioning `FooBuilder` points at the user's `struct Foo`
//! rather than at the attribute.
//!
//! Names are built by prefixing or suffixing the declared name verbatim, never
//! by transforming its case. A transform would not be injective: uppercasing
//! collapsed `Foo` and `FOO` onto one interning table, and snake-casing
//! collapsed `AB` and `A_b` onto one roots field. Both collisions surfaced as
//! duplicate-definition errors inside expanded code. See
//! `docs/adr/0006-generated-names-from-the-declared-name.md`.

use proc_macro2::Ident;

/// Which kind of `#[hirpdag]` declaration a set of names belongs to.
///
/// Structs are hashconsed and appear as entries in an archive's node table;
/// enums are inline payload data within their parent node. The two kinds name
/// their archived form differently, and only structs use the interning table,
/// builder and roots names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataTypeKind {
    Struct,
    Enum,
}

/// The identifiers generated for one `#[hirpdag]` data type.
///
/// Built once per declaration and carried on its `DataTypeEntry`, so the
/// per-type expansion and the module-level expansion name the same items
/// without either of them spelling the convention.
///
/// A `DataTypeKind::Enum` set still carries the struct-only names
/// (`struct_data`, `table`, `builder`, `roots_field`); the enum expansion
/// never interpolates them, and an identifier that is not interpolated emits
/// no tokens. Only [`archive_form`](Self::archive_form), which differs between
/// the two kinds, is resolved by kind at construction, so no call site can
/// pick the wrong one.
#[derive(Clone)]
pub struct DataTypeNames {
    /// Whether the declaration is a hashconsed struct or an inline enum.
    pub kind: DataTypeKind,
    /// The declared name, which is also the name of the public reference type
    /// users hold: `Foo`.
    pub ref_name: Ident,
    /// The inner struct holding the fields: `HirpdagStructFoo`.
    pub struct_data: Ident,
    /// The archived (plain data) form: `HirpdagArchiveStructFoo` for a struct,
    /// `HirpdagArchiveEnumFoo` for an enum.
    pub archive_form: Ident,
    /// The global interning table static: `HIRPDAG_TABLE_Foo`.
    pub table: Ident,
    /// The rewrite rule and driver method: `rewrite_Foo`.
    pub rewrite_method: Ident,
    /// The memoization cache field for this type: `cache_Foo`.
    pub cache_member: Ident,
    /// The builder type: `FooBuilder`.
    pub builder: Ident,
    /// This type's field on `HirpdagArchiveRoots`, when it is a
    /// `#[hirpdag(root)]`: `root_Foo`.
    pub roots_field: Ident,
}

impl DataTypeNames {
    /// The names generated for a type declared as `declared`.
    pub fn new(declared: &Ident, kind: DataTypeKind) -> Self {
        let span = declared.span();
        let derived = |text: String| Ident::new(&text, span);
        let name = declared.to_string();
        Self {
            kind,
            ref_name: declared.clone(),
            struct_data: derived(format!("HirpdagStruct{name}")),
            archive_form: match kind {
                DataTypeKind::Struct => derived(format!("HirpdagArchiveStruct{name}")),
                DataTypeKind::Enum => derived(format!("HirpdagArchiveEnum{name}")),
            },
            table: derived(format!("HIRPDAG_TABLE_{name}")),
            rewrite_method: derived(format!("rewrite_{name}")),
            cache_member: derived(format!("cache_{name}")),
            builder: derived(format!("{name}Builder")),
            roots_field: derived(format!("root_{name}")),
        }
    }

    /// Whether this is a hashconsed struct (as opposed to an inline enum).
    pub fn is_struct(&self) -> bool {
        self.kind == DataTypeKind::Struct
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::Span;

    fn names_of(name: &str, kind: DataTypeKind) -> DataTypeNames {
        DataTypeNames::new(&Ident::new(name, Span::call_site()), kind)
    }

    #[test]
    fn struct_names() {
        let n = names_of("MessageA", DataTypeKind::Struct);
        assert_eq!(n.ref_name.to_string(), "MessageA");
        assert_eq!(n.struct_data.to_string(), "HirpdagStructMessageA");
        assert_eq!(n.archive_form.to_string(), "HirpdagArchiveStructMessageA");
        assert_eq!(n.table.to_string(), "HIRPDAG_TABLE_MessageA");
        assert_eq!(n.rewrite_method.to_string(), "rewrite_MessageA");
        assert_eq!(n.cache_member.to_string(), "cache_MessageA");
        assert_eq!(n.builder.to_string(), "MessageABuilder");
        assert_eq!(n.roots_field.to_string(), "root_MessageA");
        assert!(n.is_struct());
    }

    #[test]
    fn enum_names_differ_only_in_the_archived_form() {
        let s = names_of("Kind", DataTypeKind::Struct);
        let e = names_of("Kind", DataTypeKind::Enum);
        assert_eq!(e.archive_form.to_string(), "HirpdagArchiveEnumKind");
        assert_eq!(s.archive_form.to_string(), "HirpdagArchiveStructKind");
        assert_eq!(e.rewrite_method.to_string(), s.rewrite_method.to_string());
        assert_eq!(e.ref_name.to_string(), s.ref_name.to_string());
        assert!(!e.is_struct());
    }

    /// The table name used to be uppercased, so `Foo` and `FOO` in one hirpdag
    /// module named the same static.
    #[test]
    fn table_names_survive_a_case_difference() {
        assert_ne!(
            names_of("Foo", DataTypeKind::Struct).table.to_string(),
            names_of("FOO", DataTypeKind::Struct).table.to_string(),
        );
    }

    /// The roots field used to be snake-cased, so `AB` and `A_b` in one hirpdag
    /// module named the same field on `HirpdagArchiveRoots`.
    #[test]
    fn roots_fields_survive_a_case_difference() {
        assert_ne!(
            names_of("AB", DataTypeKind::Struct).roots_field.to_string(),
            names_of("A_b", DataTypeKind::Struct)
                .roots_field
                .to_string(),
        );
    }

    /// Names are prefixed or suffixed, never transformed, so distinct
    /// declarations stay distinct in every family.
    #[test]
    fn every_family_is_distinct_for_distinct_declarations() {
        let a = names_of("Alpha", DataTypeKind::Struct);
        let b = names_of("Beta", DataTypeKind::Struct);
        let family = |n: &DataTypeNames| {
            vec![
                n.ref_name.to_string(),
                n.struct_data.to_string(),
                n.archive_form.to_string(),
                n.table.to_string(),
                n.rewrite_method.to_string(),
                n.cache_member.to_string(),
                n.builder.to_string(),
                n.roots_field.to_string(),
            ]
        };
        for (x, y) in family(&a).iter().zip(family(&b).iter()) {
            assert_ne!(x, y);
        }
    }
}
