#![forbid(unsafe_code)]

use proc_macro2::{Ident, TokenStream};

use crate::presets::{self, ConfigTypes, DEFAULT_PRESET};

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

    /// Named preset selecting the reference and table types together.
    Preset(String),
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
            "preset" => Handler::String(|s: &syn::LitStr| {
                let name = s.value();
                if presets::find(&name).is_none() {
                    return Err(syn::Error::new(
                        s.span(),
                        format!(
                            "unknown preset `{}`; known presets: {}",
                            name,
                            presets::names().join(", ")
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
            types: presets::find(DEFAULT_PRESET)
                .expect("default preset is known")
                .config_types(),
        }
    }

    pub fn from(args: &HirpdagArgs) -> Self {
        let mut config = Self::default();
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
                HirpdagArg::Preset(name) => {
                    config.types = presets::find(name)
                        .expect("preset validated at parse time")
                        .config_types();
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
}
