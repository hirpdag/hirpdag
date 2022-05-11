#![forbid(unsafe_code)]

#[macro_use]
extern crate quote;
extern crate syn;

extern crate proc_macro;
extern crate proc_macro2;

use proc_macro2::{Ident, Span};

#[macro_use]
extern crate lazy_static;

lazy_static! {
    /// Collects all of the Hirpdag struct types seen in the module.
    ///
    /// This will be used from the HirpdagEnd to generate code which can operate on all of them.
    static ref DATA_TYPES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
}

enum HirpdagArg {
    /// Normalizer will be defined by user for construction.
    Normalizer,

    /// Hashconsing strong reference type specified by user.
    ReferenceType(String),

    /// Hashconsing weak reference type specified by user.
    ReferenceWeakType(String),

    /// Hashconsing table type specified by user.
    /// The table must be compatible with the reference type used.
    TableType(String),

    /// Hashconsing table sharing type specified by user.
    TableSharedType(String),

    /// Builder for Hashconsing table sharing type specified by user.
    BuildTableSharedType(String),
}

impl syn::parse::Parse for HirpdagArg {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let key_ident: Ident = input.parse()?;
        let opeq: Option<syn::Token![=]> = input.parse()?;
        let value_lit: Option<syn::Lit> = input.parse()?;
        let has_value = opeq.is_some() && value_lit.is_some();
        let key = key_ident.to_string();
        match key.as_str() {
            "normalizer" => Ok(HirpdagArg::Normalizer),
            "reference_type" => {
                if has_value {
                    if let syn::Lit::Str(s) = value_lit.unwrap() {
                        return Ok(HirpdagArg::ReferenceType(s.value()));
                    }
                }
                Err(syn::Error::new(
                    input.span(),
                    "HirpdagArg key reference_type requires a string argument.",
                ))
            }
            "reference_weak_type" => {
                if has_value {
                    if let syn::Lit::Str(s) = value_lit.unwrap() {
                        return Ok(HirpdagArg::ReferenceWeakType(s.value()));
                    }
                }
                Err(syn::Error::new(
                    input.span(),
                    "HirpdagArg key reference_weak_type requires a string argument.",
                ))
            }
            "table_type" => {
                if has_value {
                    if let syn::Lit::Str(s) = value_lit.unwrap() {
                        return Ok(HirpdagArg::TableType(s.value()));
                    }
                }
                Err(syn::Error::new(
                    input.span(),
                    "HirpdagArg key table_type requires a string argument.",
                ))
            }
            "tableshared_type" => {
                if has_value {
                    if let syn::Lit::Str(s) = value_lit.unwrap() {
                        return Ok(HirpdagArg::TableSharedType(s.value()));
                    }
                }
                Err(syn::Error::new(
                    input.span(),
                    "HirpdagArg key tableshared_type requires a string argument.",
                ))
            }
            "build_tableshared_type" => {
                if has_value {
                    if let syn::Lit::Str(s) = value_lit.unwrap() {
                        return Ok(HirpdagArg::BuildTableSharedType(s.value()));
                    }
                }
                Err(syn::Error::new(
                    input.span(),
                    "HirpdagArg key build_tableshared_type requires a string argument.",
                ))
            }
            _ => Err(syn::Error::new(input.span(), "HirpdagArg key unrecognised")),
        }
    }
}

struct HirpdagArgs {
    args: Vec<HirpdagArg>,
}

impl syn::parse::Parse for HirpdagArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let vars =
            syn::punctuated::Punctuated::<HirpdagArg, syn::Token![,]>::parse_terminated(input)?;
        Ok(Self {
            args: vars.into_iter().collect(),
        })
    }
}

struct HirpdagConfig {
    normalizer: bool,

    reference_type: String,
    reference_weak_type: String,

    table_type: String,
    tableshared_type: String,
    build_tableshared_type: String,
}

impl HirpdagConfig {
    fn default() -> Self {
        Self {
            normalizer: false,
            reference_type: "hirpdag_hashconsing::RefArc<D>".to_string(),
            reference_weak_type: "hirpdag_hashconsing::RefArcWeak<D>".to_string(),
            table_type: "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>, hirpdag_hashconsing::TableVecLinearWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>>>".to_string(),
            tableshared_type: "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>".to_string(),
            build_tableshared_type: "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>".to_string(),
        }
    }

