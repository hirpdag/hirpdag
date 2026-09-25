#![forbid(unsafe_code)]

//! Node metadata: the `HirpdagComputeMeta` implementations.
//!
//! A struct's metadata is computed from its fields when it is interned and
//! stored with the node, so its reference reads it back rather than
//! recomputing it, and exposes it as `hirpdag_get_meta()`. An enum's metadata
//! is that of its active variant's payload. A type named with
//! `#[hirpdag(flags = ...)]` ORs its own flags in. There are no module-level
//! items.

use crate::config::TypeConfig;
use crate::names::DataTypeNames;

/// The call that ORs a type's own metadata flags into its metadata, from the
/// function `#[hirpdag(flags = ...)]` names; nothing when it names none. The
/// call is spanned at the path, so a function with the wrong signature is
/// reported at the attribute.
fn get_add_flags(config: &TypeConfig) -> proc_macro2::TokenStream {
    use syn::spanned::Spanned;
    match config.flags() {
        Some(path) => quote::quote_spanned! {path.span()=> .add_flags(#path(self)) },
        None => quote! {},
    }
}

fn get_fields_compute_meta(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_compute_meta = quote! {
    //    hirpdag::base::HirpdagComputeMeta::hirpdag_compute_meta(&self.a),
    //    hirpdag::base::HirpdagComputeMeta::hirpdag_compute_meta(&self.b),
    //    hirpdag::base::HirpdagComputeMeta::hirpdag_compute_meta(&self.c),
    //};
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .map(|field_name| {
            quote! { hirpdag::base::HirpdagComputeMeta::hirpdag_compute_meta(&self.#field_name), }
        })
        .collect()
}

fn get_variants_compute_meta(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    //let variants_compute_meta = quote! {
    //    Foo(x) => hirpdag::base::HirpdagComputeMeta::hirpdag_compute_meta(x),
    //    Bar(x) => hirpdag::base::HirpdagComputeMeta::hirpdag_compute_meta(x),
    //    Baz(x) => hirpdag::base::HirpdagComputeMeta::hirpdag_compute_meta(x),
    //};
    input_enum
        .variants
        .iter()
        .map(|t| {
            let variant = &t.ident;
            quote! { #variant(x) => hirpdag::base::HirpdagComputeMeta::hirpdag_compute_meta(x), }
        })
        .collect()
}

pub fn for_struct(
    config: &TypeConfig,
    names: &DataTypeNames,
    fields_named: &syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        struct_data: hirpdag_struct_name,
        ..
    } = names;
    let fields_compute_meta = get_fields_compute_meta(fields_named);
    let add_flags = get_add_flags(config);

    quote! {
        impl HirpdagComputeMeta for #hirpdag_struct_name {
            fn hirpdag_compute_meta(&self) -> HirpdagMeta {
                [
                    #fields_compute_meta
                ]
                    .iter()
                    .sum::<HirpdagMeta>()
                    .increment()
                    #add_flags
            }
        }

        impl HirpdagComputeMeta for #hirpdag_ref_name {
            fn hirpdag_compute_meta(&self) -> HirpdagMeta {
                self.hirpdag_get_meta().clone()
            }
        }

        impl #hirpdag_ref_name {
            /// The metadata cached on this node when it was interned: the size
            /// and height of its subtree and its flags. O(1); nothing is
            /// traversed or cloned.
            pub fn hirpdag_get_meta(&self) -> &HirpdagMeta {
                self.0.hirpdag_get_meta()
            }
        }
    }
}

pub fn for_enum(
    config: &TypeConfig,
    names: &DataTypeNames,
    input_enum: &syn::DataEnum,
) -> proc_macro2::TokenStream {
    let name = &names.ref_name;
    let variants_compute_meta = get_variants_compute_meta(input_enum);
    let variants_meta = if config.flags().is_some() {
        let add_flags = get_add_flags(config);
        quote! {
            let meta = match self {
                #variants_compute_meta
            };
            meta #add_flags
        }
    } else {
        quote! {
            match self {
                #variants_compute_meta
            }
        }
    };

    quote! {
        impl HirpdagComputeMeta for #name {
            fn hirpdag_compute_meta(&self) -> HirpdagMeta {
                use #name::*;
                #variants_meta
            }
        }
    }
}
