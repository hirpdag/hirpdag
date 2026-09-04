#![forbid(unsafe_code)]

#[macro_use]
extern crate quote;
extern crate syn;

extern crate proc_macro;
extern crate proc_macro2;

mod config;
mod names;

use crate::config::{HirpdagArgs, HirpdagConfig};
use crate::names::{DataTypeKind, DataTypeNames};

use proc_macro2::{Ident, Span};

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
    let attrs = syn::parse_macro_input!(attr as HirpdagArgs);
    let config = HirpdagConfig::from(&attrs);
    let module = syn::parse_macro_input!(input as syn::ItemMod);
    // The one ambient read: cargo sets this for the rustc invocation the macro
    // runs in. Everything below is a function of its arguments.
    let package = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    expand_hirpdag_module(&config, &module, &package)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand_hirpdag_module(
    config: &HirpdagConfig,
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
    config: &HirpdagConfig,
    items: &[syn::Item],
    package: &str,
) -> syn::Result<(proc_macro2::TokenStream, Vec<DataTypeEntry>)> {
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

/// The named fields of a `#[hirpdag]` struct.
///
/// Tuple and unit structs are rejected here rather than panicking: a panic in a
/// proc macro reaches the user as "custom attribute panicked" with no span,
/// while this points at the declaration.
fn get_fields_named<'a>(
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

/// The fields as struct declarations: `a: i32, b: String, c: Option<MessageA>,`.
fn get_fields_declarations(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
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

/// The field names as an argument list: `a, b, c,`.
fn get_fields_list(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .map(|field_name| quote! { #field_name, })
        .collect()
}

/// Each field's meta contribution: `self.a.hirpdag_compute_meta(),`.
fn get_fields_compute_meta(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .map(|field_name| quote! { self.#field_name.hirpdag_compute_meta(), })
        .collect()
}

/// Body of a struct's `default_rewrite`.
///
/// Each field is rewritten through the recursion driver into a local and
/// compared against the original. If every field is unchanged, the input
/// reference is cloned (one reference-count bump on the already-interned node)
/// rather than paying for `Self::new` (normalization + a hash-cons table
/// lookup) to rebuild a structurally identical node.
///
/// Equality is cheap for the common cases: child `HirpdagRef` fields compare by
/// pointer, and leaf fields compare by value.
fn get_default_rewrite_body(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    let field_names: Vec<&syn::Ident> = fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .collect();

    // A struct with no fields has nothing to rewrite; clone the input reference.
    if field_names.is_empty() {
        return quote! { self.clone() };
    }

    // Prefixed locals so a field literally named `driver` or `self` cannot
    // shadow the parameters used to rewrite the remaining fields.
    let locals: Vec<syn::Ident> = field_names
        .iter()
        .map(|field_name| Ident::new(&format!("hirpdag_rw_{}", field_name), Span::call_site()))
        .collect();

    let lets: proc_macro2::TokenStream = field_names
        .iter()
        .zip(locals.iter())
        .map(|(field_name, local)| quote! { let #local = driver.rewrite(&self.#field_name); })
        .collect();

    let unchanged = field_names
        .iter()
        .zip(locals.iter())
        .map(|(field_name, local)| quote! { #local == self.#field_name });
    let unchanged = quote! { #(#unchanged)&&* };

    let new_args: proc_macro2::TokenStream =
        locals.iter().map(|local| quote! { #local, }).collect();

    quote! {
        #lets
        if #unchanged {
            self.clone()
        } else {
            Self::new(#new_args)
        }
    }
}

/// The fields of a struct's archived form (`pub a: HirpdagArchiveOf<i32>,`):
/// the same names, with each type replaced by its archived form (references
/// become `u64` node indices).
fn get_archive_fields_declarations(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    fields_named
        .named
        .iter()
        .map(|field| {
            let field_name = field.ident.as_ref().unwrap();
            let field_type = &field.ty;
            quote! { pub #field_name: HirpdagArchiveOf<#field_type>, }
        })
        .collect()
}

/// Field initialisers of a struct's archived form:
/// `a: hirpdag_archive_encode(&self.a, index)?,`.
fn get_fields_to_archive(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .map(|field_name| {
            quote! { #field_name: hirpdag_archive_encode(&self.#field_name, index)?, }
        })
        .collect()
}

/// Field initialisers rebuilding a struct from its archived form:
/// `a: hirpdag_archive_decode::<i32>(archived.a, nodes)?,`.
fn get_fields_from_archive(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    fields_named
        .named
        .iter()
        .map(|field| {
            let field_name = field.ident.as_ref().unwrap();
            let field_type = &field.ty;
            quote! {
                #field_name: hirpdag_archive_decode::<#field_type>(archived.#field_name, nodes)?,
            }
        })
        .collect()
}

/// Each field's collect call: `HirpdagCollect::hirpdag_collect(&self.a, ctx);`.
fn get_fields_collect(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
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

/// The builder's fields, each optional until set: `a: Option<i32>,`.
fn get_builder_field_declarations(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
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

/// The builder's setters: `pub fn a(mut self, value: i32) -> Self { .. }`.
fn get_builder_setters(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
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

/// The builder's initial field values: `a: None,`.
fn get_builder_none_fields(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    fields_named
        .named
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().unwrap();
            quote! { #name: None, }
        })
        .collect()
}

/// The builder's field values taken from an existing node:
/// `a: Some(node.a.clone()),`.
fn get_builder_from_node_fields(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    fields_named
        .named
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().unwrap();
            quote! { #name: Some(node.#name.clone()), }
        })
        .collect()
}

/// The builder's fields as `new` arguments, each required:
/// `self.a.expect("Builder field 'a' not set"),`.
fn get_builder_build_args(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
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
) -> syn::Result<(proc_macro2::TokenStream, DataTypeEntry)> {
    let name: &Ident = &input.ident;
    let name_str = name.to_string();

    let names = DataTypeNames::new(name, DataTypeKind::Struct);
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        struct_data: hirpdag_struct_name,
        archive_form: hirpdag_archive_struct_name,
        table: hirpdag_table_name,
        rewrite_method: hirpdag_rewrite_method_name,
        builder: hirpdag_builder_name,
        ..
    } = names.clone();

    let fields_named = get_fields_named(input, input_struct)?;

    let entry = DataTypeEntry {
        names,
        is_root: config.is_root(),
        definition: get_definition_string_struct(&name_str, config.is_root(), fields_named),
    };

    let fields_declarations = get_fields_declarations(fields_named);
    let fields_parameters = get_fields_parameters(fields_named);
    let fields_list = get_fields_list(fields_named);
    let fields_compute_meta = get_fields_compute_meta(fields_named);
    let default_rewrite_body = get_default_rewrite_body(fields_named);
    let fields_collect = get_fields_collect(fields_named);
    let archive_fields_declarations = get_archive_fields_declarations(fields_named);
    let fields_to_archive = get_fields_to_archive(fields_named);
    let fields_from_archive = get_fields_from_archive(fields_named);

    let builder_field_declarations = get_builder_field_declarations(fields_named);
    let builder_setters = get_builder_setters(fields_named);
    let builder_none_fields = get_builder_none_fields(fields_named);
    let builder_from_node_fields = get_builder_from_node_fields(fields_named);
    let builder_build_args = get_builder_build_args(fields_named);

    let default_normalizer = get_default_normalizer(config, fields_named);

    let tokens = quote! {
        use hirpdag::base::*;

        #[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
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
            /// O(n) in the size of the DAG. Prefer `cmp` (creation-ID based) for
            /// ordering; use this only when structural order is specifically needed.
            pub fn hirpdag_cmp_deep(&self, other: &Self) -> std::cmp::Ordering {
                self.0.hirpdag_cmp_deep(&other.0)
            }

            // If normalizer is not provided, generate one.
            #default_normalizer

            /// Rewrite every field through `driver` and rebuild this node.
            ///
            /// This is the traversal step a `HirpdagRewriter` rule delegates to
            /// when it has nothing special to do for a node. Recursion goes
            /// through the driver (not through the rule), so a memoizing driver
            /// sees, and can cache, every node in the traversal.
            #[allow(non_snake_case)]
            pub fn default_rewrite<D: HirpdagRewriteDriver>(&self, driver: &D) -> Self {
                #default_rewrite_body
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

        impl<D: HirpdagRewriteDriver> HirpdagRewritable<D> for #hirpdag_ref_name {
            fn hirpdag_rewrite(&self, driver: &D) -> Self {
                driver.#hirpdag_rewrite_method_name(self)
            }
        }

        // ==== Serialization
        //
        // A node's data is archived as the same data with every reference
        // replaced by the u64 index of the node it names; a reference is
        // archived as that index. The rules are in hirpdag::base::archive;
        // this is the per-type wiring.

        /// The archived form of this node's data: the same fields, with
        /// references replaced by node table indices.
        #[doc(hidden)]
        #[derive(Clone, Debug)]
        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde")]
        pub struct #hirpdag_archive_struct_name {
            #archive_fields_declarations
        }

        impl hirpdag::base::HirpdagArchived<[HirpdagNodeRef]> for #hirpdag_struct_name {
            type Archive = #hirpdag_archive_struct_name;

            fn hirpdag_to_archive(
                &self,
                index: &hirpdag::base::HirpdagNodeIndex,
            ) -> Result<Self::Archive, hirpdag::base::HirpdagSerializeError> {
                Ok(#hirpdag_archive_struct_name {
                    #fields_to_archive
                })
            }

            fn hirpdag_from_archive(
                archived: Self::Archive,
                nodes: &[HirpdagNodeRef],
            ) -> Result<Self, hirpdag::base::HirpdagDeserializeError> {
                Ok(Self {
                    #fields_from_archive
                })
            }
        }

        impl hirpdag::base::HirpdagArchived<[HirpdagNodeRef]> for #hirpdag_ref_name {
            type Archive = u64;

            fn hirpdag_to_archive(
                &self,
                index: &hirpdag::base::HirpdagNodeIndex,
            ) -> Result<u64, hirpdag::base::HirpdagSerializeError> {
                index.index_of(self.0.hirpdag_get_creation_id(), #name_str)
            }

            fn hirpdag_from_archive(
                archived: u64,
                nodes: &[HirpdagNodeRef],
            ) -> Result<Self, hirpdag::base::HirpdagDeserializeError> {
                hirpdag::base::archive_resolve_ref::<HirpdagArchiveSchema, Self>(archived, nodes)
            }
        }

        impl hirpdag::base::HirpdagArchiveMember<HirpdagArchiveSchema> for #hirpdag_ref_name {
            const TYPE_NAME: &'static str = #name_str;

            fn hirpdag_archive_member(node: &HirpdagNodeRef) -> Option<&Self> {
                match node {
                    HirpdagNodeRef::#hirpdag_ref_name(node) => Some(node),
                    #[allow(unreachable_patterns)]
                    _ => None,
                }
            }
        }

        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for #hirpdag_ref_name {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                ctx.visit(
                    self.0.hirpdag_get_creation_id(),
                    |ctx| hirpdag::base::HirpdagCollect::hirpdag_collect(&(**self), ctx),
                    || HirpdagNodeRef::#hirpdag_ref_name(self.clone()),
                );
            }
        }

        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for #hirpdag_struct_name {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                #fields_collect
            }
        }
    };

    Ok((tokens, entry))
}

/// The variants as declarations: `Foo(i32), Bar(String),`.
fn get_variants_declarations(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    let variants_declarations = input_enum.variants.clone();
    quote! { #variants_declarations }
}

/// Each variant's meta contribution: `Foo(x) => x.hirpdag_compute_meta(),`.
fn get_variants_compute_meta(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    input_enum
        .variants
        .iter()
        .map(|t| {
            let variant = &t.ident;
            quote! { #variant(x) => x.hirpdag_compute_meta(), }
        })
        .collect()
}

/// Each variant's collect arm:
/// `Foo(x) => HirpdagCollect::hirpdag_collect(x, ctx),`.
fn get_variants_collect(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
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

/// Each variant's rewrite arm: `Foo(x) => Foo(driver.rewrite(&x)),`.
fn get_variants_rewrite(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    input_enum
        .variants
        .iter()
        .map(|t| {
            let variant = &t.ident;
            quote! { #variant(x) => #variant(driver.rewrite(&x)), }
        })
        .collect()
}

/// The payload type of a single-field tuple variant, which is the only shape
/// `#[hirpdag]` enums take.
fn get_variant_type(variant: &syn::Variant) -> &syn::Type {
    match variant.fields.iter().next() {
        Some(field) if variant.fields.len() == 1 => &field.ty,
        _ => panic!(
            "`#[hirpdag]` enum variants must have exactly one unnamed field: `{}`",
            variant.ident
        ),
    }
}

/// The variants of an enum's archived form (`Foo(HirpdagArchiveOf<i32>),`):
/// the same names, with each payload type replaced by its archived form.
fn get_variants_archive_declarations(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    input_enum
        .variants
        .iter()
        .map(|variant| {
            let name = &variant.ident;
            let payload = get_variant_type(variant);
            quote! { #name(HirpdagArchiveOf<#payload>), }
        })
        .collect()
}

/// Match arms encoding each variant into the enum's archived form:
/// `Kind::Foo(x) => ArchiveKind::Foo(hirpdag_archive_encode(x, index)?),`.
fn get_variants_to_archive(
    input_enum: &syn::DataEnum,
    name: &Ident,
    archive_name: &Ident,
) -> proc_macro2::TokenStream {
    input_enum
        .variants
        .iter()
        .map(|variant| {
            let variant = &variant.ident;
            quote! {
                #name::#variant(x) => #archive_name::#variant(
                    hirpdag_archive_encode(x, index)?
                ),
            }
        })
        .collect()
}

/// Match arms rebuilding each variant from the enum's archived form:
/// `ArchiveKind::Foo(x) => Kind::Foo(hirpdag_archive_decode::<i32>(x, nodes)?),`.
fn get_variants_from_archive(
    input_enum: &syn::DataEnum,
    name: &Ident,
    archive_name: &Ident,
) -> proc_macro2::TokenStream {
    input_enum
        .variants
        .iter()
        .map(|variant| {
            let payload = get_variant_type(variant);
            let variant = &variant.ident;
            quote! {
                #archive_name::#variant(x) => #name::#variant(
                    hirpdag_archive_decode::<#payload>(x, nodes)?
                ),
            }
        })
        .collect()
}

fn expand_hirpdag_enum(
    config: &HirpdagConfig,
    input: &syn::DeriveInput,
    input_enum: &syn::DataEnum,
) -> syn::Result<(proc_macro2::TokenStream, DataTypeEntry)> {
    let name: &Ident = &input.ident;

    let name_str = name.to_string();

    if config.is_root() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`#[hirpdag(root)]` can only be applied to structs; enums are not hashconsed",
        ));
    }

    let names = DataTypeNames::new(name, DataTypeKind::Enum);
    let DataTypeNames {
        archive_form: hirpdag_archive_enum_name,
        rewrite_method: hirpdag_rewrite_method_name,
        ..
    } = names.clone();

    let entry = DataTypeEntry {
        names,
        is_root: false,
        definition: get_definition_string_enum(&name_str, input_enum),
    };

    let variants_declarations = get_variants_declarations(input_enum);
    let variants_compute_meta = get_variants_compute_meta(input_enum);
    let variants_rewrite = get_variants_rewrite(input_enum);
    let variants_collect = get_variants_collect(input_enum);
    let variants_archive_declarations = get_variants_archive_declarations(input_enum);
    let variants_to_archive = get_variants_to_archive(input_enum, name, &hirpdag_archive_enum_name);
    let variants_from_archive =
        get_variants_from_archive(input_enum, name, &hirpdag_archive_enum_name);

    let tokens = quote! {
        use hirpdag::base::*;

        #[derive(Hash, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
            /// Rewrite the payload of the active variant through `driver`.
            ///
            /// See the struct `default_rewrite` for why recursion goes through
            /// the driver rather than through the rewriter's rules.
            #[allow(non_snake_case)]
            pub fn default_rewrite<D: HirpdagRewriteDriver>(&self, driver: &D) -> Self {
                use #name::*;
                match self {
                    #variants_rewrite
                }
            }
        }

        impl<D: HirpdagRewriteDriver> HirpdagRewritable<D> for #name {
            fn hirpdag_rewrite(&self, driver: &D) -> Self {
                driver.#hirpdag_rewrite_method_name(self)
            }
        }

        // ==== Serialization
        //
        // Enum data types are not hashconsed; they are inline payload within
        // their parent node. Collect recurses into the active variant, and
        // the archived form is the same variant carrying an archived payload.

        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for #name {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                use #name::*;
                match self {
                    #variants_collect
                }
            }
        }

        /// The archived form of this payload type: the same variants, with
        /// references replaced by node table indices.
        #[doc(hidden)]
        #[derive(Clone, Debug)]
        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde")]
        pub enum #hirpdag_archive_enum_name {
            #variants_archive_declarations
        }

        impl hirpdag::base::HirpdagArchived<[HirpdagNodeRef]> for #name {
            type Archive = #hirpdag_archive_enum_name;

            fn hirpdag_to_archive(
                &self,
                index: &hirpdag::base::HirpdagNodeIndex,
            ) -> Result<Self::Archive, hirpdag::base::HirpdagSerializeError> {
                Ok(match self {
                    #variants_to_archive
                })
            }

            fn hirpdag_from_archive(
                archived: Self::Archive,
                nodes: &[HirpdagNodeRef],
            ) -> Result<Self, hirpdag::base::HirpdagDeserializeError> {
                Ok(match archived {
                    #variants_from_archive
                })
            }
        }
    };

    Ok((tokens, entry))
}

