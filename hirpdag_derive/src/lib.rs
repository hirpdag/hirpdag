#![forbid(unsafe_code)]

#[macro_use]
extern crate quote;
extern crate syn;

extern crate proc_macro;
extern crate proc_macro2;

mod archive;
mod builder;
mod config;
mod hashcons;
mod meta;
mod names;
mod rewrite;

use crate::config::{ModuleConfig, TypeConfig};
use crate::names::{DataTypeKind, DataTypeNames};

use proc_macro2::Ident;

/// A hirpdag data type seen in the module.
#[derive(Debug)]
struct DataTypeEntry {
    /// Every identifier generated for this type, derived once from its
    /// declaration. Also carries whether the type is a hashconsed struct (an
    /// entry in the serialized node table) or an inline payload enum.
    names: DataTypeNames,
    /// Root types (`#[hirpdag(root)]`) get a vector in the generated
    /// HirpdagArchiveRoots struct used to serialize and deserialize.
    is_root: bool,
    /// Canonical description of the type definition (name, fields/variants
    /// and their types, root marker). The definitions of all types in the
    /// module, in declaration order, are hashed into the schema fingerprint
    /// embedded in binary archives.
    definition: String,
}

/// Generates hirpdag data structures for an inline module.
///
/// Each struct or enum marked `#[hirpdag]` becomes a hash-consed data type,
/// other items pass through unchanged, and the module-level machinery
/// (rewriting, serialization) is appended. Attribute arguments select the
/// hash-consing configuration: a named `preset = "..."` or the explicit
/// `reference_type`, `reference_weak_type`, `table_type` and
/// `tableshared_type` strings.
///
/// ```ignore
/// #[hirpdag_module]
/// mod datamodel {
///     #[hirpdag]
///     struct Node {
///         children: Vec<Node>,
///     }
/// }
/// ```
///
/// Generated code uses absolute paths (the module needs no imports) and is
/// produced by this single invocation (no state shared between expansions;
/// see docs/adr/0002-module-attribute-macro.md). Outer attribute form only
/// (rust-lang/rust#54726).
#[proc_macro_attribute]
pub fn hirpdag_module(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let config = syn::parse_macro_input!(attr as ModuleConfig);
    let module = syn::parse_macro_input!(input as syn::ItemMod);
    // The one ambient read: cargo sets this for the rustc invocation the macro
    // runs in. Everything below is a function of its arguments.
    let package = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    expand_hirpdag_module(&config, &module, &package)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand_hirpdag_module(
    config: &ModuleConfig,
    module: &syn::ItemMod,
    package: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    let (_, items) = module.content.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(
            module,
            "#[hirpdag_module] requires an inline module: `mod name { ... }`",
        )
    })?;
    let (body, _types) = expand_module_items(config, items, package)?;
    let (inner_attrs, outer_attrs): (Vec<_>, Vec<_>) = module
        .attrs
        .iter()
        .partition(|a| matches!(a.style, syn::AttrStyle::Inner(_)));
    let vis = &module.vis;
    let ident = &module.ident;
    Ok(quote! {
        #(#outer_attrs)*
        #vis mod #ident {
            #(#inner_attrs)*
            #body
        }
    })
}

