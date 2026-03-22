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

lazy_static! {
    /// Collects all of the Hirpdag types seen in the module.
    /// Each entry is (type_name, is_enum). is_enum=true for hirpdag enums,
    /// is_enum=false for hirpdag structs (which get a HirpdagStruct{Name} inner type).
    ///
    /// This will be used from the HirpdagEnd to generate code which can operate on all of them.
    static ref DATA_TYPES: std::sync::Mutex<Vec<(String, bool)>> = std::sync::Mutex::new(Vec::new());
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

fn get_fields_dag_collect(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .fold(proc_macro2::TokenStream::new(), |mut s, field_name| {
            s.extend(quote! { self.#field_name.hirpdag_dag_collect(ctx); });
            s
        })
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

fn get_serde_struct_extras(
    hirpdag_ref_name: &Ident,
    hirpdag_struct_name: &Ident,
    _fields_named: &syn::FieldsNamed,
    fields_list: &proc_macro2::TokenStream,
    fields_dag_collect: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if !cfg!(feature = "serde") {
        return quote! {};
    }
    quote! {
        // ==== Serialization (DAG-aware)

        impl hirpdag::serde::Serialize for #hirpdag_ref_name {
            fn serialize<S: hirpdag::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                hirpdag::base::dag_serde::HIRPDAG_DAG_SER_CTX.with(|ctx| {
                    let ctx = ctx.borrow();
                    let id_to_idx = ctx.as_ref().expect(
                        "hirpdag::Serialize called outside of HirpdagDag serialization context; \
                         wrap your root in HirpdagDag::new(root) before serializing"
                    );
                    let id = self.0.hirpdag_get_creation_id();
                    let idx = *id_to_idx.get(&id).expect(
                        "hirpdag node missing from DAG context; this is a bug in hirpdag"
                    );
                    hirpdag::serde::Serialize::serialize(&idx, s)
                })
            }
        }

        impl<'de> hirpdag::serde::Deserialize<'de> for #hirpdag_ref_name {
            fn deserialize<D: hirpdag::serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                hirpdag::base::dag_serde::HIRPDAG_DAG_DESER_CTX.with(|ctx| {
                    let ctx = ctx.borrow();
                    let results = ctx.as_ref().expect(
                        "hirpdag::Deserialize called outside of HirpdagDag deserialization context; \
                         deserialize into HirpdagDag<T> rather than T directly"
                    );
                    let idx = <usize as hirpdag::serde::Deserialize>::deserialize(d)?;
                    results[idx]
                        .downcast_ref::<#hirpdag_ref_name>()
                        .ok_or_else(|| hirpdag::serde::de::Error::custom(
                            concat!("DAG deserialization type mismatch for ", stringify!(#hirpdag_ref_name))
                        ))
                        .map(Clone::clone)
                })
            }
        }

        impl hirpdag::base::dag_serde::HirpdagDagCollect for #hirpdag_ref_name {
            fn hirpdag_dag_collect(
                &self,
                ctx: &mut hirpdag::base::dag_serde::HirpdagDagCollectCtx,
            ) {
                let id = self.0.hirpdag_get_creation_id();
                if ctx.visited.contains(&id) {
                    return;
                }
                // Visit fields first so dependencies are always before dependents.
                #fields_dag_collect
                // Record self.
                let idx = ctx.nodes.len();
                ctx.visited.insert(id);
                ctx.id_to_idx.insert(id, idx);
                ctx.nodes.push(std::sync::Arc::new(self.clone()));
            }
        }

        impl #hirpdag_ref_name {
            /// Reconstruct a `#hirpdag_ref_name` from its inner struct value.
            /// Used by the DAG deserializer generated at `#[hirpdag_end]`.
            pub fn hirpdag_from_hirpdag_struct(inner: #hirpdag_struct_name) -> Self {
                let #hirpdag_struct_name { #fields_list } = inner;
                Self::spawn(#fields_list)
            }
        }
    }
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
        guard.push((name_str.clone(), false)); // false = struct, not enum
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
    let fields_dag_collect = get_fields_dag_collect(fields_named);

    let builder_field_declarations = get_builder_field_declarations(fields_named);
    let builder_setters = get_builder_setters(fields_named);
    let builder_none_fields = get_builder_none_fields(fields_named);
    let builder_from_node_fields = get_builder_from_node_fields(fields_named);
    let builder_build_args = get_builder_build_args(fields_named);

    let default_normalizer = get_default_normalizer(config, fields_named);

    let serde_extras = get_serde_struct_extras(
        &hirpdag_ref_name,
        &hirpdag_struct_name,
        fields_named,
        &fields_list,
        &fields_dag_collect,
    );

    let serde_struct_derives = if cfg!(feature = "serde") {
        quote! { , hirpdag::serde::Serialize, hirpdag::serde::Deserialize }
    } else {
        quote! {}
    };

    let serde_struct_crate_attr = if cfg!(feature = "serde") {
        quote! { #[serde(crate = "hirpdag::serde")] }
    } else {
        quote! {}
    };

    quote! {
        use hirpdag::base::*;

        #[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord #serde_struct_derives)]
        #serde_struct_crate_attr
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

        // ==== Serde (DAG-aware, only present when hirpdag/serde feature is enabled)

        #serde_extras
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

fn get_variants_dag_collect(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    input_enum
        .variants
        .iter()
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            let variant = t.ident.clone();
            s.extend(quote! { #variant(x) => x.hirpdag_dag_collect(ctx), });
            s
        })
}

fn get_serde_enum_extras(name: &Ident, variants_dag_collect: &proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    if !cfg!(feature = "serde") {
        return quote! {};
    }
    quote! {
        // ==== Serialization (DAG-aware) for enum

        impl hirpdag::base::dag_serde::HirpdagDagCollect for #name {
            fn hirpdag_dag_collect(
                &self,
                ctx: &mut hirpdag::base::dag_serde::HirpdagDagCollectCtx,
            ) {
                use #name::*;
                match self {
                    #variants_dag_collect
                }
            }
        }
    }
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
    _config: &HirpdagConfig,
    input: &syn::DeriveInput,
    input_enum: &syn::DataEnum,
) -> proc_macro2::TokenStream {
    let name: &Ident = &input.ident;

    let name_str = name.to_string();

    {
        let mut guard = DATA_TYPES.lock().unwrap();
        guard.push((name_str.clone(), true)); // true = enum
    }

    let hirpdag_rewrite_method_name_str = format!("rewrite_{}", name_str);
    let hirpdag_rewrite_method_name =
        Ident::new(&hirpdag_rewrite_method_name_str, Span::call_site());

    let variants_declarations = get_variants_declarations(input_enum);
    let variants_compute_meta = get_variants_compute_meta(input_enum);
    let variants_rewrite = get_variants_rewrite(input_enum);
    let variants_dag_collect = get_variants_dag_collect(input_enum);

    let serde_enum_extras = get_serde_enum_extras(&name, &variants_dag_collect);

    let serde_enum_derives = if cfg!(feature = "serde") {
        quote! { , hirpdag::serde::Serialize, hirpdag::serde::Deserialize }
    } else {
        quote! {}
    };

    let serde_enum_crate_attr = if cfg!(feature = "serde") {
        quote! { #[serde(crate = "hirpdag::serde")] }
    } else {
        quote! {}
    };

    quote! {
        use hirpdag::base::*;

        #[derive(Hash, Clone, Debug, PartialEq, Eq, PartialOrd, Ord #serde_enum_derives)]
        #serde_enum_crate_attr
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

        #serde_enum_extras
    }
}

fn get_serde_end(entries: &[(String, bool)]) -> proc_macro2::TokenStream {
    if !cfg!(feature = "serde") {
        return quote! {};
    }

    // Only struct types are independently hash-consed and appear in the flat node list.
    // Enum types are embedded as field values and serialized inline (with their own serde impls).
    let struct_names: Vec<&String> = entries
        .iter()
        .filter(|(_, is_enum)| !is_enum)
        .map(|(name, _)| name)
        .collect();

    let node_serde_variants: proc_macro2::TokenStream = struct_names
        .iter()
        .map(|name| {
            let name_ident = Ident::new(name, Span::call_site());
            let struct_ident =
                Ident::new(&format!("HirpdagStruct{}", name), Span::call_site());
            quote! { #name_ident(#struct_ident), }
        })
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    let make_wrapper_arms: proc_macro2::TokenStream = struct_names
        .iter()
        .map(|name| {
            let name_ident = Ident::new(name, Span::call_site());
            quote! {
                if let Some(n) = node.downcast_ref::<#name_ident>() {
                    return HirpdagNodeSerde::#name_ident((**n).clone());
                }
            }
        })
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    let hashcons_arms: proc_macro2::TokenStream = struct_names
        .iter()
        .map(|name| {
            let name_ident = Ident::new(name, Span::call_site());
            quote! {
                HirpdagNodeSerde::#name_ident(inner) =>
                    std::sync::Arc::new(#name_ident::hirpdag_from_hirpdag_struct(inner)),
            }
        })
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    quote! {
        // ==== DAG-aware serde infrastructure (generated by #[hirpdag_end])

        /// Unified enum for serializing and deserializing any hirpdag node in this module.
        ///
        /// When the DAG serialization thread-local context is active, child hirpdag-ref fields
        /// serialize as integer indices and deserialize from integer indices, preserving sharing.
        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde")]
        #[serde(tag = "t", content = "d")]
        pub enum HirpdagNodeSerde {
            #node_serde_variants
        }

        fn hirpdag_make_node_wrapper(
            node: &std::sync::Arc<dyn std::any::Any>,
        ) -> HirpdagNodeSerde {
            #make_wrapper_arms
            panic!("unknown hirpdag node type in DAG serialization; \
                    ensure all node types in this module use #[hirpdag]")
        }

        fn hirpdag_hashcons_node_data(
            data: HirpdagNodeSerde,
        ) -> std::sync::Arc<dyn std::any::Any> {
            match data {
                #hashcons_arms
            }
        }

        /// Wrapper for DAG-aware serde serialization of a hirpdag root node.
        ///
        /// Serializes as a flat list of all reachable nodes in topological order, with
        /// child references encoded as integer indices — so shared sub-DAGs appear once.
        ///
        /// # Example
        /// ```
        /// let json = serde_json::to_string(&HirpdagDag::new(root)).unwrap();
        /// let dag: HirpdagDag<MyType> = serde_json::from_str(&json).unwrap();
        /// ```
        pub struct HirpdagDag<T> {
            pub root: T,
        }

        impl<T> HirpdagDag<T> {
            pub fn new(root: T) -> Self {
                Self { root }
            }
        }

        impl<T> hirpdag::serde::Serialize for HirpdagDag<T>
        where
            T: hirpdag::base::dag_serde::HirpdagDagCollect + 'static,
        {
            fn serialize<S: hirpdag::serde::Serializer>(
                &self,
                s: S,
            ) -> Result<S::Ok, S::Error> {
                use hirpdag::serde::ser::SerializeStruct;

                // Phase 1: collect all reachable nodes in topological order.
                let mut collect_ctx =
                    hirpdag::base::dag_serde::HirpdagDagCollectCtx::new();
                self.root.hirpdag_dag_collect(&mut collect_ctx);
                let root_idx = collect_ctx.nodes.len() - 1;

                // Phase 2: build owned node wrappers (just clones inner struct data).
                let node_wrappers: Vec<HirpdagNodeSerde> = collect_ctx
                    .nodes
                    .iter()
                    .map(hirpdag_make_node_wrapper)
                    .collect();

                // Phase 3: serialize with DAG context active so child refs emit indices.
                hirpdag::base::dag_serde::HIRPDAG_DAG_SER_CTX.with(|ctx| {
                    *ctx.borrow_mut() = Some(collect_ctx.id_to_idx);
                });

                let mut state = s.serialize_struct("HirpdagDag", 2)?;
                let nodes_result = state.serialize_field("nodes", &node_wrappers);

                hirpdag::base::dag_serde::HIRPDAG_DAG_SER_CTX.with(|ctx| {
                    *ctx.borrow_mut() = None;
                });

                nodes_result?;
                state.serialize_field("root", &root_idx)?;
                state.end()
            }
        }

        impl<'de, T> hirpdag::serde::Deserialize<'de> for HirpdagDag<T>
        where
            T: std::any::Any + Clone + 'static,
        {
            fn deserialize<D: hirpdag::serde::Deserializer<'de>>(
                d: D,
            ) -> Result<Self, D::Error> {
                use hirpdag::serde::de::{MapAccess, SeqAccess, Visitor};

                // Seed that processes nodes one-at-a-time, keeping the DAG context
                // up-to-date with already-deserialized nodes before each element.
                struct NodeListSeed<'r> {
                    results: &'r mut Vec<std::sync::Arc<dyn std::any::Any>>,
                }
                impl<'de2, 'r> hirpdag::serde::de::DeserializeSeed<'de2> for NodeListSeed<'r> {
                    type Value = ();
                    fn deserialize<D2: hirpdag::serde::Deserializer<'de2>>(
                        self,
                        d: D2,
                    ) -> Result<(), D2::Error> {
                        struct SeqVisitor<'r2> {
                            results: &'r2 mut Vec<std::sync::Arc<dyn std::any::Any>>,
                        }
                        impl<'de3, 'r2> Visitor<'de3> for SeqVisitor<'r2> {
                            type Value = ();
                            fn expecting(
                                &self,
                                f: &mut std::fmt::Formatter,
                            ) -> std::fmt::Result {
                                write!(f, "a sequence of hirpdag nodes")
                            }
                            fn visit_seq<A: SeqAccess<'de3>>(
                                self,
                                mut seq: A,
                            ) -> Result<(), A::Error> {
                                loop {
                                    // Expose already-deserialized refs so child fields
                                    // can look up their dependencies by index.
                                    hirpdag::base::dag_serde::HIRPDAG_DAG_DESER_CTX
                                        .with(|ctx| {
                                            *ctx.borrow_mut() =
                                                Some(self.results.clone());
                                        });
                                    match seq.next_element::<HirpdagNodeSerde>()? {
                                        None => break,
                                        Some(node) => {
                                            self.results.push(
                                                hirpdag_hashcons_node_data(node),
                                            );
                                        }
                                    }
                                }
                                hirpdag::base::dag_serde::HIRPDAG_DAG_DESER_CTX
                                    .with(|ctx| *ctx.borrow_mut() = None);
                                Ok(())
                            }
                        }
                        d.deserialize_seq(SeqVisitor { results: self.results })
                    }
                }

                struct DagVisitor<T2>(std::marker::PhantomData<T2>);
                impl<'de2, T2: std::any::Any + Clone + 'static> Visitor<'de2>
                    for DagVisitor<T2>
                {
                    type Value = HirpdagDag<T2>;
                    fn expecting(
                        &self,
                        f: &mut std::fmt::Formatter,
                    ) -> std::fmt::Result {
                        write!(f, "a HirpdagDag with 'nodes' and 'root' fields")
                    }
                    fn visit_map<A: MapAccess<'de2>>(
                        self,
                        mut map: A,
                    ) -> Result<HirpdagDag<T2>, A::Error> {
                        let mut results: Vec<std::sync::Arc<dyn std::any::Any>> =
                            Vec::new();
                        let mut root_idx: Option<usize> = None;

                        while let Some(key) =
                            map.next_key::<std::string::String>()?
                        {
                            match key.as_str() {
                                "nodes" => {
                                    map.next_value_seed(NodeListSeed {
                                        results: &mut results,
                                    })?;
                                }
                                "root" => {
                                    root_idx = Some(map.next_value::<usize>()?);
                                }
                                _ => {
                                    let _ = map.next_value::<
                                        hirpdag::serde::de::IgnoredAny,
                                    >()?;
                                }
                            }
                        }

                        let root_idx = root_idx.ok_or_else(|| {
                            hirpdag::serde::de::Error::missing_field("root")
                        })?;
                        let root = results[root_idx]
                            .downcast_ref::<T2>()
                            .ok_or_else(|| {
                                hirpdag::serde::de::Error::custom(
                                    "root type mismatch in HirpdagDag",
                                )
                            })?
                            .clone();
                        Ok(HirpdagDag { root })
                    }
                }

                d.deserialize_map(DagVisitor::<T>(std::marker::PhantomData))
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
        .map(|(name, _)| get_rewrite_datatype(name))
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    let cache_members = guard
        .iter()
        .map(|(name, _)| get_cache_member(name))
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    let cache_members_new = guard
        .iter()
        .map(|(name, _)| get_cache_member_new(name))
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    let cache_methods = guard
        .iter()
        .map(|(name, _)| get_cache_rewrite(name))
        .fold(proc_macro2::TokenStream::new(), |mut s, t| {
            s.extend(t);
            s
        });

    let serde_end = get_serde_end(&guard);

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

        #serde_end
    };
    t.into()
}
