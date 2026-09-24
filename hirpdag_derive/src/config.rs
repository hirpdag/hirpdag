#![forbid(unsafe_code)]

use proc_macro2::{Ident, Span, TokenStream};

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
    // `third-party-tables` feature).
    "arc_tovweaktable",
    "arc_dashmap",
    "arc_flurry",
    "arc_skipmap",
    "arc_arcswap",
];

/// The type strings that select a hash-consing implementation.
///
/// `reference_type`, `reference_weak_type` and `tableshared_type` are always
/// emitted, as the aliases `ImplRef<D>`, `ImplRefWeak<D>` and
/// `ImplTableShared<D>`. The shared table builds itself from those type
/// parameters (it is `Default`), so no factory is named here.
/// `ImplRef` / `ImplRefWeak` are the strong/weak reference pair, the vocabulary
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
            tableshared_type:
                "hirpdag::hirpdag_hashconsing::TableSharedSharded<D, ImplRef<D>, ImplTable<D>>"
                    .to_string(),
        }
    }
    // A `ConfigTypes` for a preset backed by a third-party concurrent collection
    // named `TableShared{shared_base}`. These store the mapping directly and are
    // not generic over an inner `ThreadUnsafeTable`, so they declare no `ImplTable`
    // alias. The hashed backends take their hasher from their own default type
    // parameter; the ordered one (skipmap) has none.
    fn concurrent(base: &str, shared_base: &str) -> ConfigTypes {
        ConfigTypes {
            reference_type: format!("hirpdag::hirpdag_hashconsing::{base}<D>"),
            reference_weak_type: format!("hirpdag::hirpdag_hashconsing::{base}Weak<D>"),
            aliases: Vec::new(),
            tableshared_type: format!(
                "hirpdag::hirpdag_hashconsing::TableShared{shared_base}<D, ImplRef<D>>"
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
        "arc_dashmap" => concurrent("RefArc", "DashMap"),
        "arc_flurry" => concurrent("RefArc", "Flurry"),
        "arc_skipmap" => concurrent("RefArc", "SkipMap"),
        "arc_arcswap" => concurrent("RefArc", "ArcSwap"),
        _ => return None,
    })
}

/// One `name` or `name = "value"` entry of an attribute's argument list,
/// before it is checked against what that attribute accepts.
struct RawArg {
    name: Ident,
    eq: Option<syn::Token![=]>,
    value: Option<syn::Lit>,
}

impl syn::parse::Parse for RawArg {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self {
            name: input.parse()?,
            eq: input.parse()?,
            value: input.parse()?,
        })
    }
}

impl RawArg {
    /// Checks the argument is a bare flag and returns its span.
    fn flag(&self) -> syn::Result<Span> {
        // A flag is set by being present. Accepting and ignoring a value
        // would turn `root = false` into a root.
        if self.eq.is_some() || self.value.is_some() {
            return Err(syn::Error::new(
                self.name.span(),
                format!(
                    "`{0}` is a flag and takes no value; write `{0}` to set it, or leave it out",
                    self.name
                ),
            ));
        }
        Ok(self.name.span())
    }

    /// Checks the argument is `name = "value"` and returns the string.
    fn string(&self) -> syn::Result<syn::LitStr> {
        match (&self.eq, &self.value) {
            (Some(_), Some(syn::Lit::Str(s))) => Ok(s.clone()),
            _ => Err(syn::Error::new(
                self.name.span(),
                format!("`{0}` takes a string: `{0} = \"...\"`", self.name),
            )),
        }
    }

    /// Checks the argument is `name = "type"` with a string that parses as a
    /// Rust type, so a typo is reported at the argument rather than inside
    /// the expansion (or as a macro panic, for a string that does not lex).
    fn type_string(&self) -> syn::Result<String> {
        let s = self.string()?;
        syn::parse_str::<syn::Type>(&s.value()).map_err(|e| {
            syn::Error::new(s.span(), format!("`{}` must name a type: {}", self.name, e))
        })?;
        Ok(s.value())
    }

