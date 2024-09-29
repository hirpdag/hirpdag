#![forbid(unsafe_code)]

use proc_macro2::{Ident, TokenStream};

pub enum HirpdagArg {
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

    pub fn from(args: &HirpdagArgs) -> Self {
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

    pub fn has_normalizer(&self) -> bool {
        self.normalizer
    }
    pub fn reference_type(&self) -> TokenStream {
        self.reference_type.parse().unwrap()
    }
    pub fn reference_weak_type(&self) -> TokenStream {
        self.reference_weak_type.parse().unwrap()
    }
    pub fn table_type(&self) -> TokenStream {
        self.table_type.parse().unwrap()
    }
    pub fn tableshared_type(&self) -> TokenStream {
        self.tableshared_type.parse().unwrap()
    }
    pub fn build_tableshared_type(&self) -> TokenStream {
        self.build_tableshared_type.parse().unwrap()
    }
}