/// One method of the user-facing `HirpdagRewriter` trait: the rewrite rule for
/// a single data type.
///
/// The rule is handed the node and the recursion driver. The default
/// implementation passes both to `default_rewrite`, which recurses into the
/// node's children through the driver.
fn get_rewrite_datatype(names: &DataTypeNames) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        rewrite_method: hirpdag_rewrite_method_name,
        ..
    } = names;

    quote! {

        #[allow(non_snake_case)]
        fn #hirpdag_rewrite_method_name<D: HirpdagRewriteDriver>(
            &self,
            x: &#hirpdag_ref_name,
            driver: &D,
        ) -> #hirpdag_ref_name {
            #hirpdag_ref_name::default_rewrite(x, driver)
        }

    }
}

/// One method of the `HirpdagRewriteDriver` trait: rewrite a node of a single
/// data type (`fn rewrite_MessageA(&self, x: &MessageA) -> MessageA;`). Drivers
/// implement the traversal strategy (plain recursion, memoized recursion, ...)
/// and are the only path recursion takes.
fn get_driver_datatype(names: &DataTypeNames) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        rewrite_method: hirpdag_rewrite_method_name,
        ..
    } = names;

    quote! {

        #[allow(non_snake_case)]
        fn #hirpdag_rewrite_method_name(&self, x: &#hirpdag_ref_name) -> #hirpdag_ref_name;

    }
}

