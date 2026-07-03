#![forbid(unsafe_code)]

#[macro_use]
extern crate quote;
extern crate syn;

extern crate proc_macro;
extern crate proc_macro2;

mod config;

use crate::config::{HirpdagArgs, HirpdagConfig};

use proc_macro2::{Ident, Span};

#[macro_use]
extern crate lazy_static;

/// A hirpdag data type seen in the module.
struct DataTypeEntry {
    name: String,
    /// Struct types are hashconsed and appear as entries in the serialized
    /// node table. Enum types are inline payload data within their parent.
    is_struct: bool,
    /// Root types (`#[hirpdag(root)]`) get a vector in the generated
    /// HirpdagArchiveRoots struct used to serialize and deserialize.
    is_root: bool,
}

lazy_static! {
    /// Collects all of the Hirpdag data types seen in the module.
    ///
    /// This will be used from the HirpdagEnd to generate code which can operate on all of them.
    static ref DATA_TYPES: std::sync::Mutex<Vec<DataTypeEntry>> = std::sync::Mutex::new(Vec::new());
}

#[proc_macro_attribute]
pub fn hirpdag(
    attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attrs = syn::parse_macro_input!(attr as HirpdagArgs);
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    expand_hirpdag(&attrs, &input).into()
}

fn expand_hirpdag(attrs: &HirpdagArgs, input: &syn::DeriveInput) -> proc_macro2::TokenStream {
    let config = HirpdagConfig::from(attrs);
    match &input.data {
        syn::Data::Struct(s) => expand_hirpdag_struct(&config, input, s),
        syn::Data::Enum(e) => expand_hirpdag_enum(&config, input, e),
        _ => panic!("`#[Hirpdag]` can only be applied to named structs and enums"),
    }
}

fn get_fields_named(input_struct: &syn::DataStruct) -> &syn::FieldsNamed {
    match &input_struct.fields {
        syn::Fields::Named(n) => n,
        _ => panic!("`#[Hirpdag]` can only be applied to named structs and enums"),
    }
}

fn get_fields_declarations(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_declarations = quote! {
    //    a: i32,
    //    b: String,
    //    c: Option<#hirpdag_ref_name>,
    //};
    let fields_declarations = fields_named.named.clone();
    quote! { #fields_declarations }
}

fn get_fields_list(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    use quote::TokenStreamExt;
    //let fields_list = quote! {
    //    a, b, c
    //};
    let fields_list: proc_macro2::TokenStream = fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.append(t.clone());
            s.append(proc_macro2::Punct::new(',', proc_macro2::Spacing::Alone));
            s
        });
    fields_list
}

