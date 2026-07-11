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
    "leak_hash_linear",
    "sep_hash_linear",
    "seppad_hash_linear",
    "sepu32_hash_linear",
    "tlc_hash_linear",
    // Tables backed by third-party collection crates (behind the
    // `third-party-tables` feature). Each concurrent backend has two variants:
    // `*_strong` stores strong references (retain-forever), while the un-suffixed
    // name wraps it in `TableAmortizedPurge` for weak-key hash-consing (dead
    // nodes are evicted). `arc_tovweaktable` is weak-key with GC.
    "arc_tovweaktable",
    "arc_dashmap",
    "arc_dashmap_strong",
    "arc_flurry",
    "arc_flurry_strong",
    "arc_skipmap",
    "arc_skipmap_strong",
    "arc_arcswap",
    "arc_arcswap_strong",
];

/// The type strings that select a hash-consing implementation.
///
/// `reference_type`, `reference_weak_type`, `tableshared_type` and
/// `build_tableshared_type` are always emitted, as the aliases `ImplRef<D>`,
/// `ImplRefWeak<D>`, `ImplTableShared<D>` and `ImplBuildTableShared<D>`.
/// `ImplRef` / `ImplRefWeak` are the strong/weak reference pair — the vocabulary
/// any table implementation draws on to name whichever reference-counting
/// implementation it was configured with, so both are available whether or not a
/// given table happens to use the weak side.
///
/// `aliases` is a list of extra `type <name><D> = <rhs>;` declarations a config
/// emits so its shared-table strings can stay short by referring to a named
/// helper instead of respelling a long concrete type. The lock-based backends
/// declare `ImplTable` (they are generic over an inner table); the
/// concurrent-collection backends store the mapping directly and declare none.
///
/// Every string is spliced into a `type …<D> = …;` alias, so they share a `D`
/// data-type parameter and may refer to each other through these alias names.
#[derive(Clone)]
struct ConfigTypes {
    reference_type: String,
    reference_weak_type: String,
    aliases: Vec<(String, String)>,
    tableshared_type: String,
    build_tableshared_type: String,
}

impl ConfigTypes {
    /// Insert the helper alias `name` (or replace it if already present).
    fn set_alias(&mut self, name: &str, rhs: String) {
        match self.aliases.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = rhs,
            None => self.aliases.push((name.to_string(), rhs)),
        }
    }
}

