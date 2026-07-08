// Test that hirpdag_module can be applied to multiple modules in the same
// compile unit, each with independent generated code.
//
// Also test that generated code that needs to be public, is public.

#[hirpdag::hirpdag_module]
mod foo {
    #[hirpdag]
    pub struct Data {
        a: i32,
        b: String,
        c: Option<Data>,
        d: i32,
    }

    pub struct ExtendLeaf {
        doot: Data,
    }

    impl ExtendLeaf {
        pub fn new() -> HirpdagRewriteMemoized<Self> {
            let extension = Data::new(0, "DOOT".to_string(), None, 7007);
            HirpdagRewriteMemoized::new(Self { doot: extension })
        }
    }

    impl HirpdagRewriter for ExtendLeaf {
        fn rewrite_Data(&self, x: &Data) -> Data {
            if x.c.is_none() {
                return Data::new(x.a, x.b.clone(), Some(self.doot.clone()), x.d);
            }

            // In the case where we don't want to make changes to extend the leaf,
            // we want to apply the default rewrite which will apply the rewrite
            // transitively to all applicable members.
            x.default_rewrite(self)
        }
    }
}

#[hirpdag::hirpdag_module]
mod bar {
    #[hirpdag]
    pub struct Data {
        a: i32,
        b: String,
        c: Option<Data>,
        d: i32,
    }

    pub struct ExtendLeaf {
        doot: Data,
    }

    impl ExtendLeaf {
        pub fn new() -> HirpdagRewriteMemoized<Self> {
            let extension = Data::new(0, "DOOT".to_string(), None, 7007);
            HirpdagRewriteMemoized::new(Self { doot: extension })
        }
    }

    impl HirpdagRewriter for ExtendLeaf {
        fn rewrite_Data(&self, x: &Data) -> Data {
            if x.c.is_none() {
                return Data::new(x.a, x.b.clone(), Some(self.doot.clone()), x.d);
            }

            // In the case where we don't want to make changes to extend the leaf,
            // we want to apply the default rewrite which will apply the rewrite
            // transitively to all applicable members.
            x.default_rewrite(self)
        }
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
        let foo_t = foo::ExtendLeaf::new();
        foo_t.rewrite(&b)
    };
    eprintln!("t(b)\n{:?}", t_b);

    eprintln!("d\n{:?}", d);
    let t_d = {
        use crate::bar::HirpdagRewriter;
        let bar_t = bar::ExtendLeaf::new();
        bar_t.rewrite(&d)
    };
    eprintln!("t(d)\n{:?}", t_d);
}
