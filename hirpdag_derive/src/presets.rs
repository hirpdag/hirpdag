#![forbid(unsafe_code)]

//! The roster of named hash-consing configuration presets.
//!
//! A preset names one (reference, table) pairing that a `#[hirpdag_module]`
//! can be built with. Everything the rest of the crate knows about presets is
//! in [`PRESETS`] below: which names exist, which types each selects, the
//! label a benchmark reports it under, and whether it needs the
//! `third-party-tables` feature.
//!
//! This is the only list. `hirpdag_for_each_preset!` (see `lib.rs`) drives the
//! test and benchmark matrices from it, so a preset added here is picked up by
//! every caller that asks for "all presets" rather than by six hand-edited
//! lists that a comment asks you to keep in step. See
//! `docs/adr/0007-preset-roster-driven-by-a-proc-macro.md`.
//!
//! The module identifier a stamped module gets is *not* stored: it is the
//! preset name, which is a valid identifier for every entry. The display
//! label is stored, because it is not derivable — six of the twelve names
//! break a snake-to-camel transform (`seppad`, `sepu32`, `tovweaktable`,
//! `dashmap`, `skipmap`, `arcswap`), the same non-injectivity that produced
//! `docs/adr/0006-generated-names-from-the-declared-name.md`.

use proc_macro2::TokenStream;

/// Preset used when no `preset`/type arguments are given.
pub const DEFAULT_PRESET: &str = "arc_hash_linear";

