#![forbid(unsafe_code)]

//! The hash-consed data types themselves: a struct's data, its interning
//! table and the public reference type, an enum's declaration, the checks on
//! field types, and the module's `Impl*` type aliases and table reset.
//!
//! Also the field and variant helpers the other features share.

use crate::config::{ModuleConfig, TypeConfig};
use crate::names::DataTypeNames;
use crate::DataTypeEntry;

/// The named fields of a `#[hirpdag]` struct.
///
/// Tuple and unit structs are rejected here rather than panicking: a panic in a
/// proc macro reaches the user as "custom attribute panicked" with no span,
/// while this points at the declaration.
pub fn get_fields_named<'a>(
    input: &syn::DeriveInput,
    input_struct: &'a syn::DataStruct,
) -> syn::Result<&'a syn::FieldsNamed> {
    match &input_struct.fields {
        syn::Fields::Named(n) => Ok(n),
        _ => Err(syn::Error::new_spanned(
            &input.ident,
            "`#[hirpdag]` can only be applied to structs with named fields",
        )),
    }
}

/// Rejects any variant that is not a single-field tuple variant, the only
/// shape `#[hirpdag]` enums take: the generated code matches every variant as
/// `Variant(x)`.
///
/// Runs before anything is generated, so the error points at the variant
/// rather than surfacing as a macro panic (no span) or as type errors inside
/// the expansion.
pub fn check_variants(input_enum: &syn::DataEnum) -> syn::Result<()> {
    for variant in &input_enum.variants {
        let is_single_tuple_field =
            matches!(&variant.fields, syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1);
        if !is_single_tuple_field {
            return Err(syn::Error::new_spanned(
                variant,
                format!(
                    "`#[hirpdag]` enum variants must have exactly one unnamed field, \
                     like `{}(T)`",
                    variant.ident
                ),
            ));
        }
    }
    Ok(())
}

/// The payload type of a single-field tuple variant.
pub fn get_variant_type(variant: &syn::Variant) -> &syn::Type {
    match &variant.fields {
        syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
        _ => unreachable!("variant shapes are checked by check_variants"),
    }
}

