#![forbid(unsafe_code)]

use proc_macro2::{Ident, TokenStream};

pub enum HirpdagArg {
    /// Normalizer will be defined by user for construction.
    Normalizer,

    /// This struct type can be a serialization root: it gets a vector in the
    /// generated HirpdagArchiveRoots struct.
    Root,

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

    /// Named preset selecting the reference and table types together.
    Preset(String),
}

/// Preset used when no `preset`/type arguments are given.
const DEFAULT_PRESET: &str = "arc_hash_linear";

/// Known named configuration presets, selectable with
/// `#[hirpdag_module(preset = "name")]`.
const PRESETS: &[&str] = &[
    "arc_hash_linear",
    "arc_hash_sorted",
    "arc_tovweaktable",
    "leak_hash_linear",
    "sep_hash_linear",
    "seppad_hash_linear",
    "sepu32_hash_linear",
    "tlc_hash_linear",
];

/// The type strings that select a hash-consing implementation.
///
/// Each string is spliced into a `type Impl…<D> = …;` alias in the generated
/// code, so they share a `D` data-type parameter and refer to each other
/// through the generated aliases `ImplRef<D>`, `ImplRefWeak<D>` and
/// `ImplTable<D>`. Using those aliases instead of respelling the concrete
/// reference/table types keeps the strings short and makes the shared-table
/// types follow whatever reference and table the config ends up with.
#[derive(Clone)]
struct ConfigTypes {
    reference_type: String,
    reference_weak_type: String,
    table_type: String,
    tableshared_type: String,
    build_tableshared_type: String,
}

/// The [`ConfigTypes`] for a named preset, or `None` if the name is unknown.
///
/// A preset only chooses a strong/weak reference pair and an inner table
/// strategy from hirpdag_hashconsing. The shared-table types are always the
/// sharded implementation over the configured reference (`ImplRef<D>`) and
/// inner table (`ImplTable<D>`), so they are the same for every preset:
/// `TableSharedSharded`'s `HB` defaults to `DefaultHasher` and is left off,
/// while `BuildTableSharedSharded` has no default hasher so it is named.
fn preset_types(name: &str) -> Option<ConfigTypes> {
    // Strong/weak reference pair, named `Ref…` / `Ref…Weak`.
    fn reference(base: &str) -> (String, String) {
        (
            format!("hirpdag::hirpdag_hashconsing::{base}<D>"),
            format!("hirpdag::hirpdag_hashconsing::{base}Weak<D>"),
        )
    }
    // A hashmap that falls back to `inner_table` at larger sizes.
    fn hashmap_fallback(inner_table: &str) -> String {
        format!(
            "hirpdag::hirpdag_hashconsing::TableHashmapFallbackWeak<D, ImplRef<D>, ImplRefWeak<D>, hirpdag::hirpdag_hashconsing::{inner_table}<D, ImplRef<D>, ImplRefWeak<D>>>"
        )
    }
    let (reference_base, table_type) = match name {
        "arc_hash_linear" => ("RefArc", hashmap_fallback("TableVecLinearWeak")),
        "arc_hash_sorted" => ("RefArc", hashmap_fallback("TableVecSortedWeak")),
        "arc_tovweaktable" => (
            "RefArc",
            "hirpdag::hirpdag_hashconsing::TableTovWeakTable<D, ImplRef<D>, ImplRefWeak<D>>"
                .to_string(),
        ),
        "leak_hash_linear" => ("RefLeak", hashmap_fallback("TableVecLinearWeak")),
        // Reference-counting experiments with counts stored separately from the
        // data (see hirpdag_hashconsing::reference_sepcount).
        "sep_hash_linear" => ("RefSep", hashmap_fallback("TableVecLinearWeak")),
        "seppad_hash_linear" => ("RefSepPad", hashmap_fallback("TableVecLinearWeak")),
        "sepu32_hash_linear" => ("RefSepU32", hashmap_fallback("TableVecLinearWeak")),
        // Thread-local deferred reference counting (see
        // hirpdag_hashconsing::reference_tlc).
        "tlc_hash_linear" => ("RefTlc", hashmap_fallback("TableVecLinearWeak")),
        _ => return None,
    };
    let (reference_type, reference_weak_type) = reference(reference_base);
    Some(ConfigTypes {
        reference_type,
        reference_weak_type,
        table_type,
        tableshared_type: "hirpdag::hirpdag_hashconsing::TableSharedSharded<D, ImplRef<D>, ImplTable<D>>".to_string(),
        build_tableshared_type: "hirpdag::hirpdag_hashconsing::BuildTableSharedSharded<D, ImplRef<D>, ImplTable<D>, hirpdag::hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>".to_string(),
    })
}