/// The `HirpdagRewriteDirect` implementation of one driver method: run the
/// rule, handing it this same driver so the recursion stays direct.
fn get_direct_rewrite(names: &DataTypeNames) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        rewrite_method: hirpdag_rewrite_method_name,
        ..
    } = names;

    quote! {

        #[allow(non_snake_case)]
        fn #hirpdag_rewrite_method_name(&self, x: &#hirpdag_ref_name) -> #hirpdag_ref_name {
            self.rewriter.#hirpdag_rewrite_method_name(x, self)
        }

    }
}

/// One data type's field in the memoize cache:
/// `cache_MessageA: HirpdagMemoizeMap<MessageA, MessageA>,`.
fn get_cache_member(names: &DataTypeNames) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        cache_member: hirpdag_cache_member_name,
        ..
    } = names;

    quote! {
        #hirpdag_cache_member_name:
            hirpdag::base::HirpdagMemoizeMap<#hirpdag_ref_name, #hirpdag_ref_name>,
    }
}

/// The initialiser for one cache field: `cache_MessageA: HirpdagMemoizeMap::new(),`.
fn get_cache_member_new(names: &DataTypeNames) -> proc_macro2::TokenStream {
    let hirpdag_cache_member_name = &names.cache_member;

    quote! {
        #hirpdag_cache_member_name: hirpdag::base::HirpdagMemoizeMap::new(),
    }
}