/// The [`ConfigTypes`] for a named preset, or `None` if the name is unknown.
fn preset_types(name: &str) -> Option<ConfigTypes> {
    // A hashmap that falls back to `inner_table` at larger sizes.
    fn hashmap_fallback(inner_table: &str) -> String {
        format!(
            "hirpdag::hirpdag_hashconsing::TableHashmapFallbackWeak<D, ImplRef<D>, ImplRefWeak<D>, hirpdag::hirpdag_hashconsing::{inner_table}<D, ImplRef<D>, ImplRefWeak<D>>>"
        )
    }
    // A `ConfigTypes` for a lock-based preset: reference `base`, generic over the
    // given inner `ThreadUnsafeTable` (exposed as the `ImplTable` alias), shared via the
    // sharded-mutex table.
    fn sharded(base: &str, inner_table: String) -> ConfigTypes {
        ConfigTypes {
            reference_type: format!("hirpdag::hirpdag_hashconsing::{base}<D>"),
            reference_weak_type: format!("hirpdag::hirpdag_hashconsing::{base}Weak<D>"),
            aliases: vec![("ImplTable".to_string(), inner_table)],
            tableshared_type: "hirpdag::hirpdag_hashconsing::TableSharedSharded<D, ImplRef<D>, ImplRefWeak<D>, ImplTable<D>>".to_string(),
            build_tableshared_type: "hirpdag::hirpdag_hashconsing::BuildTableSharedSharded<D, ImplRef<D>, ImplRefWeak<D>, ImplTable<D>, hirpdag::hirpdag_hashconsing::BuildThreadUnsafeTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>".to_string(),
        }
    }
    // A `ConfigTypes` for a preset backed by a third-party concurrent collection
    // named `TableShared{shared_base}`. These store the mapping directly and are
    // not generic over an inner `ThreadUnsafeTable`, so they declare no `ImplTable` alias.
    // `hashed` backends take a default-hasher generic argument; ordered /
    // self-hashing backends (skipmap) do not.
    fn concurrent(base: &str, shared_base: &str, hashed: bool) -> ConfigTypes {
        let hasher = if hashed {
            ", std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>"
        } else {
            ""
        };
        ConfigTypes {
            reference_type: format!("hirpdag::hirpdag_hashconsing::{base}<D>"),
            reference_weak_type: format!("hirpdag::hirpdag_hashconsing::{base}Weak<D>"),
            aliases: Vec::new(),
            tableshared_type: format!(
                "hirpdag::hirpdag_hashconsing::TableShared{shared_base}<D, ImplRef<D>>"
            ),
            build_tableshared_type: format!(
                "hirpdag::hirpdag_hashconsing::BuildTableShared{shared_base}<D, ImplRef<D>{hasher}>"
            ),
        }
    }
    // The purging variant of a concurrent backend: the backend stores weak
    // references (`WeakEntryStrong`) and is wrapped in `TableAmortizedPurge`,
    // which adds amortized eviction of dead entries. Unlike the strong variant
    // this frees unreferenced nodes. The inner backend takes its default hasher
    // (skipmap / evmap have none), so no hasher argument is threaded here.
    fn concurrent_purging(base: &str, shared_base: &str) -> ConfigTypes {
        let inner = format!(
            "hirpdag::hirpdag_hashconsing::TableShared{shared_base}<D, hirpdag::hirpdag_hashconsing::WeakEntryStrong<D, ImplRef<D>, ImplRefWeak<D>>>"
        );
        ConfigTypes {
            reference_type: format!("hirpdag::hirpdag_hashconsing::{base}<D>"),
            reference_weak_type: format!("hirpdag::hirpdag_hashconsing::{base}Weak<D>"),
            aliases: Vec::new(),
            tableshared_type: format!(
                "hirpdag::hirpdag_hashconsing::TableAmortizedPurge<D, ImplRef<D>, ImplRefWeak<D>, {inner}>"
            ),
            build_tableshared_type: format!(
                "hirpdag::hirpdag_hashconsing::BuildTableAmortizedPurge<D, ImplRef<D>, ImplRefWeak<D>, {inner}>"
            ),
        }
    }

    let tovweaktable =
        "hirpdag::hirpdag_hashconsing::TableTovWeakTable<D, ImplRef<D>, ImplRefWeak<D>>"
            .to_string();

    Some(match name {
        "arc_hash_linear" => sharded("RefArc", hashmap_fallback("TableVecLinearWeak")),
        "arc_hash_sorted" => sharded("RefArc", hashmap_fallback("TableVecSortedWeak")),
        "leak_hash_linear" => sharded("RefLeak", hashmap_fallback("TableVecLinearWeak")),
        // Reference-counting experiments with counts stored separately from the
        // data (see hirpdag_hashconsing::reference::sepcount).
        "sep_hash_linear" => sharded("RefSep", hashmap_fallback("TableVecLinearWeak")),
        "seppad_hash_linear" => sharded("RefSepPad", hashmap_fallback("TableVecLinearWeak")),
        "sepu32_hash_linear" => sharded("RefSepU32", hashmap_fallback("TableVecLinearWeak")),
        // Thread-local deferred reference counting (see
        // hirpdag_hashconsing::reference::tlc).
        "tlc_hash_linear" => sharded("RefTlc", hashmap_fallback("TableVecLinearWeak")),
        // Tables backed by third-party collection crates (behind the
        // `third-party-tables` feature). `arc_tovweaktable` wraps the weak-table
        // crate's `WeakHashSet` as an inner `ThreadUnsafeTable` behind the sharded
        // shared table; the rest store the mapping directly in a concurrent collection
        // (strong references, no weak-reference GC) via `TableShared*`. `RefArc`
        // is used because the concurrent backends require a `Send + Sync`
        // reference. See the `table::*_strong` / `table::shared_*` /
        // `table::tov_weak_table_threadunsafe` modules.
        "arc_tovweaktable" => sharded("RefArc", tovweaktable),
        // Strong variants retain every interned node (no weak-reference GC);
        // the un-suffixed variants wrap the same backend in `TableAmortizedPurge`
        // so unreferenced nodes are evicted (weak-key hash-consing).
        "arc_dashmap_strong" => concurrent("RefArc", "DashMap", true),
        "arc_flurry_strong" => concurrent("RefArc", "Flurry", true),
        "arc_skipmap_strong" => concurrent("RefArc", "SkipMap", false),
        "arc_arcswap_strong" => concurrent("RefArc", "ArcSwap", true),
        "arc_dashmap" => concurrent_purging("RefArc", "DashMap"),
        "arc_flurry" => concurrent_purging("RefArc", "Flurry"),
        "arc_skipmap" => concurrent_purging("RefArc", "SkipMap"),
        "arc_arcswap" => concurrent_purging("RefArc", "ArcSwap"),
        _ => return None,
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
                HirpdagArg::TableType(name) => config.types.set_alias("ImplTable", name.clone()),
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
    /// The extra `type <name><D> = <rhs>;` helper aliases this config declares,
    /// as `(name, rhs)` token pairs for the generated code to emit.
    pub fn helper_aliases(&self) -> Vec<(Ident, TokenStream)> {
        self.types
            .aliases
            .iter()
            .map(|(name, rhs)| {
                (
                    Ident::new(name, proc_macro2::Span::call_site()),
                    rhs.parse().unwrap(),
                )
            })
            .collect()
    }
    pub fn tableshared_type(&self) -> TokenStream {
        self.types.tableshared_type.parse().unwrap()
    }
    pub fn build_tableshared_type(&self) -> TokenStream {
        self.types.build_tableshared_type.parse().unwrap()
    }
}