/// Expands the items of a hirpdag module: structs and enums marked with an
/// inert `#[hirpdag]` attribute become hash-consed data types, other items
/// pass through unchanged, and the module-level code for the given
/// configuration is appended.
///
/// Returns the expanded body together with what the scan learned about the
/// module: one [`DataTypeEntry`] per `#[hirpdag]` declaration, in declaration
/// order. The entries are what the module-level expansion is built from, and
/// they are returned rather than written into a caller's vector so that this
/// function is a value the tests can hold.
fn expand_module_items(
    config: &ModuleConfig,
    items: &[syn::Item],
    package: &str,
) -> syn::Result<(proc_macro2::TokenStream, Vec<DataTypeEntry>)> {
    let mut types: Vec<DataTypeEntry> = Vec::new();
    let mut body = proc_macro2::TokenStream::new();
    for item in items {
        let mut item = item.clone();
        if let Some(attr) = take_hirpdag_attr(&mut item) {
            let type_config = parse_hirpdag_args(&attr)?;
            let input: syn::DeriveInput = match item {
                syn::Item::Struct(s) => s.into(),
                syn::Item::Enum(e) => e.into(),
                _ => unreachable!("take_hirpdag_attr only matches structs and enums"),
            };
            let (tokens, entry) = match &input.data {
                syn::Data::Struct(s) => expand_hirpdag_struct(&type_config, &input, s)?,
                syn::Data::Enum(e) => expand_hirpdag_enum(&type_config, &input, e)?,
                _ => unreachable!(),
            };
            body.extend(tokens);
            types.push(entry);
        } else {
            body.extend(quote! { #item });
        }
    }
    body.extend(expand_hirpdag_end(config, &types, package));
    Ok((body, types))
}

/// If the item is a struct or enum with a `#[hirpdag]` attribute, removes
/// and returns that attribute.
fn take_hirpdag_attr(item: &mut syn::Item) -> Option<syn::Attribute> {
    let attrs = match item {
        syn::Item::Struct(s) => &mut s.attrs,
        syn::Item::Enum(e) => &mut e.attrs,
        _ => return None,
    };
    let position = attrs.iter().position(|a| a.path().is_ident("hirpdag"))?;
    Some(attrs.remove(position))
}

fn parse_hirpdag_args(attr: &syn::Attribute) -> syn::Result<TypeConfig> {
    match &attr.meta {
        syn::Meta::Path(_) => syn::parse2(proc_macro2::TokenStream::new()),
        syn::Meta::List(list) => syn::parse2(list.tokens.clone()),
        syn::Meta::NameValue(nv) => Err(syn::Error::new_spanned(
            nv,
            "unexpected `#[hirpdag = ...]`; use `#[hirpdag]` or `#[hirpdag(...)]`",
        )),
    }
}

// Each data type and the module are expanded feature by feature, and each
// feature module (hashcons, meta, builder, rewrite, archive) provides a
// `for_struct`, `for_enum` and `for_module` for the parts it has.

fn expand_hirpdag_struct(
    config: &TypeConfig,
    input: &syn::DeriveInput,
    input_struct: &syn::DataStruct,
) -> syn::Result<(proc_macro2::TokenStream, DataTypeEntry)> {
    let name: &Ident = &input.ident;
    let names = DataTypeNames::new(name, DataTypeKind::Struct);

    let fields_named = hashcons::get_fields_named(input, input_struct)?;

    let hashcons = hashcons::for_struct(config, &names, fields_named);
    let meta = meta::for_struct(&names, fields_named);
    let builder = builder::for_struct(&names, fields_named);
    let rewrite = rewrite::for_struct(&names, fields_named);
    let archive = archive::for_struct(&names, fields_named);

    let tokens = quote! {
        use hirpdag::base::*;

        #hashcons
        #meta
        #builder
        #rewrite
        #archive
    };

    let entry = DataTypeEntry {
        definition: archive::get_definition_string_struct(
            &name.to_string(),
            config.is_root(),
            fields_named,
        ),
        names,
        is_root: config.is_root(),
    };

    Ok((tokens, entry))
}

fn expand_hirpdag_enum(
    config: &TypeConfig,
    input: &syn::DeriveInput,
    input_enum: &syn::DataEnum,
) -> syn::Result<(proc_macro2::TokenStream, DataTypeEntry)> {
    let name: &Ident = &input.ident;

    if let Some(span) = config.root() {
        return Err(syn::Error::new(
            span,
            "`#[hirpdag(root)]` can only be applied to structs; enums are not hashconsed",
        ));
    }
    if let Some(span) = config.normalizer() {
        return Err(syn::Error::new(
            span,
            "`#[hirpdag(normalizer)]` can only be applied to structs; \
             enums have no constructor to normalize",
        ));
    }

    hashcons::check_variants(input_enum)?;

    let names = DataTypeNames::new(name, DataTypeKind::Enum);

    let hashcons = hashcons::for_enum(&names, input_enum);
    let meta = meta::for_enum(&names, input_enum);
    let rewrite = rewrite::for_enum(&names, input_enum);
    let archive = archive::for_enum(&names, input_enum);

    let tokens = quote! {
        use hirpdag::base::*;

        #hashcons
        #meta
        #rewrite
        #archive
    };

    let entry = DataTypeEntry {
        definition: archive::get_definition_string_enum(&name.to_string(), input_enum),
        names,
        is_root: false,
    };

    Ok((tokens, entry))
}

/// Generates the module-level code for the given configuration from all of
/// the `#[hirpdag]` types in the module: the Impl* type aliases, the
/// HirpdagRewriter trait, memoized rewriting, and the serialization
/// machinery.
fn expand_hirpdag_end(
    config: &ModuleConfig,
    types: &[DataTypeEntry],
    package: &str,
) -> proc_macro2::TokenStream {
    let hashcons = hashcons::for_module(config, types);
    let rewrite = rewrite::for_module(types);
    let archive = archive::for_module(types, package);
    quote! {
        #hashcons
        #rewrite
        #archive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(args: &str) -> ModuleConfig {
        syn::parse_str(args).expect("attribute args parse")
    }

    /// The items of an inline module, as `#[hirpdag_module]` sees them.
    fn items(src: &str) -> Vec<syn::Item> {
        let module: syn::ItemMod = syn::parse_str(src).expect("module parse");
        module.content.expect("inline module").1
    }

    /// What the scan learned about a module: one entry per `#[hirpdag]`
    /// declaration, in declaration order.
    fn scan(src: &str) -> Vec<DataTypeEntry> {
        expand_module_items(&config(""), &items(src), "test_pkg")
            .expect("expansion")
            .1
    }

    const MODULE: &str = r#"
        mod datamodel {
            #[hirpdag(root)]
            struct Item {
                name: String,
                deps: Vec<Item>,
            }

            #[hirpdag]
            enum Kind {
                Num(u32),
                Sum(Vec<Node>),
            }

            #[hirpdag]
            struct Node {
                kind: Kind,
            }

            pub fn not_a_data_type() {}
        }
    "#;

    #[test]
    fn scan_records_every_declaration_in_order() {
        let seen: Vec<(String, bool, bool)> = scan(MODULE)
            .iter()
            .map(|e| (e.names.ref_name.to_string(), e.names.is_struct(), e.is_root))
            .collect();
        assert_eq!(
            seen,
            vec![
                ("Item".to_string(), true, true),
                ("Kind".to_string(), false, false),
                ("Node".to_string(), true, false),
            ]
        );
    }

    #[test]
    fn items_without_the_attribute_are_not_data_types() {
        // `not_a_data_type` is in the module but not in the scan.
        assert_eq!(scan(MODULE).len(), 3);
    }

    #[test]
    fn definition_strings_describe_the_declaration() {
        let types = scan(MODULE);
        assert_eq!(
            types[0].definition,
            "root struct Item;name:String;deps:Vec < Item >"
        );
        assert_eq!(types[1].definition, "enum Kind;Num(u32);Sum(Vec < Node >)");
        assert_eq!(types[2].definition, "struct Node;kind:Kind");
    }

    #[test]
    fn the_root_marker_is_part_of_the_definition() {
        let plain = scan("mod m { #[hirpdag] struct S { a: u32 } }");
        let root = scan("mod m { #[hirpdag(root)] struct S { a: u32 } }");
        assert_eq!(plain[0].definition, "struct S;a:u32");
        assert_eq!(root[0].definition, "root struct S;a:u32");
        assert_ne!(plain[0].definition, root[0].definition);
    }

    #[test]
    fn a_tuple_struct_is_rejected_with_a_message_naming_the_problem() {
        let err = expand_module_items(
            &config(""),
            &items("mod m { #[hirpdag] struct T(i32); }"),
            "test_pkg",
        )
        .expect_err("tuple structs are not hashconsable");
        assert!(
            err.to_string().contains("named fields"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_root_enum_is_rejected() {
        let err = expand_module_items(
            &config(""),
            &items("mod m { #[hirpdag(root)] enum E { A(u32) } }"),
            "test_pkg",
        )
        .expect_err("enums are not hashconsed, so they cannot be roots");
        assert!(
            err.to_string().contains("can only be applied to structs"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn a_non_inline_module_is_rejected() {
        let module: syn::ItemMod = syn::parse_str("mod m;").expect("module parse");
        let err = expand_hirpdag_module(&config(""), &module, "test_pkg")
            .expect_err("there is nothing to expand");
        assert!(
            err.to_string().contains("inline module"),
            "unexpected error: {err}"
        );
    }

    /// The error `expand_module_items` reports for a module.
    fn expansion_error(src: &str) -> String {
        expand_module_items(&config(""), &items(src), "test_pkg")
            .expect_err("expansion should fail")
            .to_string()
    }

    /// A flag set with a value used to be read as set whatever the value, so
    /// `root = false` made a root.
    #[test]
    fn a_flag_with_a_value_is_rejected() {
        for src in [
            "mod m { #[hirpdag(root = false)] struct S { a: u32 } }",
            "mod m { #[hirpdag(root = true)] struct S { a: u32 } }",
            "mod m { #[hirpdag(normalizer = false)] struct S { a: u32 } }",
            "mod m { #[hirpdag(root \"yes\")] struct S { a: u32 } }",
        ] {
            let err = expansion_error(src);
            assert!(err.contains("takes no value"), "{src}: {err}");
        }
        // Bare flags are still accepted.
        assert!(scan("mod m { #[hirpdag(root)] struct S { a: u32 } }")[0].is_root);
    }

    /// The error `#[hirpdag_module(...)]` reports for an argument list.
    fn module_args_error(args: &str) -> String {
        syn::parse_str::<ModuleConfig>(args)
            .err()
            .expect("module arguments should be rejected")
            .to_string()
    }

    /// Each attribute accepts only the arguments it uses. Both used to parse
    /// one shared grammar and ignore the rest, so `normalizer` on the module
    /// or a type string on a struct compiled and did nothing.
    #[test]
    fn an_argument_for_the_other_attribute_is_rejected() {
        for args in ["normalizer", "root", "preset = \"arc_hash_linear\", root"] {
            let err = module_args_error(args);
            assert!(
                err.contains("is a `#[hirpdag(...)]` argument"),
                "{args}: {err}"
            );
        }
        for args in [
            "preset = \"leak_hash_linear\"",
            "reference_type = \"this is not a type\"",
            "reference_weak_type = \"X\"",
            "table_type = \"X\"",
            "tableshared_type = \"X\"",
        ] {
            let src = format!("mod m {{ #[hirpdag({args})] struct S {{ a: u32 }} }}");
            let err = expansion_error(&src);
            assert!(
                err.contains("is a `#[hirpdag_module(...)]` argument"),
                "{args}: {err}"
            );
        }
    }

    #[test]
    fn an_unknown_argument_lists_what_is_accepted() {
        let err = module_args_error("nonsense");
        assert!(err.contains("expected one of: preset,"), "{err}");
        let err = expansion_error("mod m { #[hirpdag(nonsense)] struct S { a: u32 } }");
        assert!(err.contains("expected one of: normalizer, root"), "{err}");
    }

    #[test]
    fn a_type_string_must_parse_as_a_type() {
        for args in [
            "reference_type = \"this is not a type\"",
            "tableshared_type = \"Foo<(\"",
            "reference_type",
            "reference_type = 3",
        ] {
            assert!(syn::parse_str::<ModuleConfig>(args).is_err(), "{args}");
        }
        assert!(syn::parse_str::<ModuleConfig>(
            "reference_type = \"hirpdag::hirpdag_hashconsing::RefArc<D>\""
        )
        .is_ok());
    }

    #[test]
    fn a_normalizer_enum_is_rejected() {
        let err = expansion_error("mod m { #[hirpdag(normalizer)] enum E { A(u32) } }");
        assert!(err.contains("can only be applied to structs"), "{err}");
    }

    /// A raw identifier field used to panic building the rewrite local's name.
    #[test]
    fn a_raw_identifier_field_expands() {
        let (tokens, _) = expand_module_items(
            &config(""),
            &items("mod m { #[hirpdag] struct S { r#type: u32, r#match: Option<S> } }"),
            "test_pkg",
        )
        .expect("expansion");
        let tokens = tokens.to_string();
        assert!(
            tokens.contains("hirpdag_rw_type"),
            "rewrite local not found"
        );
        assert!(
            tokens.contains("hirpdag_rw_match"),
            "rewrite local not found"
        );
    }

    /// Enum variants other than a single tuple field used to panic the macro,
    /// or, for a one-field struct variant, expand into code that did not
    /// compile.
    #[test]
    fn an_enum_variant_of_the_wrong_shape_is_rejected() {
        for src in [
            "mod m { #[hirpdag] enum E { A, B(u32) } }",
            "mod m { #[hirpdag] enum E { A(u32, u32) } }",
            "mod m { #[hirpdag] enum E { A { x: u32 } } }",
            "mod m { #[hirpdag] enum E { A(), B(u32) } }",
        ] {
            let err = expansion_error(src);
            assert!(err.contains("exactly one unnamed field"), "{src}: {err}");
        }
    }

    /// The package name is passed in rather than read from the environment, so
    /// the expansion is a function of its arguments.
    #[test]
    fn the_package_name_reaches_the_schema_name() {
        let (tokens, _) = expand_module_items(
            &config(""),
            &items("mod m { #[hirpdag] struct S { a: u32 } }"),
            "some_package",
        )
        .expect("expansion");
        assert!(
            tokens.to_string().contains("\"some_package:S\""),
            "schema name not found in the expansion"
        );
    }
}