/// The clear call for one cache field: `self.cache_MessageA.clear();`.
fn get_cache_clear(names: &DataTypeNames) -> proc_macro2::TokenStream {
    let hirpdag_cache_member_name = &names.cache_member;

    quote! {
        self.#hirpdag_cache_member_name.clear();
    }
}

/// The `HirpdagMemoize` impl that points the cache's per-type API at one type's
/// table, so `cache.get_or_else(&node, || ..)` resolves to the right map.
fn get_cache_memoize_impl(names: &DataTypeNames) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        cache_member: hirpdag_cache_member_name,
        ..
    } = names;

    quote! {

        impl hirpdag::base::HirpdagMemoize<#hirpdag_ref_name> for HirpdagMemoizeCache {
            fn hirpdag_memoize_map(
                &self,
            ) -> &hirpdag::base::HirpdagMemoizeMap<#hirpdag_ref_name, #hirpdag_ref_name> {
                &self.#hirpdag_cache_member_name
            }
        }

    }
}

/// The `HirpdagRewriteMemoized` implementation of one driver method: the cache
/// serves the node, or runs the rule once and remembers the result.
fn get_cache_rewrite(names: &DataTypeNames) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        rewrite_method: hirpdag_rewrite_method_name,
        ..
    } = names;

    quote! {

        #[allow(non_snake_case)]
        fn #hirpdag_rewrite_method_name(&self, x: &#hirpdag_ref_name) -> #hirpdag_ref_name {
            self.memoize_cache.get_or_else(x, || {
                self.rewriter.#hirpdag_rewrite_method_name(x, self)
            })
        }

    }
}