fn get_fields_compute_meta(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_compute_meta = quote! {
    //    self.a.hirpdag_compute_meta(),
    //    self.b.hirpdag_compute_meta(),
    //    self.c.hirpdag_compute_meta(),
    //};
    let fields_compute_meta: proc_macro2::TokenStream = fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .fold(proc_macro2::TokenStream::new(), |mut s, field_name| {
            s.extend(quote! { self.#field_name.hirpdag_compute_meta(), });
            s
        });
    fields_compute_meta
}

fn get_fields_rewrite(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_rewrite = quote! {
    //    rewriter.rewrite(&self.a),
    //    rewriter.rewrite(&self.b),
    //    rewriter.rewrite(&self.c),
    //};
    let fields_rewrite: proc_macro2::TokenStream = fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .fold(proc_macro2::TokenStream::new(), |mut s, field_name| {
            s.extend(quote! { rewriter.rewrite(&self.#field_name), });
            s
        });
    quote! { #fields_rewrite }
}

fn get_fields_collect(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_collect = quote! {
    //    hirpdag::base::HirpdagCollect::hirpdag_collect(&self.a, ctx);
    //    hirpdag::base::HirpdagCollect::hirpdag_collect(&self.b, ctx);
    //    hirpdag::base::HirpdagCollect::hirpdag_collect(&self.c, ctx);
    //};
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .fold(proc_macro2::TokenStream::new(), |mut s, field_name| {
            s.extend(quote! {
                hirpdag::base::HirpdagCollect::hirpdag_collect(&self.#field_name, ctx);
            });
            s
        })
}

fn get_builder_field_declarations(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let builder_field_declarations = quote! {
    //    a: Option<i32>,
    //    b: Option<String>,
    //    c: Option<Option<MessageA>>,
    //};
    fields_named
        .named
        .iter()
        .fold(proc_macro2::TokenStream::new(), |mut s, field| {
            let name = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            s.extend(quote! { #name: Option<#ty>, });
            s
        })
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
        .fold(proc_macro2::TokenStream::new(), |mut s, field| {
            let name = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            s.extend(quote! {
                pub fn #name(mut self, value: #ty) -> Self {
                    self.#name = Some(value);
                    self
                }
            });
            s
        })
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
        .fold(proc_macro2::TokenStream::new(), |mut s, field| {
            let name = field.ident.as_ref().unwrap();
            s.extend(quote! { #name: None, });
            s
        })
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
        .fold(proc_macro2::TokenStream::new(), |mut s, field| {
            let name = field.ident.as_ref().unwrap();
            s.extend(quote! { #name: Some(node.#name.clone()), });
            s
        })
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
        .fold(proc_macro2::TokenStream::new(), |mut s, field| {
            let name = field.ident.as_ref().unwrap();
            let msg = format!("Builder field '{}' not set", name);
            s.extend(quote! { self.#name.expect(#msg), });
            s
        })
}

fn get_default_normalizer(
    config: &HirpdagConfig,
    fields_named: &syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    if config.has_normalizer() {
        quote! {}
    } else {
        let fields_declarations = get_fields_declarations(fields_named);
        let fields_list = get_fields_list(fields_named);
        quote! {
            pub fn new(#fields_declarations) -> Self {
                Self::spawn(#fields_list)
            }
        }
    }
}

fn expand_hirpdag_struct(
    config: &HirpdagConfig,
    input: &syn::DeriveInput,
    input_struct: &syn::DataStruct,
) -> proc_macro2::TokenStream {
    let name: &Ident = &input.ident;

    let name_str = name.to_string();
    let name_uppercase_str = name_str.to_ascii_uppercase();

    {
        let mut guard = DATA_TYPES.lock().unwrap();
        guard.push(DataTypeEntry {
            name: name_str.clone(),
            is_struct: true,
            is_root: config.is_root(),
        });
    }

    let hirpdag_ref_name_str = name_str.to_string();
    let hirpdag_ref_name = Ident::new(&hirpdag_ref_name_str, Span::call_site());

    let hirpdag_struct_name_str = format!("HirpdagStruct{}", name_str);
    let hirpdag_struct_name = Ident::new(&hirpdag_struct_name_str, Span::call_site());

    let hirpdag_table_name_str = format!("HIRPDAG_TABLE_{}", name_uppercase_str);
    let hirpdag_table_name = Ident::new(&hirpdag_table_name_str, Span::call_site());

    let hirpdag_rewrite_method_name_str = format!("rewrite_{}", name_str);
    let hirpdag_rewrite_method_name =
        Ident::new(&hirpdag_rewrite_method_name_str, Span::call_site());

    let hirpdag_builder_name_str = format!("{}Builder", name_str);
    let hirpdag_builder_name = Ident::new(&hirpdag_builder_name_str, Span::call_site());

    let fields_named = get_fields_named(input_struct);
    let fields_declarations = get_fields_declarations(fields_named);
    let fields_list = get_fields_list(fields_named);
    let fields_compute_meta = get_fields_compute_meta(fields_named);
    let fields_rewrite = get_fields_rewrite(fields_named);
    let fields_collect = get_fields_collect(fields_named);

    let msg_outside_ser_session = format!(
        "hirpdag ref {} serialized outside a hirpdag serialization session",
        name_str
    );
    let msg_not_collected = format!(
        "hirpdag ref {} was not collected before serialization",
        name_str
    );
    let msg_outside_de_session = format!(
        "hirpdag ref {} deserialized outside a hirpdag deserialization session",
        name_str
    );
    let msg_node_type_mismatch = format!("hirpdag node type mismatch: expected {}", name_str);

    let builder_field_declarations = get_builder_field_declarations(fields_named);
    let builder_setters = get_builder_setters(fields_named);
    let builder_none_fields = get_builder_none_fields(fields_named);
    let builder_from_node_fields = get_builder_from_node_fields(fields_named);
    let builder_build_args = get_builder_build_args(fields_named);

    let default_normalizer = get_default_normalizer(config, fields_named);

    quote! {
        use hirpdag::base::*;

        #[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde")]
        pub struct #hirpdag_struct_name {
            #fields_declarations
        }

        impl HirpdagStruct for #hirpdag_struct_name {
            type ReferenceStorageStruct = ImplRef<HirpdagStorage<#hirpdag_struct_name>>;
            fn hirpdag_hashcons(self) ->
            HirpdagRef<#hirpdag_struct_name, ImplRef<HirpdagStorage<#hirpdag_struct_name>>> {
                #hirpdag_table_name.hirpdag_hashcons(self)
            }
        }

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

        hirpdag::lazy_static! {
            static ref #hirpdag_table_name: HirpdagHashconsTable<
            #hirpdag_struct_name,
            ImplRef<HirpdagStorage<#hirpdag_struct_name>>,
            ImplTable<HirpdagStorage<#hirpdag_struct_name>>,
            ImplTableShared<HirpdagStorage<#hirpdag_struct_name>>> =
                HirpdagHashconsTable::new(
                  ImplBuildTableShared::<HirpdagStorage::<#hirpdag_struct_name>>::default()
                );
        }

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

        impl HirpdagComputeMeta for #hirpdag_ref_name {
            fn hirpdag_compute_meta(&self) -> HirpdagMeta {
                self.0.hirpdag_get_meta().clone()
            }
        }

        impl #hirpdag_ref_name {
            fn spawn(#fields_declarations) -> Self {
                let data = #hirpdag_struct_name { #fields_list };
                Self(data.hirpdag_hashcons())
            }

            /// Deep structural comparison of the underlying data, independent of creation order.
            ///
            /// This is the previous default `Ord` behaviour. It is O(n) in the size of the DAG.
            /// Prefer `cmp` (creation-ID based) for ordering; use this only when structural
            /// order is specifically needed.
            pub fn hirpdag_cmp_deep(&self, other: &Self) -> std::cmp::Ordering {
                self.0.hirpdag_cmp_deep(&other.0)
            }

            // If normalizer is not provided, generate one.
            #default_normalizer

            #[allow(non_snake_case)]
            fn default_rewrite<T: HirpdagRewriter>(&self, rewriter: &T) -> Self {
                Self::new(
                    #fields_rewrite
                    )
            }

            pub fn builder() -> #hirpdag_builder_name {
                #hirpdag_builder_name::new()
            }

            pub fn to_builder(&self) -> #hirpdag_builder_name {
                #hirpdag_builder_name::from(self)
            }
        }

        // ==== Builder

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

        // ==== Rewriting

        impl<T: HirpdagRewriter> HirpdagRewritable<T> for #hirpdag_ref_name {
            fn hirpdag_rewrite(&self, rewriter: &T) -> Self {
                rewriter.#hirpdag_rewrite_method_name(self)
            }
        }

        // ==== Serialization
        //
        // A ref serializes as a u64 index into the archive's node table,
        // resolved through the thread-local session opened by the
        // hirpdag_serialize/hirpdag_deserialize entry points generated by
        // #[hirpdag_end]. Refs cannot be (de)serialized outside a session.

        impl hirpdag::serde::Serialize for #hirpdag_ref_name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: hirpdag::serde::Serializer,
            {
                let creation_id = self.0.hirpdag_get_creation_id();
                HIRPDAG_SER_SESSION.with(|cell| {
                    let borrow = cell.borrow();
                    let index_map = borrow.as_ref().ok_or_else(|| {
                        <S::Error as hirpdag::serde::ser::Error>::custom(#msg_outside_ser_session)
                    })?;
                    let index = index_map.get(&creation_id).ok_or_else(|| {
                        <S::Error as hirpdag::serde::ser::Error>::custom(#msg_not_collected)
                    })?;
                    serializer.serialize_u64(*index)
                })
            }
        }

        impl<'de> hirpdag::serde::Deserialize<'de> for #hirpdag_ref_name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: hirpdag::serde::Deserializer<'de>,
            {
                let index: u64 =
                    <u64 as hirpdag::serde::Deserialize>::deserialize(deserializer)?;
                HIRPDAG_DE_SESSION.with(|cell| {
                    let borrow = cell.borrow();
                    let nodes = borrow.as_ref().ok_or_else(|| {
                        <D::Error as hirpdag::serde::de::Error>::custom(#msg_outside_de_session)
                    })?;
                    // Nodes are stored children-first, so a valid archive only
                    // ever references nodes that are already reconstructed.
                    // A forward reference is indistinguishable from an
                    // out-of-range index here, and both are rejected.
                    if index >= nodes.len() as u64 {
                        return Err(<D::Error as hirpdag::serde::de::Error>::custom(format!(
                            "hirpdag node index {} is invalid (out of range or forward reference)",
                            index
                        )));
                    }
                    match &nodes[index as usize] {
                        HirpdagNodeRef::#hirpdag_ref_name(r) => Ok(r.clone()),
                        #[allow(unreachable_patterns)]
                        _ => Err(<D::Error as hirpdag::serde::de::Error>::custom(
                            #msg_node_type_mismatch,
                        )),
                    }
                })
            }
        }

        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for #hirpdag_ref_name {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                let creation_id = self.0.hirpdag_get_creation_id();
                if ctx.seen.contains_key(&creation_id) {
                    return;
                }
                // Post-order DFS: register children before their parent so
                // every child's node index is smaller than its parent's.
                hirpdag::base::HirpdagCollect::hirpdag_collect(&(**self), ctx);
                let index = ctx.nodes.len() as u64;
                ctx.nodes
                    .push(HirpdagArchiveNode::#hirpdag_ref_name((**self).clone()));
                ctx.seen.insert(creation_id, index);
            }
        }

        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for #hirpdag_struct_name {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                #fields_collect
            }
        }
    }
}

fn get_variants_declarations(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    //let variants_declarations = quote! {
    //    Foo(i32),
    //    Bar(String),
    //    Baz(Option<MessageA>),
    //};
    let variants_declarations = input_enum.variants.clone();
    quote! { #variants_declarations }
}

fn get_variants_compute_meta(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    //let variants_compute_meta = quote! {
    //    Foo(a) => a.hirpdag_compute_meta(),
    //    Bar(a) => a.hirpdag_compute_meta(),
    //    Baz(a) => a.hirpdag_compute_meta(),
    //};
    let variants_compute_meta: proc_macro2::TokenStream =
        input_enum
            .variants
            .iter()
            .fold(proc_macro2::TokenStream::new(), |mut s, t| {
                let variant = t.ident.clone();
                s.extend(quote! { #variant(x) => x.hirpdag_compute_meta(), });
                s
            });
    variants_compute_meta
}

fn get_variants_collect(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    //let variants_collect = quote! {
    //    Foo(x) => hirpdag::base::HirpdagCollect::hirpdag_collect(x, ctx),
    //    Bar(x) => hirpdag::base::HirpdagCollect::hirpdag_collect(x, ctx),
    //    Baz(x) => hirpdag::base::HirpdagCollect::hirpdag_collect(x, ctx),
    //};
    input_enum
        .variants
        .iter()
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            let variant = t.ident.clone();
            s.extend(quote! {
                #variant(x) => hirpdag::base::HirpdagCollect::hirpdag_collect(x, ctx),
            });
            s
        })
}

fn get_variants_rewrite(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    //let variants_compute_meta = quote! {
    //    Foo(x) => Foo(rewriter.rewrite(&x)),
    //    Bar(x) => Bar(rewriter.rewrite(&x)),
    //    Baz(x) => Baz(rewriter.rewrite(&x)),
    //};
    let variants_rewrite: proc_macro2::TokenStream =
        input_enum
            .variants
            .iter()
            .fold(proc_macro2::TokenStream::new(), |mut s, t| {
                let variant = t.ident.clone();
                s.extend(quote! { #variant(x) => #variant(rewriter.rewrite(&x)), });
                s
            });
    variants_rewrite
}

fn expand_hirpdag_enum(
    config: &HirpdagConfig,
    input: &syn::DeriveInput,
    input_enum: &syn::DataEnum,
) -> proc_macro2::TokenStream {
    let name: &Ident = &input.ident;

    let name_str = name.to_string();

    if config.is_root() {
        panic!("`#[hirpdag(root)]` can only be applied to structs; enums are not hashconsed");
    }

    {
        let mut guard = DATA_TYPES.lock().unwrap();
        guard.push(DataTypeEntry {
            name: name_str.clone(),
            is_struct: false,
            is_root: false,
        });
    }

    let hirpdag_rewrite_method_name_str = format!("rewrite_{}", name_str);
    let hirpdag_rewrite_method_name =
        Ident::new(&hirpdag_rewrite_method_name_str, Span::call_site());

    let variants_declarations = get_variants_declarations(input_enum);
    let variants_compute_meta = get_variants_compute_meta(input_enum);
    let variants_rewrite = get_variants_rewrite(input_enum);
    let variants_collect = get_variants_collect(input_enum);

    quote! {
        use hirpdag::base::*;

        #[derive(Hash, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde")]
        pub enum #name {
            #variants_declarations
        }

        impl HirpdagComputeMeta for #name {
            fn hirpdag_compute_meta(&self) -> HirpdagMeta {
                use #name::*;
                match self {
                    #variants_compute_meta
                }
            }
        }

        impl #name {
            #[allow(non_snake_case)]
            fn default_rewrite<T: HirpdagRewriter>(&self, rewriter: &T) -> Self {
                use #name::*;
                match self {
                    #variants_rewrite
                }
            }
        }

        impl<T: HirpdagRewriter> HirpdagRewritable<T> for #name {
            fn hirpdag_rewrite(&self, rewriter: &T) -> Self {
                rewriter.#hirpdag_rewrite_method_name(self)
            }
        }

        // ==== Serialization
        //
        // Enum data types are not hashconsed; they are inline payload within
        // their parent node. Collect just recurses into the active variant.

        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for #name {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                use #name::*;
                match self {
                    #variants_collect
                }
            }
        }
    }
}

fn get_rewrite_datatype(name: &str) -> proc_macro2::TokenStream {
    //let rewrite_datatype = quote! {
    //    #[allow(non_snake_case)]
    //    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA {
    //        MessageA::default_rewrite<Self>(x, self)
    //    }
    //};
    let hirpdag_ref_name = Ident::new(name, Span::call_site());

    let hirpdag_rewrite_method_name_str = format!("rewrite_{}", name);
    let hirpdag_rewrite_method_name =
        Ident::new(&hirpdag_rewrite_method_name_str, Span::call_site());

    quote! {

        #[allow(non_snake_case)]
        fn #hirpdag_rewrite_method_name(&self, x: &#hirpdag_ref_name) -> #hirpdag_ref_name {
            #hirpdag_ref_name::default_rewrite::<Self>(x, self)
        }

    }
}

fn get_cache_member(name: &str) -> proc_macro2::TokenStream {
    //let cache_member = quote! {
    //    cache_MessageA: std::collections::HashMap<MessageA, MessageA>,
    //};
    let hirpdag_ref_name = Ident::new(name, Span::call_site());

    let hirpdag_cache_member_name_str = format!("cache_{}", name);
    let hirpdag_cache_member_name = Ident::new(&hirpdag_cache_member_name_str, Span::call_site());

    quote! {
        #hirpdag_cache_member_name: std::collections::HashMap<#hirpdag_ref_name, #hirpdag_ref_name>,
    }
}

fn get_cache_member_new(name: &str) -> proc_macro2::TokenStream {
    //let cache_member_new = quote! {
    //    cache_MessageA: std::collections::HashMap::new(),
    //};
    let hirpdag_cache_member_name_str = format!("cache_{}", name);
    let hirpdag_cache_member_name = Ident::new(&hirpdag_cache_member_name_str, Span::call_site());

    quote! {
        #hirpdag_cache_member_name: std::collections::HashMap::new(),
    }
}

fn get_cache_rewrite(name: &str) -> proc_macro2::TokenStream {
    //let cache_rewrite = quote! {
    //    #[allow(non_snake_case)]
    //    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA {
    //        cache_MessageA.get(x).unwrap_or_else(|x| {
    //          MessageA::default_rewrite<Self>(x, self)
    //        };
    //    }
    //};
    let hirpdag_ref_name = Ident::new(name, Span::call_site());

    let hirpdag_cache_member_name_str = format!("cache_{}", name);
    let hirpdag_cache_member_name = Ident::new(&hirpdag_cache_member_name_str, Span::call_site());

    let hirpdag_rewrite_method_name_str = format!("rewrite_{}", name);
    let hirpdag_rewrite_method_name =
        Ident::new(&hirpdag_rewrite_method_name_str, Span::call_site());

    quote! {

        #[allow(non_snake_case)]
        fn #hirpdag_rewrite_method_name(&self, x: &#hirpdag_ref_name) -> #hirpdag_ref_name {
            self.#hirpdag_cache_member_name
                .get(x)
                .map(|v| {
                    v.clone()
                })
                .unwrap_or_else(|| {
                    self.rewriter.rewrite(x)
                })
        }

    }
}

#[proc_macro_attribute]
pub fn hirpdag_end(
    attr: proc_macro::TokenStream,
    _input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attrs = syn::parse_macro_input!(attr as HirpdagArgs);
    let config = HirpdagConfig::from(&attrs);

    let mut guard = DATA_TYPES.lock().unwrap();

    let rewrite_methods = guard
        .iter()
        .map(|entry| get_rewrite_datatype(&entry.name))
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    let cache_members = guard
        .iter()
        .map(|entry| get_cache_member(&entry.name))
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    let cache_members_new = guard
        .iter()
        .map(|entry| get_cache_member_new(&entry.name))
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    let cache_methods = guard
        .iter()
        .map(|entry| get_cache_rewrite(&entry.name))
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    // (name, is_root) for each hashconsed struct type in the module.
    let struct_types: Vec<(String, bool)> = guard
        .iter()
        .filter(|entry| entry.is_struct)
        .map(|entry| (entry.name.clone(), entry.is_root))
        .collect();
    let serialization_items = get_serialization_items(&struct_types);

    guard.clear();

    let reference_type: proc_macro2::TokenStream = config.reference_type();
    let reference_weak_type: proc_macro2::TokenStream = config.reference_weak_type();

    let table_type: proc_macro2::TokenStream = config.table_type();
    let tableshared_type: proc_macro2::TokenStream = config.tableshared_type();
    let build_tableshared_type: proc_macro2::TokenStream = config.build_tableshared_type();

    let t = quote! {
        type ImplRef<D> = #reference_type;
        type ImplRefWeak<D> = #reference_weak_type;
        type ImplTable<D> = #table_type;
        type ImplTableShared<D> = #tableshared_type;
        type ImplBuildTableShared<D> = #build_tableshared_type;

        pub trait HirpdagRewriter: std::marker::Sized {
            #rewrite_methods

            fn rewrite<T: HirpdagRewritable<Self>>(&self, x: &T) -> T {
                x.hirpdag_rewrite(self)
            }
        }

        pub struct HirpdagRewriteMemoized<Rewriter: HirpdagRewriter> {
            #cache_members
            rewriter: Rewriter,
        }

        impl<Rewriter: HirpdagRewriter> HirpdagRewriteMemoized<Rewriter> {
            pub fn new(rewriter: Rewriter) -> Self {
                Self {
                    #cache_members_new
                    rewriter: rewriter,
                }
            }
        }

        impl<Rewriter: HirpdagRewriter> HirpdagRewriter for HirpdagRewriteMemoized<Rewriter> {
            #cache_methods
        }

        #serialization_items
    };
    t.into()
}

/// Converts a CamelCase type name to a snake_case field name.
/// e.g. "MessageA" -> "message_a".
fn to_snake_case(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Generates the module-level serialization machinery: the archive node enum,
/// the HirpdagArchiveRoots struct (one vector per `#[hirpdag(root)]` type),
/// the collect context, the thread-local (de)serialization sessions, and the
/// public entry points.
///
/// `struct_types` is (name, is_root) for each hashconsed struct type.
///
/// The session/collect infrastructure is generated whenever the module has
/// struct types (the per-struct impls generated by `#[hirpdag]` refer to it).
/// The HirpdagArchiveRoots struct and the entry points are only generated
/// when at least one type is marked `#[hirpdag(root)]`.
fn get_serialization_items(struct_types: &[(String, bool)]) -> proc_macro2::TokenStream {
    if struct_types.is_empty() {
        // No hashconsed types in this module; nothing to serialize.
        return proc_macro2::TokenStream::new();
    }
    let has_roots = struct_types.iter().any(|(_, is_root)| *is_root);

    let mut archive_variants = proc_macro2::TokenStream::new();
    let mut noderef_variants = proc_macro2::TokenStream::new();
    let mut intern_arms = proc_macro2::TokenStream::new();
    let mut roots_field_declarations = proc_macro2::TokenStream::new();
    let mut roots_fields_collect = proc_macro2::TokenStream::new();

    for (name, is_root) in struct_types {
        let ref_name = Ident::new(name, Span::call_site());
        let struct_name = Ident::new(&format!("HirpdagStruct{}", name), Span::call_site());

        archive_variants.extend(quote! {
            #ref_name(#struct_name),
        });
        noderef_variants.extend(quote! {
            #ref_name(#ref_name),
        });
        // Nodes are re-interned through the normal hashcons path (not the
        // normalizing constructor: the archived data was produced from
        // already-normalized nodes). This merges with any nodes already live
        // in the process and restores sharing exactly.
        intern_arms.extend(quote! {
            HirpdagArchiveNode::#ref_name(data) => HirpdagNodeRef::#ref_name(#ref_name(
                hirpdag::base::HirpdagStruct::hirpdag_hashcons(data),
            )),
        });

        if *is_root {
            let field_name = Ident::new(&to_snake_case(name), Span::call_site());
            roots_field_declarations.extend(quote! {
                pub #field_name: Vec<#ref_name>,
            });
            roots_fields_collect.extend(quote! {
                for root in &self.#field_name {
                    hirpdag::base::HirpdagCollect::hirpdag_collect(root, ctx);
                }
            });
        }
    }

    let roots_items = get_serialization_roots_items(
        has_roots,
        roots_field_declarations,
        roots_fields_collect,
        intern_arms,
    );

    quote! {
        // ==== Serialization
        //
        // Archive layout: version, then the node table in post-order DFS
        // order (children before parents), then the roots. Refs are encoded
        // as u64 indices into the node table.

        /// One entry in the serialized node table.
        #[derive(Clone, Debug)]
        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde")]
        #[allow(dead_code)]
        enum HirpdagArchiveNode {
            #archive_variants
        }

        /// A reconstructed node of any hirpdag type in this module.
        /// Internal to the deserialization session: node references resolve
        /// their u64 index against a vector of these.
        #[derive(Clone, Debug)]
        #[allow(dead_code)]
        enum HirpdagNodeRef {
            #noderef_variants
        }

        /// Collect phase state: dedup map from node creation id to node table
        /// index, and the node table itself in post-order DFS order.
        #[doc(hidden)]
        pub struct HirpdagCollectCtx {
            seen: std::collections::HashMap<u64, u64>,
            nodes: Vec<HirpdagArchiveNode>,
        }

        impl HirpdagCollectCtx {
            fn new() -> Self {
                Self {
                    seen: std::collections::HashMap::new(),
                    nodes: Vec::new(),
                }
            }
        }

        // serde's traits carry no user state, so the ref index resolution
        // state lives in thread-local sessions scoped to the entry points
        // below. Sessions are per-thread and not re-entrant.
        std::thread_local! {
            static HIRPDAG_SER_SESSION: std::cell::RefCell<
                Option<std::collections::HashMap<u64, u64>>,
            > = std::cell::RefCell::new(None);
            static HIRPDAG_DE_SESSION: std::cell::RefCell<Option<Vec<HirpdagNodeRef>>> =
                std::cell::RefCell::new(None);
        }

        #roots_items
    }
}

/// Generates the roots-dependent serialization items: the HirpdagArchiveRoots
/// struct, the session guards, the archive container and the public entry
/// points. Empty when no type in the module is marked `#[hirpdag(root)]`.
fn get_serialization_roots_items(
    has_roots: bool,
    roots_field_declarations: proc_macro2::TokenStream,
    roots_fields_collect: proc_macro2::TokenStream,
    intern_arms: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if !has_roots {
        return proc_macro2::TokenStream::new();
    }

    quote! {
        /// The roots of a serialized archive: one vector per
        /// `#[hirpdag(root)]` type. Input of the serialize entry points and
        /// output of the deserialize entry points.
        ///
        /// Implements `Default`, so a subset of root types can be set with
        /// struct update syntax:
        /// `HirpdagArchiveRoots { foo: vec![x], ..Default::default() }`.
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde", default)]
        #[allow(dead_code)]
        pub struct HirpdagArchiveRoots {
            #roots_field_declarations
        }

        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for HirpdagArchiveRoots {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                #roots_fields_collect
            }
        }

        struct HirpdagSerSessionGuard;

        impl HirpdagSerSessionGuard {
            fn open(
                index_map: std::collections::HashMap<u64, u64>,
            ) -> Result<Self, hirpdag::base::HirpdagSerializeError> {
                HIRPDAG_SER_SESSION.with(|cell| {
                    let mut borrow = cell.borrow_mut();
                    if borrow.is_some() {
                        return Err(hirpdag::base::HirpdagSerializeError::SessionActive);
                    }
                    *borrow = Some(index_map);
                    Ok(HirpdagSerSessionGuard)
                })
            }
        }

        impl Drop for HirpdagSerSessionGuard {
            fn drop(&mut self) {
                HIRPDAG_SER_SESSION.with(|cell| *cell.borrow_mut() = None);
            }
        }

        struct HirpdagDeSessionGuard;

        impl HirpdagDeSessionGuard {
            fn open() -> Result<Self, hirpdag::base::HirpdagDeserializeError> {
                HIRPDAG_DE_SESSION.with(|cell| {
                    let mut borrow = cell.borrow_mut();
                    if borrow.is_some() {
                        return Err(hirpdag::base::HirpdagDeserializeError::SessionActive);
                    }
                    *borrow = Some(Vec::new());
                    Ok(HirpdagDeSessionGuard)
                })
            }
        }

        impl Drop for HirpdagDeSessionGuard {
            fn drop(&mut self) {
                HIRPDAG_DE_SESSION.with(|cell| *cell.borrow_mut() = None);
            }
        }

        /// The node table. Serializes as a plain sequence; deserialization
        /// interns each node into the hashcons table as soon as it is
        /// decoded, so later nodes (and the roots) can resolve references to
        /// it in a single forward pass.
        struct HirpdagNodeSeq(Vec<HirpdagArchiveNode>);

        impl hirpdag::serde::Serialize for HirpdagNodeSeq {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: hirpdag::serde::Serializer,
            {
                use hirpdag::serde::ser::SerializeSeq;
                let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
                for node in &self.0 {
                    seq.serialize_element(node)?;
                }
                seq.end()
            }
        }

        impl<'de> hirpdag::serde::Deserialize<'de> for HirpdagNodeSeq {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: hirpdag::serde::Deserializer<'de>,
            {
                struct HirpdagNodeSeqVisitor;

                impl<'de> hirpdag::serde::de::Visitor<'de> for HirpdagNodeSeqVisitor {
                    type Value = HirpdagNodeSeq;

                    fn expecting(
                        &self,
                        formatter: &mut std::fmt::Formatter,
                    ) -> std::fmt::Result {
                        formatter.write_str("a sequence of hirpdag nodes")
                    }

                    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                    where
                        A: hirpdag::serde::de::SeqAccess<'de>,
                    {
                        while let Some(node) =
                            seq.next_element::<HirpdagArchiveNode>()?
                        {
                            let reconstructed = match node {
                                #intern_arms
                            };
                            HIRPDAG_DE_SESSION.with(|cell| -> Result<(), A::Error> {
                                cell.borrow_mut()
                                    .as_mut()
                                    .ok_or_else(|| {
                                        <A::Error as hirpdag::serde::de::Error>::custom(
                                            "hirpdag nodes deserialized outside a hirpdag deserialization session",
                                        )
                                    })?
                                    .push(reconstructed);
                                Ok(())
                            })?;
                        }
                        // The reconstructed refs live in the session; the
                        // archive value itself carries nothing further.
                        Ok(HirpdagNodeSeq(Vec::new()))
                    }
                }

                deserializer.deserialize_seq(HirpdagNodeSeqVisitor)
            }
        }

        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde")]
        struct HirpdagArchive {
            version: hirpdag::base::HirpdagFormatVersion,
            nodes: HirpdagNodeSeq,
            roots: HirpdagArchiveRoots,
        }

        /// Runs the collect phase: post-order DFS from each root, registering
        /// every unique reachable node exactly once, children first.
        fn hirpdag_collect_archive(
            roots: &HirpdagArchiveRoots,
        ) -> (HirpdagArchive, std::collections::HashMap<u64, u64>) {
            let mut ctx = HirpdagCollectCtx::new();
            hirpdag::base::HirpdagCollect::hirpdag_collect(roots, &mut ctx);
            let archive = HirpdagArchive {
                version: hirpdag::base::HirpdagFormatVersion,
                nodes: HirpdagNodeSeq(ctx.nodes),
                roots: roots.clone(),
            };
            (archive, ctx.seen)
        }

        /// Serializes the given roots (and every node reachable from them)
        /// into the hirpdag binary archive format. Each unique node is
        /// written exactly once, preserving DAG sharing.
        #[allow(dead_code)]
        pub fn hirpdag_serialize(
            roots: &HirpdagArchiveRoots,
        ) -> Result<Vec<u8>, hirpdag::base::HirpdagSerializeError> {
            let (archive, index_map) = hirpdag_collect_archive(roots);
            let _session = HirpdagSerSessionGuard::open(index_map)?;
            let payload = hirpdag::postcard::to_stdvec(&archive)
                .map_err(|e| hirpdag::base::HirpdagSerializeError::Format(e.to_string()))?;
            let mut bytes =
                Vec::with_capacity(hirpdag::base::HIRPDAG_MAGIC.len() + payload.len());
            bytes.extend_from_slice(hirpdag::base::HIRPDAG_MAGIC);
            bytes.extend_from_slice(&payload);
            Ok(bytes)
        }

        /// Deserializes a hirpdag binary archive, re-interning every node
        /// through the hashcons table, and returns the typed roots.
        #[allow(dead_code)]
        pub fn hirpdag_deserialize(
            bytes: &[u8],
        ) -> Result<HirpdagArchiveRoots, hirpdag::base::HirpdagDeserializeError> {
            let payload = hirpdag::base::hirpdag_strip_magic(bytes)?;
            let _session = HirpdagDeSessionGuard::open()?;
            let archive: HirpdagArchive = hirpdag::postcard::from_bytes(payload)
                .map_err(|e| hirpdag::base::HirpdagDeserializeError::Format(e.to_string()))?;
            Ok(archive.roots)
        }

        /// JSON (text format) variant of [`hirpdag_serialize`].
        #[allow(dead_code)]
        pub fn hirpdag_serialize_json(
            roots: &HirpdagArchiveRoots,
        ) -> Result<String, hirpdag::base::HirpdagSerializeError> {
            let (archive, index_map) = hirpdag_collect_archive(roots);
            let _session = HirpdagSerSessionGuard::open(index_map)?;
            hirpdag::serde_json::to_string(&archive)
                .map_err(|e| hirpdag::base::HirpdagSerializeError::Format(e.to_string()))
        }

        /// JSON (text format) variant of [`hirpdag_deserialize`].
        #[allow(dead_code)]
        pub fn hirpdag_deserialize_json(
            text: &str,
        ) -> Result<HirpdagArchiveRoots, hirpdag::base::HirpdagDeserializeError> {
            let _session = HirpdagDeSessionGuard::open()?;
            let archive: HirpdagArchive = hirpdag::serde_json::from_str(text)
                .map_err(|e| hirpdag::base::HirpdagDeserializeError::Format(e.to_string()))?;
            Ok(archive.roots)
        }
    }
}