    /// The error for an argument that `attribute` does not accept. An
    /// argument that belongs to the other attribute says where it goes.
    fn not_accepted(&self, attribute: &str, accepted: &[&str]) -> syn::Error {
        let name = self.name.to_string();
        let message = if MODULE_ARGS.contains(&name.as_str()) {
            format!(
                "`{name}` is a `#[hirpdag_module(...)]` argument; \
                 it applies to the whole module, not to one type"
            )
        } else if TYPE_ARGS.contains(&name.as_str()) {
            format!("`{name}` is a `#[hirpdag(...)]` argument; put it on the struct")
        } else {
            format!(
                "unknown `{attribute}` argument `{name}`; expected one of: {}",
                accepted.join(", ")
            )
        };
        syn::Error::new(self.name.span(), message)
    }
}

/// Parses a comma-separated argument list.
fn parse_raw_args(input: syn::parse::ParseStream) -> syn::Result<Vec<RawArg>> {
    Ok(
        syn::punctuated::Punctuated::<RawArg, syn::Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect(),
    )
}

/// The arguments `#[hirpdag_module(...)]` accepts.
const MODULE_ARGS: &[&str] = &[
    "preset",
    "reference_type",
    "reference_weak_type",
    "table_type",
    "tableshared_type",
];

/// The arguments `#[hirpdag(...)]` accepts.
const TYPE_ARGS: &[&str] = &["normalizer", "root"];

/// The configuration of a module: which hash-consing implementation its
/// generated code uses. Parsed from `#[hirpdag_module(...)]`.
///
/// Arguments apply in order, so a `preset` replaces every type chosen before
/// it and an explicit type string after it overrides that one type.
pub struct ModuleConfig {
    types: ConfigTypes,
}

impl syn::parse::Parse for ModuleConfig {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut types = preset_types(DEFAULT_PRESET).expect("default preset is known");
        for arg in parse_raw_args(input)? {
            match arg.name.to_string().as_str() {
                "preset" => {
                    let s = arg.string()?;
                    types = preset_types(&s.value()).ok_or_else(|| {
                        syn::Error::new(
                            s.span(),
                            format!(
                                "unknown preset `{}`; known presets: {}",
                                s.value(),
                                PRESETS.join(", ")
                            ),
                        )
                    })?;
                }
                "reference_type" => types.reference_type = arg.type_string()?,
                "reference_weak_type" => types.reference_weak_type = arg.type_string()?,
                "table_type" => types.set_alias("ImplTable", arg.type_string()?),
                "tableshared_type" => types.tableshared_type = arg.type_string()?,
                _ => return Err(arg.not_accepted("#[hirpdag_module]", MODULE_ARGS)),
            }
        }
        Ok(Self { types })
    }
}

impl ModuleConfig {
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
}

/// The configuration of one data type. Parsed from `#[hirpdag(...)]`.
///
/// Each flag keeps the span it was written at, so a flag that does not apply
/// to the item it is on (an enum) can be reported there.
#[derive(Default)]
pub struct TypeConfig {
    normalizer: Option<Span>,
    root: Option<Span>,
}

impl syn::parse::Parse for TypeConfig {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut config = Self::default();
        for arg in parse_raw_args(input)? {
            match arg.name.to_string().as_str() {
                "normalizer" => config.normalizer = Some(arg.flag()?),
                "root" => config.root = Some(arg.flag()?),
                _ => return Err(arg.not_accepted("#[hirpdag]", TYPE_ARGS)),
            }
        }
        Ok(config)
    }
}

impl TypeConfig {
    /// Where `normalizer` was written, if it was.
    pub fn normalizer(&self) -> Option<Span> {
        self.normalizer
    }
    /// Where `root` was written, if it was.
    pub fn root(&self) -> Option<Span> {
        self.root
    }
    pub fn has_normalizer(&self) -> bool {
        self.normalizer.is_some()
    }
    pub fn is_root(&self) -> bool {
        self.root.is_some()
    }
}