/// The fields as a function parameter list: `a: i32, b: String,`.
/// Field visibility and attributes are not valid on parameters.
fn get_fields_parameters(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    let mut parameters = fields_named.named.clone();
    for field in parameters.iter_mut() {
        field.vis = syn::Visibility::Inherited;
        field.attrs.clear();
    }
    quote! { #parameters }
}

fn get_fields_list(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_list = quote! {
    //    a, b, c
    //};
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .map(|field_name| quote! { #field_name, })
        .collect()
}

fn get_default_normalizer(
    config: &TypeConfig,
    fields_named: &syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    if config.has_normalizer() {
        quote! {}
    } else {
        let fields_parameters = get_fields_parameters(fields_named);
        let fields_list = get_fields_list(fields_named);
        quote! {
            pub fn new(#fields_parameters) -> Self {
                Self::spawn(#fields_list)
            }
        }
    }
}

/// A compile-time check that every field type is one hirpdag can hold.
///
/// Each check is spanned to the type it checks, so an unsupported type
/// (`Box<u32>`, or a type of the user's that is not a
/// `hirpdag::base::HirpdagLeaf`) is reported where the field declares it, with
/// the reason spelled out by the field traits' diagnostic, rather than only at
/// the attribute through the generated code that uses the field. The generated
/// code still fails to compile as well; this adds the error that says where
/// and why.
///
/// `HirpdagComputeMeta` stands in for the four field traits: the leaf and
/// container implementations of all four are uniform (see
/// `hirpdag::base::field`), so a type has one exactly when it has the others.
fn get_field_type_checks<'a>(
    field_types: impl Iterator<Item = &'a syn::Type>,
) -> proc_macro2::TokenStream {
    use syn::spanned::Spanned;
    let mut checked = Vec::new();
    for ty in field_types {
        field_check_types(ty, &mut checked);
    }
    let checks = checked.into_iter().map(|ty| {
        quote_spanned! {ty.span()=>
            hirpdag_field::<#ty>();
        }
    });
    quote! {
        #[allow(dead_code)]
        const _: () = {
            fn hirpdag_field<T: hirpdag::base::HirpdagComputeMeta>() {}
            fn hirpdag_check_fields() {
                #(#checks)*
            }
        };
    }
}

/// The types to check for a field of type `ty`: the elements of the
/// containers hirpdag provides (`Option`, `Vec`, tuples), however deeply they
/// nest, and `ty` itself for anything else.
///
/// Checking `Option<(bool, Colour)>` as a whole cannot say which element is at
/// fault: the leaf implementation and the `Option` one both match, so the
/// compiler reports the outer type. Checking the elements puts the error on
/// `Colour`. This reads the syntax, so a container reached through a type
/// alias, or a user type that happens to be named `Option`, is checked as a
/// whole; either way the generated code enforces the real bounds.
fn field_check_types<'a>(ty: &'a syn::Type, out: &mut Vec<&'a syn::Type>) {
    match ty {
        syn::Type::Paren(paren) => field_check_types(&paren.elem, out),
        syn::Type::Group(group) => field_check_types(&group.elem, out),
        syn::Type::Tuple(tuple) if !tuple.elems.is_empty() => {
            for elem in &tuple.elems {
                field_check_types(elem, out);
            }
        }
        syn::Type::Path(path) if path.qself.is_none() => {
            let segment = path.path.segments.last().expect("a path has a segment");
            let single_arg = match &segment.arguments {
                syn::PathArguments::AngleBracketed(args) if args.args.len() == 1 => {
                    match &args.args[0] {
                        syn::GenericArgument::Type(inner) => Some(inner),
                        _ => None,
                    }
                }
                _ => None,
            };
            match single_arg {
                Some(inner) if segment.ident == "Option" || segment.ident == "Vec" => {
                    field_check_types(inner, out)
                }
                _ => out.push(ty),
            }
        }
        _ => out.push(ty),
    }
}

/// A struct's data, its interning table, and the reference type users hold.
pub fn for_struct(
    config: &TypeConfig,
    names: &DataTypeNames,
    fields_named: &syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        struct_data: hirpdag_struct_name,
        table: hirpdag_table_name,
        ..
    } = names;

    //let fields_declarations = quote! {
    //    a: i32,
    //    b: String,
    //    c: Option<#hirpdag_ref_name>,
    //};
    let fields_declarations = &fields_named.named;
    let fields_parameters = get_fields_parameters(fields_named);
    let fields_list = get_fields_list(fields_named);
    let default_normalizer = get_default_normalizer(config, fields_named);
    let field_type_checks = get_field_type_checks(fields_named.named.iter().map(|f| &f.ty));

    quote! {
        #[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
        pub struct #hirpdag_struct_name {
            #fields_declarations
        }

        #field_type_checks

        impl HirpdagStruct for #hirpdag_struct_name {
            type ReferenceStorageStruct = ImplRef<HirpdagStorage<#hirpdag_struct_name>>;
            fn hirpdag_hashcons(self) ->
            HirpdagRef<#hirpdag_struct_name, ImplRef<HirpdagStorage<#hirpdag_struct_name>>> {
                #hirpdag_table_name.hirpdag_hashcons(self)
            }
        }

        // Named after the declaration verbatim (`HIRPDAG_TABLE_Foo`) rather
        // than uppercased, so two types differing only in case cannot name the
        // same table. See hirpdag_derive::names.
        #[allow(non_upper_case_globals)]
        static #hirpdag_table_name: std::sync::LazyLock<HirpdagHashconsTable<
            #hirpdag_struct_name,
            ImplRef<HirpdagStorage<#hirpdag_struct_name>>,
            ImplTableShared<HirpdagStorage<#hirpdag_struct_name>>>> =
                std::sync::LazyLock::new(HirpdagHashconsTable::new);

        #[derive(Hash, Clone, Debug, PartialEq, Eq)]
        pub struct #hirpdag_ref_name(HirpdagRef<#hirpdag_struct_name, ImplRef<HirpdagStorage<#hirpdag_struct_name>>>);

        impl std::ops::Deref for #hirpdag_ref_name {
            type Target = #hirpdag_struct_name;
            fn deref(&self) -> &#hirpdag_struct_name {
                &(*(self.0))
            }
        }

        impl std::cmp::PartialOrd for #hirpdag_ref_name {
            fn partial_cmp(&self, other: &#hirpdag_ref_name) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl std::cmp::Ord for #hirpdag_ref_name {
            /// Semantically-aware ordering based on creation order.
            ///
            /// If node B is a dependency of node A (B must be created before A),
            /// then B < A. Equal if both references point to the same interned node.
            fn cmp(&self, other: &#hirpdag_ref_name) -> std::cmp::Ordering {
                if self == other {
                    std::cmp::Ordering::Equal
                } else {
                    self.0.hirpdag_get_creation_id().cmp(&other.0.hirpdag_get_creation_id())
                }
            }
        }

        impl #hirpdag_ref_name {
            fn spawn(#fields_parameters) -> Self {
                let data = #hirpdag_struct_name { #fields_list };
                Self(data.hirpdag_hashcons())
            }

            /// Deep structural comparison of the underlying data, independent of creation order.
            ///
            /// O(n) in the size of the DAG. Prefer `cmp` (creation-ID based) for
            /// ordering; use this only when structural order is specifically needed.
            pub fn hirpdag_cmp_deep(&self, other: &Self) -> std::cmp::Ordering {
                self.0.hirpdag_cmp_deep(&other.0)
            }

            // If normalizer is not provided, generate one.
            #default_normalizer
        }
    }
}