/// The inner `ThreadUnsafeTable` a lock-based preset stores its nodes in.
#[derive(Clone, Copy)]
pub enum PresetInner {
    /// A hashmap that falls back to the named table at larger sizes.
    HashmapFallback(&'static str),
    /// The named table, used directly.
    Direct(&'static str),
}

/// How a preset reaches its interned nodes.
#[derive(Clone, Copy)]
pub enum PresetTables {
    /// Lock-based: an inner `ThreadUnsafeTable` behind `TableSharedSharded`.
    Sharded {
        base: &'static str,
        inner: PresetInner,
    },
    /// A concurrent collection storing the mapping directly, with no inner
    /// table. These hold strong references (no weak-reference GC) and need a
    /// `Send + Sync` reference, which is why every one of them is wired to
    /// `RefArc`.
    Concurrent {
        base: &'static str,
        shared: &'static str,
    },
}

/// One named configuration preset.
pub struct Preset {
    /// The name written as `preset = "..."`, and the identifier of a module
    /// stamped out for this preset.
    pub name: &'static str,
    /// How a benchmark reports this preset, e.g. `ArcHashLinear`.
    pub label: &'static str,
    /// Whether selecting this preset requires the `third-party-tables`
    /// feature. Gated entries are emitted under a `#[cfg]` by
    /// `hirpdag_for_each_preset!`.
    pub gated: bool,
    pub tables: PresetTables,
}

/// Every known preset, in the order they are reported.
pub const PRESETS: &[Preset] = {
    use PresetInner::{Direct, HashmapFallback};
    use PresetTables::{Concurrent, Sharded};

    /// A lock-based preset: reference `base`, an inner table behind the
    /// sharded-mutex shared table.
    const fn sharded(
        name: &'static str,
        label: &'static str,
        base: &'static str,
        inner: PresetInner,
    ) -> Preset {
        Preset {
            name,
            label,
            gated: false,
            tables: Sharded { base, inner },
        }
    }

    /// A preset backed by a third-party concurrent collection. Always gated.
    const fn concurrent(name: &'static str, label: &'static str, shared: &'static str) -> Preset {
        Preset {
            name,
            label,
            gated: true,
            tables: Concurrent {
                base: "RefArc",
                shared,
            },
        }
    }

    &[
        sharded(
            "arc_hash_linear",
            "ArcHashLinear",
            "RefArc",
            HashmapFallback("TableVecLinearWeak"),
        ),
        sharded(
            "arc_hash_sorted",
            "ArcHashSorted",
            "RefArc",
            HashmapFallback("TableVecSortedWeak"),
        ),
        sharded(
            "leak_hash_linear",
            "LeakHashLinear",
            "RefLeak",
            HashmapFallback("TableVecLinearWeak"),
        ),
        // Reference-counting experiments with counts stored separately from
        // the data (see hirpdag_hashconsing::reference::sepcount).
        sharded(
            "sep_hash_linear",
            "SepHashLinear",
            "RefSep",
            HashmapFallback("TableVecLinearWeak"),
        ),
        sharded(
            "seppad_hash_linear",
            "SepPadHashLinear",
            "RefSepPad",
            HashmapFallback("TableVecLinearWeak"),
        ),
        sharded(
            "sepu32_hash_linear",
            "SepU32HashLinear",
            "RefSepU32",
            HashmapFallback("TableVecLinearWeak"),
        ),
        // Thread-local deferred reference counting (see
        // hirpdag_hashconsing::reference::tlc).
        sharded(
            "tlc_hash_linear",
            "TlcHashLinear",
            "RefTlc",
            HashmapFallback("TableVecLinearWeak"),
        ),
        // Backed by third-party collection crates, behind the
        // `third-party-tables` feature. `arc_tovweaktable` wraps the
        // weak-table crate's `WeakHashSet` as an inner table behind the
        // sharded shared table; the rest store the mapping directly in a
        // concurrent collection. See the `table::*_strong` /
        // `table::tov_weak_table_threadunsafe` modules.
        Preset {
            name: "arc_tovweaktable",
            label: "ArcTovWeakTable",
            gated: true,
            tables: Sharded {
                base: "RefArc",
                inner: Direct("TableTovWeakTable"),
            },
        },
        concurrent("arc_dashmap", "ArcDashMap", "DashMap"),
        concurrent("arc_flurry", "ArcFlurry", "Flurry"),
        concurrent("arc_skipmap", "ArcSkipMap", "SkipMap"),
        concurrent("arc_arcswap", "ArcArcSwap", "ArcSwap"),
    ]
};

/// The preset named `name`, or `None` if there is no such preset.
pub fn find(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}

/// Every preset name, for the unknown-preset error message.
pub fn names() -> Vec<&'static str> {
    PRESETS.iter().map(|p| p.name).collect()
}

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
pub struct ConfigTypes {
    pub reference_type: String,
    pub reference_weak_type: String,
    pub aliases: Vec<(String, String)>,
    pub tableshared_type: String,
}

impl ConfigTypes {
    /// Insert the helper alias `name` (or replace it if already present).
    pub fn set_alias(&mut self, name: &str, rhs: String) {
        match self.aliases.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = rhs,
            None => self.aliases.push((name.to_string(), rhs)),
        }
    }
}

impl Preset {
    /// The identifier of a module stamped out for this preset.
    pub fn module_ident(&self) -> proc_macro2::Ident {
        proc_macro2::Ident::new(self.name, proc_macro2::Span::call_site())
    }

