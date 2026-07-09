#![forbid(unsafe_code)]

#[macro_use]
extern crate quote;
extern crate syn;

extern crate proc_macro;
extern crate proc_macro2;

mod config;

use crate::config::{HirpdagArgs, HirpdagConfig};

use proc_macro2::{Ident, Span};

/// A hirpdag data type seen in the module.
struct DataTypeEntry {
    name: String,
    /// Struct types are hashconsed and appear as entries in the serialized
    /// node table. Enum types are inline payload data within their parent.
    is_struct: bool,
    /// Root types (`#[hirpdag(root)]`) get a vector in the generated
    /// HirpdagArchiveRoots struct used to serialize and deserialize.
    is_root: bool,
    /// Canonical description of the type definition (name, fields/variants
    /// and their types, root marker). The definitions of all types in the
    /// module, in declaration order, are hashed into the schema fingerprint
    /// embedded in binary archives.
    definition: String,
}

/// FNV-1a 64-bit hash. Implemented here (rather than using std's
/// DefaultHasher) because the value is embedded in serialized archives and
/// must be stable across Rust releases and platforms.
fn fnv1a_64(data: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in data.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Generates hirpdag data structures for an inline module.
///
/// Each struct or enum marked `#[hirpdag]` becomes a hash-consed data type,
/// other items pass through unchanged, and the module-level machinery
/// (rewriting, serialization) is appended. Attribute arguments select the
/// hash-consing configuration: a named `preset = "..."` or the explicit
/// `reference_type`, `reference_weak_type`, `table_type`,
/// `tableshared_type`, and `build_tableshared_type` strings.
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
    let attrs = syn::parse_macro_input!(attr as HirpdagArgs);
    let config = HirpdagConfig::from(&attrs);
    let module = syn::parse_macro_input!(input as syn::ItemMod);
    expand_hirpdag_module(&config, &module)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand_hirpdag_module(
    config: &HirpdagConfig,
    module: &syn::ItemMod,
) -> syn::Result<proc_macro2::TokenStream> {
    let (_, items) = module.content.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(
            module,
            "#[hirpdag_module] requires an inline module: `mod name { ... }`",
        )
    })?;
    let body = expand_module_items(config, items)?;
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
fn expand_module_items(
    config: &HirpdagConfig,
    items: &[syn::Item],
) -> syn::Result<proc_macro2::TokenStream> {
    let mut types: Vec<DataTypeEntry> = Vec::new();
    let mut body = proc_macro2::TokenStream::new();
    for item in items {
        let mut item = item.clone();
        if let Some(attr) = take_hirpdag_attr(&mut item) {
            let args = parse_hirpdag_args(&attr)?;
            let type_config = HirpdagConfig::from(&args);
            let input: syn::DeriveInput = match item {
                syn::Item::Struct(s) => s.into(),
                syn::Item::Enum(e) => e.into(),
                _ => unreachable!("take_hirpdag_attr only matches structs and enums"),
            };
            body.extend(match &input.data {
                syn::Data::Struct(s) => expand_hirpdag_struct(&type_config, &input, s, &mut types),
                syn::Data::Enum(e) => expand_hirpdag_enum(&type_config, &input, e, &mut types),
                _ => unreachable!(),
            });
        } else {
            body.extend(quote! { #item });
        }
    }
    body.extend(expand_hirpdag_end(config, &types));
    Ok(body)
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

fn parse_hirpdag_args(attr: &syn::Attribute) -> syn::Result<HirpdagArgs> {
    match &attr.meta {
        syn::Meta::Path(_) => syn::parse2(proc_macro2::TokenStream::new()),
        syn::Meta::List(list) => syn::parse2(list.tokens.clone()),
        syn::Meta::NameValue(nv) => Err(syn::Error::new_spanned(
            nv,
            "unexpected `#[hirpdag = ...]`; use `#[hirpdag]` or `#[hirpdag(...)]`",
        )),
    }
}

/// Canonical description of a struct definition for schema fingerprinting:
/// field names and types (not attributes or doc comments), in order.
fn get_definition_string_struct(
    name: &str,
    is_root: bool,
    fields_named: &syn::FieldsNamed,
) -> String {
    use quote::ToTokens;
    let mut s = String::new();
    if is_root {
        s.push_str("root ");
    }
    s.push_str("struct ");
    s.push_str(name);
    for field in &fields_named.named {
        s.push_str(&format!(
            ";{}:{}",
            field.ident.as_ref().unwrap(),
            field.ty.to_token_stream()
        ));
    }
    s
}

/// Canonical description of an enum definition for schema fingerprinting:
/// variant names and payload types (not attributes or doc comments), in order.
fn get_definition_string_enum(name: &str, input_enum: &syn::DataEnum) -> String {
    use quote::ToTokens;
    let mut s = String::new();
    s.push_str("enum ");
    s.push_str(name);
    for variant in &input_enum.variants {
        s.push_str(&format!(
            ";{}{}",
            variant.ident,
            variant.fields.to_token_stream()
        ));
    }
    s
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

fn get_fields_compute_meta(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_compute_meta = quote! {
    //    self.a.hirpdag_compute_meta(),
    //    self.b.hirpdag_compute_meta(),
    //    self.c.hirpdag_compute_meta(),
    //};
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .map(|field_name| quote! { self.#field_name.hirpdag_compute_meta(), })
        .collect()
}

fn get_fields_rewrite(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_rewrite = quote! {
    //    rewriter.rewrite(&self.a),
    //    rewriter.rewrite(&self.b),
    //    rewriter.rewrite(&self.c),
    //};
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .map(|field_name| quote! { rewriter.rewrite(&self.#field_name), })
        .collect()
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
        .map(|field_name| {
            quote! {
                hirpdag::base::HirpdagCollect::hirpdag_collect(&self.#field_name, ctx);
            }
        })
        .collect()
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

fn get_default_normalizer(
    config: &HirpdagConfig,
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

fn expand_hirpdag_struct(
    config: &HirpdagConfig,
    input: &syn::DeriveInput,
    input_struct: &syn::DataStruct,
    types: &mut Vec<DataTypeEntry>,
) -> proc_macro2::TokenStream {
    let name: &Ident = &input.ident;

    let name_str = name.to_string();
    let name_uppercase_str = name_str.to_ascii_uppercase();

    types.push(DataTypeEntry {
        name: name_str.clone(),
        is_struct: true,
        is_root: config.is_root(),
        definition: get_definition_string_struct(
            &name_str,
            config.is_root(),
            get_fields_named(input_struct),
        ),
    });

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
    let fields_parameters = get_fields_parameters(fields_named);
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
            fn spawn(#fields_parameters) -> Self {
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
            pub fn default_rewrite<T: HirpdagRewriter>(&self, rewriter: &T) -> Self {
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
    input_enum
        .variants
        .iter()
        .map(|t| {
            let variant = &t.ident;
            quote! { #variant(x) => x.hirpdag_compute_meta(), }
        })
        .collect()
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
        .map(|t| {
            let variant = &t.ident;
            quote! {
                #variant(x) => hirpdag::base::HirpdagCollect::hirpdag_collect(x, ctx),
            }
        })
        .collect()
}

fn get_variants_rewrite(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    //let variants_compute_meta = quote! {
    //    Foo(x) => Foo(rewriter.rewrite(&x)),
    //    Bar(x) => Bar(rewriter.rewrite(&x)),
    //    Baz(x) => Baz(rewriter.rewrite(&x)),
    //};
    input_enum
        .variants
        .iter()
        .map(|t| {
            let variant = &t.ident;
            quote! { #variant(x) => #variant(rewriter.rewrite(&x)), }
        })
        .collect()
}

fn expand_hirpdag_enum(
    config: &HirpdagConfig,
    input: &syn::DeriveInput,
    input_enum: &syn::DataEnum,
    types: &mut Vec<DataTypeEntry>,
) -> proc_macro2::TokenStream {
    let name: &Ident = &input.ident;

    let name_str = name.to_string();

    if config.is_root() {
        panic!("`#[hirpdag(root)]` can only be applied to structs; enums are not hashconsed");
    }

    types.push(DataTypeEntry {
        name: name_str.clone(),
        is_struct: false,
        is_root: false,
        definition: get_definition_string_enum(&name_str, input_enum),
    });

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
            pub fn default_rewrite<T: HirpdagRewriter>(&self, rewriter: &T) -> Self {
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
                .cloned()
                .unwrap_or_else(|| {
                    self.rewriter.rewrite(x)
                })
        }

    }
}

/// Generates the module-level code for the given configuration from all of
/// the `#[hirpdag]` types in the module: the Impl* type aliases, the
/// HirpdagRewriter trait, memoized rewriting, and the serialization
/// machinery.
fn expand_hirpdag_end(config: &HirpdagConfig, types: &[DataTypeEntry]) -> proc_macro2::TokenStream {
    let rewrite_methods: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_rewrite_datatype(&entry.name))
        .collect();

    let cache_members: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_cache_member(&entry.name))
        .collect();

    let cache_members_new: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_cache_member_new(&entry.name))
        .collect();

    let cache_methods: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_cache_rewrite(&entry.name))
        .collect();

    // (name, is_root) for each hashconsed struct type in the module.
    let struct_types: Vec<(String, bool)> = types
        .iter()
        .filter(|entry| entry.is_struct)
        .map(|entry| (entry.name.clone(), entry.is_root))
        .collect();

    // Schema fingerprint: a stable hash of every type definition in the
    // module, in declaration order (declaration order is part of the binary
    // wire format), plus a human-readable name for debuggable mismatch
    // errors. Computed at macro expansion time and embedded in the header of
    // binary archives.
    let schema_hash = {
        let definitions: Vec<&str> = types
            .iter()
            .map(|entry| entry.definition.as_str())
            .collect();
        fnv1a_64(&definitions.join("\n"))
    };
    let schema_name = {
        // The package being compiled (cargo sets this for the rustc
        // invocation the macro runs in). Stable proc macros cannot get the
        // source file name, so package name + type names identify the
        // hirpdag_end for debugging purposes.
        let pkg = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
        let type_names: Vec<&str> = types.iter().map(|entry| entry.name.as_str()).collect();
        let mut name = format!("{}:{}", pkg, type_names.join(","));
        const SCHEMA_NAME_MAX: usize = 128;
        const ELLIPSIS: &str = "...";
        if name.len() > SCHEMA_NAME_MAX {
            // Leave room for the ellipsis so the total stays within the
            // limit, and back off to a char boundary (identifiers may be
            // non-ASCII; String::truncate panics mid-character).
            let mut cut = SCHEMA_NAME_MAX - ELLIPSIS.len();
            while !name.is_char_boundary(cut) {
                cut -= 1;
            }
            name.truncate(cut);
            name.push_str(ELLIPSIS);
        }
        name
    };

    let serialization_items = get_serialization_items(&struct_types, schema_hash, &schema_name);

    let reference_type: proc_macro2::TokenStream = config.reference_type();
    let reference_weak_type: proc_macro2::TokenStream = config.reference_weak_type();
    let tableshared_type: proc_macro2::TokenStream = config.tableshared_type();
    let build_tableshared_type: proc_macro2::TokenStream = config.build_tableshared_type();
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
    }
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
fn get_serialization_items(
    struct_types: &[(String, bool)],
    schema_hash: u64,
    schema_name: &str,
) -> proc_macro2::TokenStream {
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
        schema_hash,
        schema_name,
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
    schema_hash: u64,
    schema_name: &str,
) -> proc_macro2::TokenStream {
    if !has_roots {
        return proc_macro2::TokenStream::new();
    }

    quote! {
        /// The schema fingerprint embedded in (and verified against) the
        /// header of binary archives written by this module.
        #[allow(dead_code)]
        fn hirpdag_schema_fingerprint() -> hirpdag::base::HirpdagSchemaFingerprint {
            hirpdag::base::HirpdagSchemaFingerprint {
                hash: #schema_hash,
                name: #schema_name.to_string(),
            }
        }

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
        /// written exactly once, preserving DAG sharing. The header carries a
        /// fingerprint of this module's type definitions.
        #[allow(dead_code)]
        pub fn hirpdag_serialize(
            roots: &HirpdagArchiveRoots,
        ) -> Result<Vec<u8>, hirpdag::base::HirpdagSerializeError> {
            let (archive, index_map) = hirpdag_collect_archive(roots);
            let _session = HirpdagSerSessionGuard::open(index_map)?;
            let payload = hirpdag::postcard::to_stdvec(&archive)
                .map_err(|e| hirpdag::base::HirpdagSerializeError::Format(e.to_string()))?;
            let mut bytes =
                hirpdag::base::hirpdag_write_binary_header(&hirpdag_schema_fingerprint())?;
            bytes.extend_from_slice(&payload);
            Ok(bytes)
        }

        /// Deserializes a hirpdag binary archive, re-interning every node
        /// through the hashcons table, and returns the typed roots. Fails
        /// with `SchemaMismatch` if the archive was written by different
        /// hirpdag type definitions.
        #[allow(dead_code)]
        pub fn hirpdag_deserialize(
            bytes: &[u8],
        ) -> Result<HirpdagArchiveRoots, hirpdag::base::HirpdagDeserializeError> {
            let payload = hirpdag::base::hirpdag_read_binary_header(
                bytes,
                &hirpdag_schema_fingerprint(),
            )?;
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
