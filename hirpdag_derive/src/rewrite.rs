#![forbid(unsafe_code)]

//! Rewriting: `default_rewrite` and `HirpdagRewritable` for each data type,
//! and for the module the `HirpdagRewriter` rules trait, the
//! `HirpdagRewriteDriver` trait, the direct and memoized drivers, and the
//! `HirpdagMemoizeCache` the memoized driver keeps its results in.

use proc_macro2::{Ident, Span};

use crate::names::DataTypeNames;
use crate::DataTypeEntry;

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
    //let body = quote! {
    //    let hirpdag_rw_a = driver.rewrite(&self.a);
    //    let hirpdag_rw_b = driver.rewrite(&self.b);
    //    let hirpdag_rw_c = driver.rewrite(&self.c);
    //    if hirpdag_rw_a == self.a && hirpdag_rw_b == self.b && hirpdag_rw_c == self.c {
    //        self.clone()
    //    } else {
    //        Self::new(hirpdag_rw_a, hirpdag_rw_b, hirpdag_rw_c)
    //    }
    //};
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
    // shadow the parameters used to rewrite the remaining fields. A raw
    // identifier (`r#type`) loses its `r#`: the prefixed name is not a
    // keyword, and `hirpdag_rw_r#type` is not an identifier at all.
    let locals: Vec<syn::Ident> = field_names
        .iter()
        .map(|field_name| {
            use syn::ext::IdentExt;
            Ident::new(
                &format!("hirpdag_rw_{}", field_name.unraw()),
                Span::call_site(),
            )
        })
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

fn get_variants_rewrite(input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    //let variants_rewrite = quote! {
    //    Foo(x) => Foo(driver.rewrite(&x)),
    //    Bar(x) => Bar(driver.rewrite(&x)),
    //    Baz(x) => Baz(driver.rewrite(&x)),
    //};
    input_enum
        .variants
        .iter()
        .map(|t| {
            let variant = &t.ident;
            quote! { #variant(x) => #variant(driver.rewrite(&x)), }
        })
        .collect()
}

/// One method of the user-facing `HirpdagRewriter` trait: the rewrite rule for
/// a single data type.
///
/// The rule is handed the node and the recursion driver. The default
/// implementation passes both to `default_rewrite`, which recurses into the
/// node's children through the driver.
fn get_rewrite_datatype(names: &DataTypeNames) -> proc_macro2::TokenStream {
    //let rewrite_datatype = quote! {
    //    #[allow(non_snake_case)]
    //    fn rewrite_MessageA<D: HirpdagRewriteDriver>(&self, x: &MessageA, driver: &D) -> MessageA {
    //        MessageA::default_rewrite(x, driver)
    //    }
    //};
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
/// data type. Drivers implement the traversal strategy (plain recursion,
/// memoized recursion, ...) and are the only path recursion takes.
fn get_driver_datatype(names: &DataTypeNames) -> proc_macro2::TokenStream {
    //let driver_datatype = quote! {
    //    #[allow(non_snake_case)]
    //    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA;
    //};
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
    //let direct_rewrite = quote! {
    //    #[allow(non_snake_case)]
    //    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA {
    //        self.rewriter.rewrite_MessageA(x, self)
    //    }
    //};
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

fn get_cache_member(names: &DataTypeNames) -> proc_macro2::TokenStream {
    //let cache_member = quote! {
    //    cache_MessageA: hirpdag::base::HirpdagMemoizeMap<MessageA, MessageA>,
    //};
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

fn get_cache_member_new(names: &DataTypeNames) -> proc_macro2::TokenStream {
    //let cache_member_new = quote! {
    //    cache_MessageA: hirpdag::base::HirpdagMemoizeMap::new(),
    //};
    let hirpdag_cache_member_name = &names.cache_member;

    quote! {
        #hirpdag_cache_member_name: hirpdag::base::HirpdagMemoizeMap::new(),
    }
}

fn get_cache_clear(names: &DataTypeNames) -> proc_macro2::TokenStream {
    //let cache_clear = quote! {
    //    self.cache_MessageA.clear();
    //};
    let hirpdag_cache_member_name = &names.cache_member;

    quote! {
        self.#hirpdag_cache_member_name.clear();
    }
}

/// The `HirpdagMemoize` impl that points the cache's per-type API at one type's
/// table, so `cache.get_or_else(&node, || ..)` resolves to the right map.
fn get_cache_memoize_impl(names: &DataTypeNames) -> proc_macro2::TokenStream {
    //let cache_memoize_impl = quote! {
    //    impl hirpdag::base::HirpdagMemoize<MessageA> for HirpdagMemoizeCache {
    //        fn hirpdag_memoize_map(&self)
    //        -> &hirpdag::base::HirpdagMemoizeMap<MessageA, MessageA> {
    //            &self.cache_MessageA
    //        }
    //    }
    //};
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
    //let cache_rewrite = quote! {
    //    #[allow(non_snake_case)]
    //    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA {
    //        self.memoize_cache.get_or_else(x, || {
    //            self.rewriter.rewrite_MessageA(x, self)
    //        })
    //    }
    //};
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

pub fn for_struct(
    names: &DataTypeNames,
    fields_named: &syn::FieldsNamed,
) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: hirpdag_ref_name,
        rewrite_method: hirpdag_rewrite_method_name,
        ..
    } = names;
    let default_rewrite_body = get_default_rewrite_body(fields_named);

    quote! {
        impl #hirpdag_ref_name {
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
        }

        impl<D: HirpdagRewriteDriver> HirpdagRewritable<D> for #hirpdag_ref_name {
            fn hirpdag_rewrite(&self, driver: &D) -> Self {
                driver.#hirpdag_rewrite_method_name(self)
            }
        }
    }
}

pub fn for_enum(names: &DataTypeNames, input_enum: &syn::DataEnum) -> proc_macro2::TokenStream {
    let DataTypeNames {
        ref_name: name,
        rewrite_method: hirpdag_rewrite_method_name,
        ..
    } = names;
    let variants_rewrite = get_variants_rewrite(input_enum);

    quote! {
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
    }
}

/// The rewriting traits and drivers, and the memoization cache, with one
/// method, cache table or impl per data type in the module.
pub fn for_module(types: &[DataTypeEntry]) -> proc_macro2::TokenStream {
    // One piece per data type, in declaration order.
    let each = |piece: fn(&DataTypeNames) -> proc_macro2::TokenStream| -> proc_macro2::TokenStream {
        types.iter().map(|entry| piece(&entry.names)).collect()
    };
    let rewrite_methods = each(get_rewrite_datatype);
    let driver_methods = each(get_driver_datatype);
    let direct_methods = each(get_direct_rewrite);
    let cache_members = each(get_cache_member);
    let cache_members_new = each(get_cache_member_new);
    let cache_clears = each(get_cache_clear);
    let cache_memoize_impls = each(get_cache_memoize_impl);
    let cache_methods = each(get_cache_rewrite);

    quote! {
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
    }
}