/// Generates the module-level code for the given configuration from all of
/// the `#[hirpdag]` types in the module: the Impl* type aliases, the
/// HirpdagRewriter trait, memoized rewriting, and the serialization
/// machinery.
fn expand_hirpdag_end(
    config: &HirpdagConfig,
    types: &[DataTypeEntry],
    package: &str,
) -> proc_macro2::TokenStream {
    let rewrite_methods: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_rewrite_datatype(&entry.names))
        .collect();

    let driver_methods: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_driver_datatype(&entry.names))
        .collect();

    let direct_methods: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_direct_rewrite(&entry.names))
        .collect();

    let cache_members: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_cache_member(&entry.names))
        .collect();

    let cache_members_new: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_cache_member_new(&entry.names))
        .collect();

    let cache_clears: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_cache_clear(&entry.names))
        .collect();

    let cache_memoize_impls: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_cache_memoize_impl(&entry.names))
        .collect();

    let cache_methods: proc_macro2::TokenStream = types
        .iter()
        .map(|entry| get_cache_rewrite(&entry.names))
        .collect();

    // The hashconsed struct types of the module, in declaration order.
    let struct_types: Vec<&DataTypeEntry> = types
        .iter()
        .filter(|entry| entry.names.is_struct())
        .collect();

    // A call to reset each struct type's global interning table. Emitted into
    // the module-level `hirpdag_reset_tables()` below (gated on the downstream
    // crate's `reset-tables` feature).
    let reset_table_calls: proc_macro2::TokenStream = struct_types
        .iter()
        .map(|entry| {
            let table_ident = &entry.names.table;
            quote! { #table_ident.reset(); }
        })
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
        // Stable proc macros cannot get the source file name, so the package
        // being compiled plus the type names identify the schema for debugging
        // purposes. The package name is passed in (read from the environment at
        // the attribute) so that expansion stays a function of its arguments.
        let type_names: Vec<String> = types
            .iter()
            .map(|entry| entry.names.ref_name.to_string())
            .collect();
        let mut name = format!("{}:{}", package, type_names.join(","));
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
    // The config's helper aliases (e.g. `ImplTable`), as `type <name><D> = <rhs>;`.
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

        /// The rewrite rules: one method per data type in this module.
        ///
        /// Implement the methods for the types to transform; the rest default to
        /// rewriting their children and rebuilding. Every rule is handed the
        /// recursion `driver` alongside the node. Pass it to `default_rewrite`
        /// (or call `driver.rewrite(..)` directly) to continue into the node's
        /// children. Recursing through the driver rather than through `self` is
        /// what lets a driver such as `HirpdagRewriteMemoized` observe, and
        /// short-circuit, the whole traversal.
        pub trait HirpdagRewriter: std::marker::Sized {
            #rewrite_methods

            /// Rewrite `x` with these rules, recursing without a cache.
            ///
            /// Shorthand for `HirpdagRewriteDirect::new(self).rewrite(&x)`. On a
            /// DAG with shared subtrees prefer
            /// `HirpdagRewriteMemoized::new(rules).rewrite(&x)`, which runs each
            /// rule once per unique node instead of once per path to it.
            fn rewrite<'hirpdag_r, T>(&'hirpdag_r self, x: &T) -> T
            where
                T: HirpdagRewritable<HirpdagRewriteDirect<'hirpdag_r, Self>>,
            {
                HirpdagRewriteDirect::new(self).rewrite(x)
            }
        }

        /// Drives a rewrite traversal: maps a node to its rewritten form.
        ///
        /// The driver decides *how* the traversal runs, the `HirpdagRewriter`
        /// rules decide *what* each node becomes. Two drivers are generated for
        /// every module: `HirpdagRewriteDirect` (recurse on every path) and
        /// `HirpdagRewriteMemoized` (recurse once per unique node). Because the
        /// rules recurse through the driver they are given, the same rules can
        /// be run under either one.
        pub trait HirpdagRewriteDriver: std::marker::Sized {
            #driver_methods

            /// Rewrite any rewritable value: a node, or a container of nodes
            /// such as `Option<Node>` or `Vec<Node>`.
            fn rewrite<T: HirpdagRewritable<Self>>(&self, x: &T) -> T {
                x.hirpdag_rewrite(self)
            }
        }

        /// Driver that applies the rules directly, with no cache: a node reached
        /// by several paths is rewritten once per path.
        pub struct HirpdagRewriteDirect<'hirpdag_r, Rewriter: HirpdagRewriter> {
            rewriter: &'hirpdag_r Rewriter,
        }

        impl<'hirpdag_r, Rewriter: HirpdagRewriter> HirpdagRewriteDirect<'hirpdag_r, Rewriter> {
            pub fn new(rewriter: &'hirpdag_r Rewriter) -> Self {
                Self { rewriter: rewriter }
            }

            /// The rules this driver runs.
            pub fn rewriter(&self) -> &Rewriter {
                self.rewriter
            }
        }

        impl<'hirpdag_r, Rewriter: HirpdagRewriter> HirpdagRewriteDriver
            for HirpdagRewriteDirect<'hirpdag_r, Rewriter>
        {
            #direct_methods
        }

        // Re-exported so that glob-importing this module is enough to call the
        // cache's methods (`cache.get_or_else(..)`) and to build node-keyed
        // tables of one's own.
        pub use hirpdag::base::{HirpdagMemoize, HirpdagMemoizeMap};

        /// Memoization tables for this module: one
        /// `hirpdag::base::HirpdagMemoizeMap` per data type, keyed by node.
        ///
        /// `HirpdagRewriteMemoized` remembers rewritten nodes here, but the cache
        /// is an ordinary value with no dependency on rewriting: build one and
        /// use it for any node-keyed computation worth doing once, calling
        /// `cache.get_or_else(&node, || expensive(node))` through the
        /// `hirpdag::base::HirpdagMemoize` implementation for each type.
        ///
        /// Filling a table takes `&self` and is thread-safe (sharded locks), so a
        /// single cache can be shared by every thread working on the same graph,
        /// and work one thread has done is not repeated by the others.
        #[allow(non_snake_case)]
        pub struct HirpdagMemoizeCache {
            #cache_members
        }

        impl HirpdagMemoizeCache {
            pub fn new() -> Self {
                Self {
                    #cache_members_new
                }
            }

            /// Forget everything the cache has remembered, for every type.
            pub fn clear(&self) {
                #cache_clears
            }
        }

        impl Default for HirpdagMemoizeCache {
            fn default() -> Self {
                Self::new()
            }
        }

        #cache_memoize_impls

        /// Driver that remembers the result of rewriting each node, in a
        /// `HirpdagMemoizeCache`.
        ///
        /// Nodes are hash-consed, so a shared subtree is literally the same node
        /// on every path that reaches it and one cache lookup (`O(1)`: the key
        /// hashes and compares by interned identity) replaces re-traversing it.
        /// On a DAG this collapses a traversal that is exponential in the sharing
        /// into one rule invocation per unique node, and a later rewrite of an
        /// already-seen node is served from the cache.
        ///
        /// Rewriting takes `&self` and the cache is thread-safe, so one memoizer
        /// can rewrite on several threads at once, sharing everything it has
        /// already computed. Results are cached under the assumption that the
        /// rules are a pure function of the node (the usual case: whatever state
        /// the rules read is fixed when the rewriter is constructed);
        /// `clear_caches` discards them, which also releases the nodes they keep
        /// alive.
        pub struct HirpdagRewriteMemoized<Rewriter: HirpdagRewriter> {
            memoize_cache: HirpdagMemoizeCache,
            rewriter: Rewriter,
        }

        impl<Rewriter: HirpdagRewriter> HirpdagRewriteMemoized<Rewriter> {
            pub fn new(rewriter: Rewriter) -> Self {
                Self::with_cache(rewriter, HirpdagMemoizeCache::new())
            }

            /// Run `rewriter` against an existing cache, reusing (and adding to)
            /// what it already holds.
            pub fn with_cache(rewriter: Rewriter, memoize_cache: HirpdagMemoizeCache) -> Self {
                Self {
                    memoize_cache: memoize_cache,
                    rewriter: rewriter,
                }
            }

            /// The rules this driver runs.
            pub fn rewriter(&self) -> &Rewriter {
                &self.rewriter
            }

            /// The rewritten nodes this driver has remembered so far.
            pub fn memoize_cache(&self) -> &HirpdagMemoizeCache {
                &self.memoize_cache
            }

            /// Forget every memoized result.
            pub fn clear_caches(&self) {
                self.memoize_cache.clear();
            }
        }

        impl<Rewriter: HirpdagRewriter> HirpdagRewriteDriver for HirpdagRewriteMemoized<Rewriter> {
            #cache_methods
        }

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

        #serialization_items
    }
}