impl syn::parse::Parse for HirpdagArg {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let arg_name_ident: Ident = input.parse()?;
        let opeq: Option<syn::Token![=]> = input.parse()?;
        let value_lit: Option<syn::Lit> = input.parse()?;
        let arg_name = arg_name_ident.to_string();
        enum Handler {
            /// Argument name is not recognised
            NotRecognised,
            /// Flag is present or not
            Flag(fn() -> syn::Result<HirpdagArg>),
            /// String literal
            String(fn(&syn::LitStr) -> syn::Result<HirpdagArg>),
        }
        let arg_handler = match arg_name.as_str() {
            "normalizer" => Handler::Flag(|| Ok(Self::Normalizer)),
            "root" => Handler::Flag(|| Ok(Self::Root)),
            "reference_type" => {
                Handler::String(|s: &syn::LitStr| Ok(Self::ReferenceType(s.value())))
            }
            "reference_weak_type" => {
                Handler::String(|s: &syn::LitStr| Ok(Self::ReferenceWeakType(s.value())))
            }
            "table_type" => Handler::String(|s: &syn::LitStr| Ok(Self::TableType(s.value()))),
            "tableshared_type" => {
                Handler::String(|s: &syn::LitStr| Ok(Self::TableSharedType(s.value())))
            }
            "build_tableshared_type" => {
                Handler::String(|s: &syn::LitStr| Ok(Self::BuildTableSharedType(s.value())))
            }
            "preset" => Handler::String(|s: &syn::LitStr| {
                let name = s.value();
                if preset_types(&name).is_none() {
                    return Err(syn::Error::new(
                        s.span(),
                        format!(
                            "unknown preset `{}`; known presets: {}",
                            name,
                            PRESETS.join(", ")
                        ),
                    ));
                }
                Ok(Self::Preset(name))
            }),
            _ => Handler::NotRecognised,
        };
        match arg_handler {
            Handler::NotRecognised => Err(syn::Error::new(
                input.span(),
                format!("HirpdagArg {} was not recognised", arg_name.as_str()),
            )),
            Handler::String(build_arg) => {
                if opeq.is_none() {
                    return Err(syn::Error::new(
                        input.span(),
                        "HirpdagArg expected = syntax.",
                    ));
                }
                if let Some(syn::Lit::Str(s)) = value_lit {
                    build_arg(&s)
                } else {
                    Err(syn::Error::new(
                        input.span(),
                        format!(
                            "HirpdagArg {} requires a string argument.",
                            arg_name.as_str()
                        ),
                    ))
                }
            }
            Handler::Flag(build_arg) => build_arg(),
        }
    }
}

pub struct HirpdagArgs {
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

pub struct HirpdagConfig {
    normalizer: bool,
    root: bool,
    types: ConfigTypes,
}

impl HirpdagConfig {
    fn default() -> Self {
        Self {
            normalizer: false,
            root: false,
            types: preset_types(DEFAULT_PRESET).expect("default preset is known"),
        }
    }

    pub fn from(args: &HirpdagArgs) -> Self {
        // Start with default config.
        let mut config = Self::default();
        // Override according to the arguments in macro attributes.
        for a in &args.args {
            match a {
                HirpdagArg::Normalizer => config.normalizer = true,
                HirpdagArg::Root => config.root = true,
                HirpdagArg::ReferenceType(name) => config.types.reference_type = name.clone(),
                HirpdagArg::ReferenceWeakType(name) => {
                    config.types.reference_weak_type = name.clone()
                }
                HirpdagArg::TableType(name) => config.types.table_type = name.clone(),
                HirpdagArg::TableSharedType(name) => config.types.tableshared_type = name.clone(),
                HirpdagArg::BuildTableSharedType(name) => {
                    config.types.build_tableshared_type = name.clone()
                }
                HirpdagArg::Preset(name) => {
                    // Validated when the argument was parsed.
                    config.types = preset_types(name).expect("preset validated at parse time");
                }
            }
        }
        config
    }

    pub fn has_normalizer(&self) -> bool {
        self.normalizer
    }
    pub fn is_root(&self) -> bool {
        self.root
    }
    pub fn reference_type(&self) -> TokenStream {
        self.types.reference_type.parse().unwrap()
    }
    pub fn reference_weak_type(&self) -> TokenStream {
        self.types.reference_weak_type.parse().unwrap()
    }
    pub fn table_type(&self) -> TokenStream {
        self.types.table_type.parse().unwrap()
    }
    pub fn tableshared_type(&self) -> TokenStream {
        self.types.tableshared_type.parse().unwrap()
    }
    pub fn build_tableshared_type(&self) -> TokenStream {
        self.types.build_tableshared_type.parse().unwrap()
    }
}
