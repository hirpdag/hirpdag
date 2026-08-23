// Test that hirpdag_module can be applied to multiple modules in the same
// compile unit, each with independent generated code.
//
// Also test that generated code that needs to be public, is public: the
// rewriters are defined *outside* their modules, so each module's
// HirpdagRewriter trait, HirpdagRewriteCache, default_rewrite, and the
// data fields the rewriter touches must all be reachable from outside.

#[hirpdag::hirpdag_module]
mod foo {
    #[hirpdag]
    pub struct Data {
        pub a: i32,
        pub b: String,
        pub c: Option<Data>,
        pub d: i32,
    }
}

#[hirpdag::hirpdag_module]
mod bar {
    #[hirpdag]
    pub struct Data {
        pub a: i32,
        pub b: String,
        pub c: Option<Data>,
        pub d: i32,
    }
}

// Rewriter for `foo`, defined outside the module.
struct FooExtendLeaf {
    doot: foo::Data,
    cache: foo::HirpdagRewriteCache,
}

impl FooExtendLeaf {
    fn new() -> Self {
        let extension = foo::Data::new(0, "DOOT".to_string(), None, 7007);
        Self {
            doot: extension,
            cache: foo::HirpdagRewriteCache::new(),
        }
    }
}

impl foo::HirpdagRewriter for FooExtendLeaf {
    fn rewrite_Data(&self, x: &foo::Data) -> foo::Data {
        if x.c.is_none() {
            return foo::Data::new(x.a, x.b.clone(), Some(self.doot.clone()), x.d);
        }

        // In the case where we don't want to make changes to extend the leaf,
        // we want to apply the default rewrite which will apply the rewrite
        // transitively to all applicable members.
        x.default_rewrite(self)
    }

    fn memoized_rewrite_cache(&self) -> Option<&foo::HirpdagRewriteCache> {
        Some(&self.cache)
    }
}

// Rewriter for `bar`, defined outside the module.
struct BarExtendLeaf {
    doot: bar::Data,
    cache: bar::HirpdagRewriteCache,
}

impl BarExtendLeaf {
    fn new() -> Self {
        let extension = bar::Data::new(0, "DOOT".to_string(), None, 7007);
        Self {
            doot: extension,
            cache: bar::HirpdagRewriteCache::new(),
        }
    }
}

impl bar::HirpdagRewriter for BarExtendLeaf {
    fn rewrite_Data(&self, x: &bar::Data) -> bar::Data {
        if x.c.is_none() {
            return bar::Data::new(x.a, x.b.clone(), Some(self.doot.clone()), x.d);
        }

        x.default_rewrite(self)
    }

    fn memoized_rewrite_cache(&self) -> Option<&bar::HirpdagRewriteCache> {
        Some(&self.cache)
    }
}

#[test]
fn foa_bar_test() {
    println!("========");
    let a: foo::Data = foo::Data::new(32, "sup".to_string(), None, 1);
    let b: foo::Data = foo::Data::new(32, "dog".to_string(), Some(a.clone()), 1);
    let c: bar::Data = bar::Data::new(32, "sup".to_string(), None, 1);
    let d: bar::Data = bar::Data::new(32, "dog".to_string(), Some(c.clone()), 1);

    eprintln!("a\n{:?}", b);
    let t_b = {
        use crate::foo::HirpdagRewriter;
        let foo_t = FooExtendLeaf::new();
        foo_t.rewrite(&b)
    };
    eprintln!("t(b)\n{:?}", t_b);

    eprintln!("d\n{:?}", d);
    let t_d = {
        use crate::bar::HirpdagRewriter;
        let bar_t = BarExtendLeaf::new();
        bar_t.rewrite(&d)
    };
    eprintln!("t(d)\n{:?}", t_d);
}