/// Converts a CamelCase type name to a snake_case field name.
/// e.g. "MessageA" -> "message_a".
/// Generates the module-level serialization items: the interned-node enum, its
/// archived counterpart (the node table entry), the two helpers that pin
/// archiving to this module's node table, and the `HirpdagArchive` impl that
/// hands all of them to `hirpdag::base::archive`, which holds the traversal
/// and the entry points.
///
/// `struct_types` is the hashconsed struct types of the module, in
/// declaration order.
///
/// The schema is generated whenever the module has struct types (the
/// per-struct impls generated by `#[hirpdag]` refer to it). The
/// HirpdagArchiveRoots struct and the entry points are only generated when at
/// least one type is marked `#[hirpdag(root)]`; a module with no root types
/// archives `HirpdagNoRoots` and gets no entry points.
fn get_serialization_items(
    struct_types: &[&DataTypeEntry],
    schema_hash: u64,
    schema_name: &str,
) -> proc_macro2::TokenStream {
    if struct_types.is_empty() {
        // No hashconsed types in this module; nothing to serialize.
        return proc_macro2::TokenStream::new();
    }
    let has_roots = struct_types.iter().any(|entry| entry.is_root);

    let mut archive_variants = proc_macro2::TokenStream::new();
    let mut noderef_variants = proc_macro2::TokenStream::new();
    let mut to_archive_arms = proc_macro2::TokenStream::new();
    let mut from_archive_arms = proc_macro2::TokenStream::new();
    let mut roots_field_declarations = proc_macro2::TokenStream::new();
    let mut roots_fields_collect = proc_macro2::TokenStream::new();
    let mut roots_archive_field_declarations = proc_macro2::TokenStream::new();
    let mut roots_fields_to_archive = proc_macro2::TokenStream::new();
    let mut roots_fields_from_archive = proc_macro2::TokenStream::new();

    for entry in struct_types {
        let DataTypeNames {
            ref_name,
            struct_data: struct_name,
            archive_form: archive_struct_name,
            roots_field: field_name,
            ..
        } = &entry.names;

        archive_variants.extend(quote! {
            #ref_name(#archive_struct_name),
        });
        noderef_variants.extend(quote! {
            #ref_name(#ref_name),
        });
        to_archive_arms.extend(quote! {
            HirpdagNodeRef::#ref_name(node) => HirpdagArchiveNode::#ref_name(
                hirpdag_archive_encode(&(**node), index)?
            ),
        });
        // Nodes are re-interned through the normal hashcons path (not the
        // normalizing constructor: the archived data was produced from
        // already-normalized nodes). This merges with any nodes already live
        // in the process and restores sharing exactly.
        from_archive_arms.extend(quote! {
            HirpdagArchiveNode::#ref_name(data) => HirpdagNodeRef::#ref_name(#ref_name(
                hirpdag::base::HirpdagStruct::hirpdag_hashcons(
                    hirpdag_archive_decode::<#struct_name>(data, nodes)?
                ),
            )),
        });

        if entry.is_root {
            roots_field_declarations.extend(quote! {
                pub #field_name: Vec<#ref_name>,
            });
            roots_fields_collect.extend(quote! {
                for root in &self.#field_name {
                    hirpdag::base::HirpdagCollect::hirpdag_collect(root, ctx);
                }
            });
            roots_archive_field_declarations.extend(quote! {
                pub #field_name: HirpdagArchiveOf<Vec<#ref_name>>,
            });
            roots_fields_to_archive.extend(quote! {
                #field_name: hirpdag_archive_encode(&self.#field_name, index)?,
            });
            roots_fields_from_archive.extend(quote! {
                #field_name: hirpdag_archive_decode::<Vec<#ref_name>>(
                    archived.#field_name, nodes
                )?,
            });
        }
    }

    let roots_type = if has_roots {
        quote! { HirpdagArchiveRoots }
    } else {
        quote! { hirpdag::base::HirpdagNoRoots }
    };

    let roots_items = get_serialization_roots_items(
        has_roots,
        RootsItems {
            field_declarations: roots_field_declarations,
            fields_collect: roots_fields_collect,
            archive_field_declarations: roots_archive_field_declarations,
            fields_to_archive: roots_fields_to_archive,
            fields_from_archive: roots_fields_from_archive,
        },
    );

    quote! {
        // ==== Serialization
        //
        // Archive layout: version, then the node table in post-order DFS
        // order (children before parents), then the roots. Refs are encoded
        // as u64 indices into the node table. The machinery is in
        // hirpdag::base::archive; what follows is this module's schema.

        /// A node of any hirpdag type in this module, interned. The collect
        /// phase builds the node table out of these, and a node reference
        /// resolves its u64 index against them.
        #[doc(hidden)]
        #[derive(Clone, Debug)]
        #[allow(dead_code)]
        pub enum HirpdagNodeRef {
            #noderef_variants
        }

        /// One entry in the serialized node table: a node's data with every
        /// reference replaced by the u64 index of the node it names.
        #[doc(hidden)]
        #[derive(Clone, Debug)]
        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde")]
        #[allow(dead_code)]
        pub enum HirpdagArchiveNode {
            #archive_variants
        }

        /// Collect phase state for this module's node table.
        #[doc(hidden)]
        pub type HirpdagCollectCtx = hirpdag::base::HirpdagCollectCtx<HirpdagNodeRef>;

        /// The archived form of a value in this module: the same value with
        /// every reference replaced by a node table index.
        #[doc(hidden)]
        pub type HirpdagArchiveOf<T> =
            <T as hirpdag::base::HirpdagArchived<[HirpdagNodeRef]>>::Archive;

        // `String`, `Vec` and the other leaf and container types are archived
        // the same way whatever the module, so a call has to say which
        // module's node table it resolves against. These two say it once, and
        // every generated encode/decode goes through them.

        /// Encode a value into its archived form.
        #[doc(hidden)]
        #[allow(dead_code)]
        fn hirpdag_archive_encode<T: hirpdag::base::HirpdagArchived<[HirpdagNodeRef]>>(
            value: &T,
            index: &hirpdag::base::HirpdagNodeIndex,
        ) -> Result<HirpdagArchiveOf<T>, hirpdag::base::HirpdagSerializeError> {
            value.hirpdag_to_archive(index)
        }

        /// Rebuild a value from its archived form, resolving node indices
        /// against the nodes reconstructed so far.
        #[doc(hidden)]
        #[allow(dead_code)]
        fn hirpdag_archive_decode<T: hirpdag::base::HirpdagArchived<[HirpdagNodeRef]>>(
            archived: HirpdagArchiveOf<T>,
            nodes: &[HirpdagNodeRef],
        ) -> Result<T, hirpdag::base::HirpdagDeserializeError> {
            T::hirpdag_from_archive(archived, nodes)
        }

        impl hirpdag::base::HirpdagArchived<[HirpdagNodeRef]> for HirpdagNodeRef {
            type Archive = HirpdagArchiveNode;

            fn hirpdag_to_archive(
                &self,
                index: &hirpdag::base::HirpdagNodeIndex,
            ) -> Result<Self::Archive, hirpdag::base::HirpdagSerializeError> {
                Ok(match self {
                    #to_archive_arms
                })
            }

            fn hirpdag_from_archive(
                archived: Self::Archive,
                nodes: &[HirpdagNodeRef],
            ) -> Result<Self, hirpdag::base::HirpdagDeserializeError> {
                Ok(match archived {
                    #from_archive_arms
                })
            }
        }

        /// This module's archive schema: the type that
        /// `hirpdag::base::archive` is parameterised by.
        #[doc(hidden)]
        pub struct HirpdagArchiveSchema;

        impl hirpdag::base::HirpdagArchive for HirpdagArchiveSchema {
            type Node = HirpdagNodeRef;
            type Roots = #roots_type;

            /// The fingerprint of this module's type definitions, embedded in
            /// (and verified against) the header of binary archives.
            fn schema_fingerprint() -> hirpdag::base::HirpdagSchemaFingerprint {
                hirpdag::base::HirpdagSchemaFingerprint {
                    hash: #schema_hash,
                    name: #schema_name.to_string(),
                }
            }
        }

        #roots_items
    }
}

