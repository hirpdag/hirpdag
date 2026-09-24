#![forbid(unsafe_code)]

//! Node metadata: the `HirpdagComputeMeta` implementations.
//!
//! A struct's metadata is computed from its fields when it is interned and
//! stored with the node, so its reference reads it back rather than
//! recomputing it. An enum's metadata is that of its active variant's payload.
//! There are no module-level items.

use crate::names::DataTypeNames;

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
    names: &DataTypeNames,
    fields_named: &syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        struct_data: hirpdag_struct_name,
        ..
    } = names;
    let fields_compute_meta = get_fields_compute_meta(fields_named);

    quote! {
        impl HirpdagComputeMeta for #hirpdag_struct_name {
            fn hirpdag_compute_meta(&self) -> HirpdagMeta {
                [
                    #fields_compute_meta
                ]
                    .iter()
                    .sum::<HirpdagMeta>()
                    .increment()
                    .add_flags(self.hirpdag_flags())
            }
        }

        impl HirpdagComputeMeta for #hirpdag_ref_name {
            fn hirpdag_compute_meta(&self) -> HirpdagMeta {
                self.0.hirpdag_get_meta().clone()
            }
        }
    }
}

pub fn for_enum(names: &DataTypeNames, input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    let name = &names.ref_name;
    let variants_compute_meta = get_variants_compute_meta(input_enum);

    quote! {
        impl HirpdagComputeMeta for #name {
            fn hirpdag_compute_meta(&self) -> HirpdagMeta {
                use #name::*;
                match self {
                    #variants_compute_meta
                }
            }
        }
    }
}