    /// The type strings this preset selects.
    pub fn config_types(&self) -> ConfigTypes {
        let reference = |base: &str| {
            (
                format!("hirpdag::hirpdag_hashconsing::{base}<D>"),
                format!("hirpdag::hirpdag_hashconsing::{base}Weak<D>"),
            )
        };
        match self.tables {
            PresetTables::Sharded { base, inner } => {
                let inner_table = match inner {
                    PresetInner::HashmapFallback(table) => format!(
                        "hirpdag::hirpdag_hashconsing::TableHashmapFallbackWeak<D, ImplRef<D>, ImplRefWeak<D>, hirpdag::hirpdag_hashconsing::{table}<D, ImplRef<D>, ImplRefWeak<D>>>"
                    ),
                    PresetInner::Direct(table) => format!(
                        "hirpdag::hirpdag_hashconsing::{table}<D, ImplRef<D>, ImplRefWeak<D>>"
                    ),
                };
                let (reference_type, reference_weak_type) = reference(base);
                ConfigTypes {
                    reference_type,
                    reference_weak_type,
                    aliases: vec![("ImplTable".to_string(), inner_table)],
                    tableshared_type:
                        "hirpdag::hirpdag_hashconsing::TableSharedSharded<D, ImplRef<D>, ImplTable<D>>"
                            .to_string(),
                }
            }
            PresetTables::Concurrent { base, shared } => {
                let (reference_type, reference_weak_type) = reference(base);
                ConfigTypes {
                    reference_type,
                    reference_weak_type,
                    aliases: Vec::new(),
                    tableshared_type: format!(
                        "hirpdag::hirpdag_hashconsing::TableShared{shared}<D, ImplRef<D>>"
                    ),
                }
            }
        }
    }
}

/// Expands `callback` once per preset, passing the preset's module
/// identifier, name and label ahead of `payload`.
///
/// Gated presets are emitted under `#[cfg(feature = "third-party-tables")]`,
/// evaluated in the calling crate.
pub fn expand_for_each_preset(callback: &proc_macro2::Ident, payload: &TokenStream) -> TokenStream {
    let mut out = TokenStream::new();
    for preset in PRESETS {
        let module = preset.module_ident();
        let name = preset.name;
        let label = preset.label;
        let gate = if preset.gated {
            quote! { #[cfg(feature = "third-party-tables")] }
        } else {
            TokenStream::new()
        };
        out.extend(quote! {
            #gate
            #callback!(#module, #name, #label, #payload);
        });
    }
    out
}

/// Which slice of the roster [`expand_preset_names`] emits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresetSelector {
    /// Presets compiled unconditionally.
    Core,
    /// Presets needing the `third-party-tables` feature.
    ThirdParty,
    /// Every preset.
    All,
}