/// The per-root-type pieces of the roots items, one entry per
/// `#[hirpdag(root)]` type in each.
struct RootsItems {
    /// `pub roots_Foo: Vec<Foo>,`
    field_declarations: proc_macro2::TokenStream,
    /// `for root in &self.roots_Foo { ... }`
    fields_collect: proc_macro2::TokenStream,
    /// `pub roots_Foo: HirpdagArchiveOf<Vec<Foo>>,`
    archive_field_declarations: proc_macro2::TokenStream,
    /// `roots_Foo: hirpdag_archive_encode(&self.roots_Foo, index)?,`
    fields_to_archive: proc_macro2::TokenStream,
    /// `roots_Foo: hirpdag_archive_decode::<Vec<Foo>>(archived.roots_Foo, nodes)?,`
    fields_from_archive: proc_macro2::TokenStream,
}

/// Generates the roots-dependent serialization items: the HirpdagArchiveRoots
/// struct, its archived form, and the four entry points, each a call into
/// `hirpdag::base::archive`. Empty when no type in the module is marked
/// `#[hirpdag(root)]`.
fn get_serialization_roots_items(has_roots: bool, roots: RootsItems) -> proc_macro2::TokenStream {
    if !has_roots {
        return proc_macro2::TokenStream::new();
    }

    let RootsItems {
        field_declarations: roots_field_declarations,
        fields_collect: roots_fields_collect,
        archive_field_declarations: roots_archive_field_declarations,
        fields_to_archive: roots_fields_to_archive,
        fields_from_archive: roots_fields_from_archive,
    } = roots;

    quote! {
        /// The roots of a serialized archive: one vector per
        /// `#[hirpdag(root)]` type, named `roots_` plus the type's name.
        /// Input of the serialize entry points and output of the deserialize
        /// entry points.
        ///
        /// Implements `Default`, so a subset of root types can be set with
        /// struct update syntax:
        /// `HirpdagArchiveRoots { roots_Foo: vec![x], ..Default::default() }`.
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        #[allow(dead_code, non_snake_case)]
        pub struct HirpdagArchiveRoots {
            #roots_field_declarations
        }

        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for HirpdagArchiveRoots {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                #roots_fields_collect
            }
        }

        /// The archived form of the roots: one vector of node table indices
        /// per `#[hirpdag(root)]` type.
        ///
        /// `#[serde(default)]`, so a root type whose vector is empty can be
        /// left out of hand-written JSON, and `deny_unknown_fields`, so a name
        /// that is *not* a root of this module is an error rather than a
        /// silently empty vector. The two compose: `default` governs what may
        /// be omitted, `deny_unknown_fields` what may be added.
        #[doc(hidden)]
        #[derive(Clone, Debug, Default)]
        #[derive(hirpdag::serde::Serialize, hirpdag::serde::Deserialize)]
        #[serde(crate = "hirpdag::serde", default, deny_unknown_fields)]
        #[allow(dead_code, non_snake_case)]
        pub struct HirpdagArchiveRootIndices {
            #roots_archive_field_declarations
        }

        impl hirpdag::base::HirpdagArchived<[HirpdagNodeRef]> for HirpdagArchiveRoots {
            type Archive = HirpdagArchiveRootIndices;

            fn hirpdag_to_archive(
                &self,
                index: &hirpdag::base::HirpdagNodeIndex,
            ) -> Result<Self::Archive, hirpdag::base::HirpdagSerializeError> {
                Ok(HirpdagArchiveRootIndices {
                    #roots_fields_to_archive
                })
            }

            fn hirpdag_from_archive(
                archived: Self::Archive,
                nodes: &[HirpdagNodeRef],
            ) -> Result<Self, hirpdag::base::HirpdagDeserializeError> {
                Ok(Self {
                    #roots_fields_from_archive
                })
            }
        }

        /// Serializes the given roots (and every node reachable from them)
        /// into the hirpdag binary archive format. Each unique node is
        /// written exactly once, preserving DAG sharing. The header carries a
        /// fingerprint of this module's type definitions.
        #[allow(dead_code)]
        pub fn hirpdag_serialize(
            roots: &HirpdagArchiveRoots,
        ) -> Result<Vec<u8>, hirpdag::base::HirpdagSerializeError> {
            hirpdag::base::archive_serialize::<HirpdagArchiveSchema>(roots)
        }

        /// Deserializes a hirpdag binary archive, re-interning every node
        /// through the hashcons table, and returns the typed roots. Fails
        /// with `SchemaMismatch` if the archive was written by different
        /// hirpdag type definitions.
        #[allow(dead_code)]
        pub fn hirpdag_deserialize(
            bytes: &[u8],
        ) -> Result<HirpdagArchiveRoots, hirpdag::base::HirpdagDeserializeError> {
            hirpdag::base::archive_deserialize::<HirpdagArchiveSchema>(bytes)
        }

        /// JSON (text format) variant of [`hirpdag_serialize`].
        #[allow(dead_code)]
        pub fn hirpdag_serialize_json(
            roots: &HirpdagArchiveRoots,
        ) -> Result<String, hirpdag::base::HirpdagSerializeError> {
            hirpdag::base::archive_serialize_json::<HirpdagArchiveSchema>(roots)
        }

        /// JSON (text format) variant of [`hirpdag_deserialize`].
        #[allow(dead_code)]
        pub fn hirpdag_deserialize_json(
            text: &str,
        ) -> Result<HirpdagArchiveRoots, hirpdag::base::HirpdagDeserializeError> {
            hirpdag::base::archive_deserialize_json::<HirpdagArchiveSchema>(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(args: &str) -> HirpdagConfig {
        HirpdagConfig::from(&syn::parse_str::<HirpdagArgs>(args).expect("attribute args parse"))
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
