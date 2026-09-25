#![forbid(unsafe_code)]

//! Builders: `FooBuilder`, with one setter per field, and the `builder()` and
//! `to_builder()` constructors on the reference type. Structs only; enums and
//! the module have no builder items.

use crate::names::DataTypeNames;

fn get_builder_field_declarations(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let builder_field_declarations = quote! {
    //    a: Option<i32>,
    //    b: Option<String>,
    //    c: Option<Option<MessageA>>,
    //};
    fields_named
        .named
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            quote! { #name: Option<#ty>, }
        })
        .collect()
}

fn get_builder_setters(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let builder_setters = quote! {
    //    pub fn a(mut self, value: i32) -> Self { self.a = Some(value); self }
    //    pub fn b(mut self, value: String) -> Self { self.b = Some(value); self }
    //    pub fn c(mut self, value: Option<MessageA>) -> Self { self.c = Some(value); self }
    //};
    fields_named
        .named
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            quote! {
                pub fn #name(mut self, value: #ty) -> Self {
                    self.#name = Some(value);
                    self
                }
            }
        })
        .collect()
}

fn get_builder_none_fields(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let builder_none_fields = quote! {
    //    a: None,
    //    b: None,
    //    c: None,
    //};
    fields_named
        .named
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().unwrap();
            quote! { #name: None, }
        })
        .collect()
}

fn get_builder_from_node_fields(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let builder_from_node_fields = quote! {
    //    a: Some(node.a.clone()),
    //    b: Some(node.b.clone()),
    //    c: Some(node.c.clone()),
    //};
    fields_named
        .named
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().unwrap();
            quote! { #name: Some(node.#name.clone()), }
        })
        .collect()
}

fn get_builder_build_args(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let builder_build_args = quote! {
    //    self.a.expect("Builder field 'a' not set"),
    //    self.b.expect("Builder field 'b' not set"),
    //    self.c.expect("Builder field 'c' not set"),
    //};
    fields_named
        .named
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().unwrap();
            let msg = format!("Builder field '{}' not set", name);
            quote! { self.#name.expect(#msg), }
        })
        .collect()
}

pub fn for_struct(
    names: &DataTypeNames,
    fields_named: &syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        builder: hirpdag_builder_name,
        ..
    } = names;

    let builder_field_declarations = get_builder_field_declarations(fields_named);
    let builder_setters = get_builder_setters(fields_named);
    let builder_none_fields = get_builder_none_fields(fields_named);
    let builder_from_node_fields = get_builder_from_node_fields(fields_named);
    let builder_build_args = get_builder_build_args(fields_named);

    quote! {
        impl #hirpdag_ref_name {
            pub fn builder() -> #hirpdag_builder_name {
                #hirpdag_builder_name::new()
            }

            pub fn to_builder(&self) -> #hirpdag_builder_name {
                #hirpdag_builder_name::from(self)
            }
        }

        #[derive(Clone, Debug)]
        pub struct #hirpdag_builder_name {
            #builder_field_declarations
        }

        impl #hirpdag_builder_name {
            pub fn new() -> Self {
                Self {
                    #builder_none_fields
                }
            }

            #builder_setters

            pub fn build(self) -> #hirpdag_ref_name {
                #hirpdag_ref_name::new(#builder_build_args)
            }
        }

        impl From<&#hirpdag_ref_name> for #hirpdag_builder_name {
            fn from(node: &#hirpdag_ref_name) -> Self {
                Self {
                    #builder_from_node_fields
                }
            }
        }
    }
}