impl PresetSelector {
    /// The selector named `name`, or `None`.
    pub fn from_ident(name: &str) -> Option<Self> {
        match name {
            "core" => Some(Self::Core),
            "third_party" => Some(Self::ThirdParty),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    fn accepts(self, preset: &Preset) -> bool {
        match self {
            Self::Core => !preset.gated,
            Self::ThirdParty => preset.gated,
            Self::All => true,
        }
    }
}

/// Expands to a `&[&str]` of the selected presets' names.
///
/// The roster is a compile-time table, so a caller that needs the names as
/// runtime data — validating a preset named in an environment variable, say —
/// would otherwise have to write the list out again.
pub fn expand_preset_names(selector: PresetSelector) -> TokenStream {
    let names = PRESETS
        .iter()
        .filter(|p| selector.accepts(p))
        .map(|p| p.name);
    quote! { &[#(#names),*] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_name_is_a_valid_module_identifier() {
        for preset in PRESETS {
            // Ident::new panics on a name that is not an identifier; this is
            // what lets the roster omit a separate module ident per entry.
            assert_eq!(preset.module_ident().to_string(), preset.name);
        }
    }

    #[test]
    fn preset_names_are_unique() {
        let mut names = names();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "duplicate preset name in the roster");
    }

    #[test]
    fn the_default_preset_is_in_the_roster_and_is_not_gated() {
        let default = find(DEFAULT_PRESET).expect("default preset is known");
        assert!(
            !default.gated,
            "the default preset must not need an opt-in feature"
        );
    }

    /// Six of the twelve labels are not a snake-to-camel transform of the
    /// name, which is why the roster stores them rather than deriving them.
    #[test]
    fn labels_are_not_derivable_from_names() {
        fn camel(name: &str) -> String {
            name.split('_')
                .map(|word| {
                    let mut c = word.chars();
                    match c.next() {
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                })
                .collect()
        }
        let underivable: Vec<&str> = PRESETS
            .iter()
            .filter(|p| camel(p.name) != p.label)
            .map(|p| p.name)
            .collect();
        assert_eq!(
            underivable,
            vec![
                "seppad_hash_linear",
                "sepu32_hash_linear",
                "arc_tovweaktable",
                "arc_dashmap",
                "arc_skipmap",
                "arc_arcswap",
            ]
        );
    }

    #[test]
    fn concurrent_presets_are_gated_and_use_a_send_sync_reference() {
        for preset in PRESETS {
            if let PresetTables::Concurrent { base, .. } = preset.tables {
                assert!(preset.gated, "{} must be gated", preset.name);
                assert_eq!(
                    base, "RefArc",
                    "{} needs a Send + Sync reference",
                    preset.name
                );
            }
        }
    }

    #[test]
    fn config_types_name_the_aliases_the_expansion_declares() {
        for preset in PRESETS {
            let types = preset.config_types();
            let declares_inner = types.aliases.iter().any(|(n, _)| n == "ImplTable");
            match preset.tables {
                // A sharded preset is generic over an inner table, so it must
                // declare the alias its shared-table string refers to.
                PresetTables::Sharded { .. } => assert!(
                    declares_inner && types.tableshared_type.contains("ImplTable<D>"),
                    "{} must declare and use ImplTable",
                    preset.name
                ),
                // A concurrent preset stores the mapping itself.
                PresetTables::Concurrent { .. } => assert!(
                    !declares_inner && !types.tableshared_type.contains("ImplTable"),
                    "{} must not name an inner table",
                    preset.name
                ),
            }
        }
    }

    #[test]
    fn the_driver_emits_one_invocation_per_preset_and_gates_the_third_party_ones() {
        let callback = proc_macro2::Ident::new("cb", proc_macro2::Span::call_site());
        let payload = quote! { extra };
        let out = expand_for_each_preset(&callback, &payload).to_string();

        assert_eq!(out.matches("cb !").count(), PRESETS.len());
        let gated = PRESETS.iter().filter(|p| p.gated).count();
        assert_eq!(gated, 5);
        assert_eq!(out.matches("third-party-tables").count(), gated);

        for preset in PRESETS {
            assert!(
                out.contains(&format!("\"{}\"", preset.name)),
                "{} missing from the expansion",
                preset.name
            );
            assert!(
                out.contains(&format!("\"{}\"", preset.label)),
                "{} label missing from the expansion",
                preset.label
            );
        }
        assert_eq!(out.matches("extra").count(), PRESETS.len());
    }

    #[test]
    fn the_name_lists_partition_the_roster() {
        let core = expand_preset_names(PresetSelector::Core).to_string();
        let third_party = expand_preset_names(PresetSelector::ThirdParty).to_string();

        for preset in PRESETS {
            let quoted = format!("\"{}\"", preset.name);
            let in_core = core.contains(&quoted);
            let in_third_party = third_party.contains(&quoted);
            assert!(
                in_core != in_third_party,
                "{} must appear in exactly one list",
                preset.name
            );
            assert_eq!(
                in_third_party, preset.gated,
                "{} is in the wrong list",
                preset.name
            );
        }

        // The two together are every preset, which is what makes validating a
        // user-supplied preset name against them complete.
        let all = expand_preset_names(PresetSelector::All).to_string();
        for preset in PRESETS {
            assert!(all.contains(&format!("\"{}\"", preset.name)));
        }
    }

    #[test]
    fn selectors_are_named_as_written_in_the_macro() {
        assert_eq!(
            PresetSelector::from_ident("core"),
            Some(PresetSelector::Core)
        );
        assert_eq!(
            PresetSelector::from_ident("third_party"),
            Some(PresetSelector::ThirdParty)
        );
        assert_eq!(PresetSelector::from_ident("all"), Some(PresetSelector::All));
        assert_eq!(PresetSelector::from_ident("nonsense"), None);
    }
}
