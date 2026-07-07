//! Implementation of the `hirpdag_configurations!` function-like macro.
//!
//! Expands a set of items (hirpdag type definitions plus code using them)
//! once per named hash-consing configuration, each inside a module named
//! after the configuration.
//!
//! The `#[hirpdag]` types and the module-level `hirpdag_end` code are
//! expanded directly here rather than by emitting `#[hirpdag]` /
//! `#[hirpdag_end]` attributes for the compiler to expand later. The
//! registry handoff between those attributes requires them to expand in
//! source order. The compiler expands attribute invocations in source order
//! when their paths resolve immediately, but inside a macro-generated
//! module whose attribute paths resolve through a glob import
//! (`use hirpdag::*`), the invocations are deferred and then expanded out
//! of source order — a module's end marker can expand before the type
//! definitions in that module, so the marker sees an empty registry.
//! (Invoking the attributes by absolute path avoids the deferral and
//! happens to restore source order, but that leans even harder on
//! unspecified expansion scheduling.) Expanding everything in this single
//! invocation removes the dependence on expansion order entirely.

use proc_macro2::{Ident, TokenStream};

use crate::config::{HirpdagArgs, HirpdagConfig};

pub struct ConfigurationsInput {
    configurations: Vec<Ident>,
    items: Vec<syn::Item>,
}

impl syn::parse::Parse for ConfigurationsInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let keyword: Ident = input.parse()?;
        if keyword != "configurations" {
            return Err(syn::Error::new(
                keyword.span(),
                "expected `configurations = [name, ...];` before the items",
            ));
        }
        input.parse::<syn::Token![=]>()?;
        let list;
        syn::bracketed!(list in input);
        let configurations =
            syn::punctuated::Punctuated::<Ident, syn::Token![,]>::parse_terminated(&list)?
                .into_iter()
                .collect();
        input.parse::<syn::Token![;]>()?;
        let mut items = Vec::new();
        while !input.is_empty() {
            items.push(input.parse()?);
        }
        Ok(Self {
            configurations,
            items,
        })
    }
}

const KNOWN_CONFIGURATIONS: &[&str] = &[
    "arc_hash_linear",
    "arc_hash_sorted",
    "arc_tovweaktable",
    "leak_hash_linear",
];

/// Named hash-consing configuration presets.
///
/// Each preset names a strong/weak reference type pair and a table
/// implementation from hirpdag_hashconsing; the shared-table wrapper types
/// are derived from the reference type.
fn configuration_preset(name: &str) -> Option<HirpdagConfig> {
    const ARC: (&str, &str) = (
        "hirpdag_hashconsing::RefArc<D>",
        "hirpdag_hashconsing::RefArcWeak<D>",
    );
    const LEAK: (&str, &str) = (
        "hirpdag_hashconsing::RefLeak<D>",
        "hirpdag_hashconsing::RefLeakWeak<D>",
    );
    fn hashmap_fallback(fallback_table: &str, (r, w): (&str, &str)) -> String {
        format!(
            "hirpdag_hashconsing::TableHashmapFallbackWeak<D, {r}, {w}, hirpdag_hashconsing::{fallback_table}<D, {r}, {w}>>"
        )
    }
    let ((reference_type, reference_weak_type), table_type) = match name {
        "arc_hash_linear" => (ARC, hashmap_fallback("TableVecLinearWeak", ARC)),
        "arc_hash_sorted" => (ARC, hashmap_fallback("TableVecSortedWeak", ARC)),
        "arc_tovweaktable" => (
            ARC,
            format!(
                "hirpdag_hashconsing::TableTovWeakTable<D, {}, {}>",
                ARC.0, ARC.1
            ),
        ),
        "leak_hash_linear" => (LEAK, hashmap_fallback("TableVecLinearWeak", LEAK)),
        _ => return None,
    };
    Some(HirpdagConfig::with_types(
        reference_type.to_string(),
        reference_weak_type.to_string(),
        table_type,
        format!("hirpdag_hashconsing::TableSharedSharded<D, {reference_type}, ImplTable<D>>"),
        format!(
            "hirpdag_hashconsing::BuildTableSharedSharded<D, {reference_type}, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
        ),
    ))
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
        syn::Meta::Path(_) => syn::parse2(TokenStream::new()),
        syn::Meta::List(list) => syn::parse2(list.tokens.clone()),
        syn::Meta::NameValue(nv) => Err(syn::Error::new_spanned(
            nv,
            "unexpected `#[hirpdag = ...]`; use `#[hirpdag]` or `#[hirpdag(...)]`",
        )),
    }
}

pub fn expand(input: TokenStream) -> syn::Result<TokenStream> {
    let input: ConfigurationsInput = syn::parse2(input)?;
    let mut output = TokenStream::new();
    for module in &input.configurations {
        let config = configuration_preset(&module.to_string()).ok_or_else(|| {
            syn::Error::new(
                module.span(),
                format!(
                    "unknown hirpdag configuration `{}`; known configurations: {}",
                    module,
                    KNOWN_CONFIGURATIONS.join(", ")
                ),
            )
        })?;
        let mut body = TokenStream::new();
        for item in &input.items {
            let mut item = item.clone();
            if let Some(attr) = take_hirpdag_attr(&mut item) {
                let args = parse_hirpdag_args(&attr)?;
                let derive_input: syn::DeriveInput = match item {
                    syn::Item::Struct(s) => s.into(),
                    syn::Item::Enum(e) => e.into(),
                    _ => unreachable!("take_hirpdag_attr only matches structs and enums"),
                };
                // Registers the type in DATA_TYPES for expand_hirpdag_end.
                body.extend(crate::expand_hirpdag(&args, &derive_input));
            } else {
                body.extend(quote! { #item });
            }
        }
        body.extend(crate::expand_hirpdag_end(&config));
        output.extend(quote! {
            mod #module {
                use hirpdag::*;

                #body
            }
        });
    }
    Ok(output)
}