    fn from(args: &HirpdagArgs) -> Self {
        // Start with default config.
        let mut config = Self::default();
        // Override according to the arguments in macro attributes.
        for a in &args.args {
            match a {
                HirpdagArg::Normalizer => {
                    config.normalizer = true;
                }
                HirpdagArg::ReferenceType(name) => {
                    config.reference_type = name.clone();
                }
                HirpdagArg::ReferenceWeakType(name) => {
                    config.reference_weak_type = name.clone();
                }
                HirpdagArg::TableType(name) => {
                    config.table_type = name.clone();
                }
                HirpdagArg::TableSharedType(name) => {
                    config.tableshared_type = name.clone();
                }
                HirpdagArg::BuildTableSharedType(name) => {
                    config.build_tableshared_type = name.clone();
                }
            }
        }
        config
    }
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
    let tokens = match &input.data {
        syn::Data::Struct(s) => expand_hirpdag_struct(&config, input, s),
        syn::Data::Enum(e) => expand_hirpdag_enum(&config, input, e),
        _ => panic!("`#[Hirpdag]` can only be applied to named structs and enums"),
    };
    // For debugging:
    // eprintln!("TOKENS:\n{}", tokens);
    tokens
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

fn get_default_normalizer(
    config: &HirpdagConfig,
    fields_named: &syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    if config.normalizer {
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
        guard.push(name_str.clone());
    }

    let hirpdag_ref_name_str = format!("{}", name_str);
    let hirpdag_ref_name = Ident::new(&hirpdag_ref_name_str, Span::call_site());

    let hirpdag_struct_name_str = format!("HirpdagStruct{}", name_str);
    let hirpdag_struct_name = Ident::new(&hirpdag_struct_name_str, Span::call_site());

    let hirpdag_table_name_str = format!("HIRPDAG_TABLE_{}", name_uppercase_str);
    let hirpdag_table_name = Ident::new(&hirpdag_table_name_str, Span::call_site());

    let hirpdag_rewrite_method_name_str = format!("rewrite_{}", name_str);
    let hirpdag_rewrite_method_name =
        Ident::new(&hirpdag_rewrite_method_name_str, Span::call_site());

    let fields_named = get_fields_named(input_struct);
    let fields_declarations = get_fields_declarations(fields_named);
    let fields_list = get_fields_list(fields_named);
    let fields_compute_meta = get_fields_compute_meta(fields_named);
    let fields_rewrite = get_fields_rewrite(fields_named);

    let default_normalizer = get_default_normalizer(config, fields_named);

    quote! {
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
            fn cmp(&self, other: &#hirpdag_ref_name) -> std::cmp::Ordering {
                // TODO: This should be more efficient, not a deep comparison.
                ((*(self.0))).cmp(&(*(other.0)))
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

            // If normalizer is not provided, generate one.
            #default_normalizer

            #[allow(non_snake_case)]
            fn default_rewrite<T: HirpdagRewriter>(&self, rewriter: &T) -> Self {
                Self::new(
                    #fields_rewrite
                    )
            }
        }

        // ==== Rewriting

        impl<T: HirpdagRewriter> HirpdagRewritable<T> for #hirpdag_ref_name {
            fn hirpdag_rewrite(&self, rewriter: &T) -> Self {
                rewriter.#hirpdag_rewrite_method_name(self)
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
        guard.push(name_str.clone());
    }

    let hirpdag_rewrite_method_name_str = format!("rewrite_{}", name_str);
    let hirpdag_rewrite_method_name =
        Ident::new(&hirpdag_rewrite_method_name_str, Span::call_site());

    let variants_declarations = get_variants_declarations(input_enum);
    let variants_compute_meta = get_variants_compute_meta(input_enum);
    let variants_rewrite = get_variants_rewrite(input_enum);

    quote! {
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

    let rewrite_methods = guard.iter().map(|name| get_rewrite_datatype(name)).fold(
        proc_macro2::TokenStream::new(),
        |mut s, t| {
            s.extend(t);
            s
        },
    );

    let cache_members = guard.iter().map(|name| get_cache_member(name)).fold(
        proc_macro2::TokenStream::new(),
        |mut s, t| {
            s.extend(t);
            s
        },
    );

    let cache_members_new = guard.iter().map(|name| get_cache_member_new(name)).fold(
        proc_macro2::TokenStream::new(),
        |mut s, t| {
            s.extend(t);
            s
        },
    );

    let cache_methods = guard.iter().map(|name| get_cache_rewrite(name)).fold(
        proc_macro2::TokenStream::new(),
        |mut s, t| {
            s.extend(t);
            s
        },
    );

    guard.clear();

    let reference_type: proc_macro2::TokenStream = config.reference_type.parse().unwrap();
    let reference_weak_type: proc_macro2::TokenStream = config.reference_weak_type.parse().unwrap();

    let table_type: proc_macro2::TokenStream = config.table_type.parse().unwrap();
    let tableshared_type: proc_macro2::TokenStream = config.tableshared_type.parse().unwrap();
    let build_tableshared_type: proc_macro2::TokenStream =
        config.build_tableshared_type.parse().unwrap();

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
    };
    t.into()
}