/// An enum's declaration. Enums are not hashconsed; they are inline payload
/// within their parent node.
pub fn for_enum(names: &DataTypeNames, input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    let name = &names.ref_name;
    //let variants_declarations = quote! {
    //    Foo(i32),
    //    Bar(String),
    //    Baz(Option<MessageA>),
    //};
    let variants_declarations = &input_enum.variants;
    let field_type_checks = get_field_type_checks(input_enum.variants.iter().map(get_variant_type));

    quote! {
        #[derive(Hash, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum #name {
            #variants_declarations
        }

        #field_type_checks
    }
}

/// The `Impl*` type aliases the configuration selects, and
/// `hirpdag_reset_tables()`.
pub fn for_module(config: &ModuleConfig, types: &[DataTypeEntry]) -> proc_macro2::TokenStream {
    // A call to reset each struct type's global interning table. Emitted into
    // the module-level `hirpdag_reset_tables()` below (gated on the downstream
    // crate's `reset-tables` feature).
    let reset_table_calls: proc_macro2::TokenStream = types
        .iter()
        .filter(|entry| entry.names.is_struct())
        .map(|entry| {
            let table_ident = &entry.names.table;
            quote! { #table_ident.reset(); }
        })
        .collect();

    let reference_type: proc_macro2::TokenStream = config.reference_type();
    let reference_weak_type: proc_macro2::TokenStream = config.reference_weak_type();
    let tableshared_type: proc_macro2::TokenStream = config.tableshared_type();
    // Extra `type <name><D> = <rhs>;` helper aliases the config's shared-table
    // strings refer to (e.g. `ImplTable`). Concurrent-collection backends, which
    // are not generic over an inner table, declare none.
    let helper_alias_defs: Vec<proc_macro2::TokenStream> = config
        .helper_aliases()
        .into_iter()
        .map(|(name, ty)| quote! { type #name<D> = #ty; })
        .collect();

    quote! {
        type ImplRef<D> = #reference_type;
        type ImplRefWeak<D> = #reference_weak_type;
        #(#helper_alias_defs)*
        type ImplTableShared<D> = #tableshared_type;

        /// Empty every hash-consing table in this module, so later construction
        /// starts as if nothing had been interned. Gated on the `reset-tables`
        /// feature of the crate this module is compiled in. Intended for
        /// benchmarks and tests; invalidates the hash-consing invariant for
        /// references interned before the call.
        #[cfg(feature = "reset-tables")]
        #[allow(dead_code)]
        pub fn hirpdag_reset_tables() {
            #reset_table_calls
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_types(ty: &str) -> Vec<String> {
        let ty: syn::Type = syn::parse_str(ty).expect("type parses");
        let mut out = Vec::new();
        field_check_types(&ty, &mut out);
        out.iter()
            .map(|t| quote::ToTokens::to_token_stream(t).to_string())
            .collect()
    }

    #[test]
    fn field_checks_look_inside_hirpdag_containers() {
        assert_eq!(checked_types("u32"), ["u32"]);
        assert_eq!(checked_types("Option<(bool, Colour)>"), ["bool", "Colour"]);
        assert_eq!(checked_types("Vec<Vec<Node>>"), ["Node"]);
        assert_eq!(
            checked_types("std::option::Option<(u8, Vec<(char, Node)>)>"),
            ["u8", "char", "Node"]
        );
        assert_eq!(checked_types("((u8))"), ["u8"]);
    }

    #[test]
    fn field_checks_take_other_types_whole() {
        assert_eq!(checked_types("()"), ["()"]);
        assert_eq!(checked_types("Box<u32>"), ["Box < u32 >"]);
        assert_eq!(checked_types("Children"), ["Children"]);
        assert_eq!(
            checked_types("HashMap<u32, Node>"),
            ["HashMap < u32 , Node >"]
        );
    }
}
