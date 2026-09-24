#![forbid(unsafe_code)]

//! Serialization: collecting the nodes reachable from a set of roots, the
//! archived (plain data) form of each data type, and for the module the node
//! table types, the schema fingerprint and the entry points. The traversal
//! and the entry points' bodies are in `hirpdag::base::archive`; this is the
//! per-type and per-module wiring.

use proc_macro2::Ident;

use crate::hashcons::get_variant_type;
use crate::names::DataTypeNames;
use crate::DataTypeEntry;

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

/// Canonical description of a struct definition for schema fingerprinting:
/// field names and types (not attributes or doc comments), in order.
pub fn get_definition_string_struct(
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
pub fn get_definition_string_enum(name: &str, input_enum: &syn::DataEnum) -> String {
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

/// The fields of a struct's archived form: the same names, with each type
/// replaced by its archived form (references become `u64` node indices).
fn get_archive_fields_declarations(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let archive_fields_declarations = quote! {
    //    pub a: HirpdagArchiveOf<i32>,
    //    pub b: HirpdagArchiveOf<Option<MessageA>>,
    //};
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

/// Field initialisers of a struct's archived form.
fn get_fields_to_archive(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_to_archive = quote! {
    //    a: hirpdag_archive_encode(&self.a, index)?,
    //    b: hirpdag_archive_encode(&self.b, index)?,
    //};
    fields_named
        .named
        .iter()
        .map(|t| t.ident.as_ref().unwrap())
        .map(|field_name| {
            quote! { #field_name: hirpdag_archive_encode(&self.#field_name, index)?, }
        })
        .collect()
}

/// Field initialisers rebuilding a struct from its archived form.
fn get_fields_from_archive(fields_named: &syn::FieldsNamed) -> proc_macro2::TokenStream {
    //let fields_from_archive = quote! {
    //    a: hirpdag_archive_decode::<i32>(archived.a, nodes)?,
    //    b: hirpdag_archive_decode::<Option<MessageA>>(archived.b, nodes)?,
    //};
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

/// The variants of an enum's archived form: the same names, with each payload
/// type replaced by its archived form.
fn get_variants_archive_declarations(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    //let variants_archive_declarations = quote! {
    //    Foo(HirpdagArchiveOf<i32>),
    //    Bar(HirpdagArchiveOf<Option<MessageA>>),
    //};
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

/// Match arms encoding each variant into the enum's archived form.
fn get_variants_to_archive(
    input_enum: &syn::DataEnum,
    name: &Ident,
    archive_name: &Ident,
) -> proc_macro2::TokenStream {
    //let variants_to_archive = quote! {
    //    MessageKind::Foo(x) => HirpdagArchiveEnumMessageKind::Foo(
    //        hirpdag_archive_encode(x, index)?),
    //};
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

/// Match arms rebuilding each variant from the enum's archived form.
fn get_variants_from_archive(
    input_enum: &syn::DataEnum,
    name: &Ident,
    archive_name: &Ident,
) -> proc_macro2::TokenStream {
    //let variants_from_archive = quote! {
    //    HirpdagArchiveEnumMessageKind::Foo(x) => MessageKind::Foo(
    //        hirpdag_archive_decode::<i32>(x, nodes)?),
    //};
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

pub fn for_struct(
    names: &DataTypeNames,
    fields_named: &syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        struct_data: hirpdag_struct_name,
        archive_form: hirpdag_archive_struct_name,
        ..
    } = names;
    let name_str = hirpdag_ref_name.to_string();

    let fields_collect = get_fields_collect(fields_named);
    let archive_fields_declarations = get_archive_fields_declarations(fields_named);
    let fields_to_archive = get_fields_to_archive(fields_named);
    let fields_from_archive = get_fields_from_archive(fields_named);

    quote! {
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

        // A ref hands its node to the collect walk, which expands it later
        // (see HirpdagCollectNode below) rather than recursing into it here.
        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for #hirpdag_ref_name {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                ctx.visit(
                    self.0.hirpdag_get_creation_id(),
                    || HirpdagNodeRef::#hirpdag_ref_name(self.clone()),
                );
            }
        }

        impl hirpdag::base::HirpdagCollect<HirpdagCollectCtx> for #hirpdag_struct_name {
            fn hirpdag_collect(&self, ctx: &mut HirpdagCollectCtx) {
                #fields_collect
            }
        }
    }
}

pub fn for_enum(names: &DataTypeNames, input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: name,
        archive_form: hirpdag_archive_enum_name,
        ..
    } = names;

    let variants_collect = get_variants_collect(input_enum);
    let variants_archive_declarations = get_variants_archive_declarations(input_enum);
    let variants_to_archive = get_variants_to_archive(input_enum, name, hirpdag_archive_enum_name);
    let variants_from_archive =
        get_variants_from_archive(input_enum, name, hirpdag_archive_enum_name);

    quote! {
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
    }
}

/// The module-level serialization items, built from every `#[hirpdag]` type
/// in the module; see [`get_serialization_items`].
pub fn for_module(types: &[DataTypeEntry], package: &str) -> proc_macro2::TokenStream {
    // The hashconsed struct types of the module, in declaration order.
    let struct_types: Vec<&DataTypeEntry> = types
        .iter()
        .filter(|entry| entry.names.is_struct())
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

    get_serialization_items(&struct_types, schema_hash, &schema_name)
}

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
    let mut collect_children_arms = proc_macro2::TokenStream::new();
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
        collect_children_arms.extend(quote! {
            HirpdagNodeRef::#ref_name(node) => {
                hirpdag::base::HirpdagCollect::hirpdag_collect(&(**node), ctx)
            }
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

        impl hirpdag::base::HirpdagCollectNode for HirpdagNodeRef {
            fn hirpdag_collect_children(&self, ctx: &mut HirpdagCollectCtx) {
                match self {
                    #collect_children_arms
                }
            }
        }

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
