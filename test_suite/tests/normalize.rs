use hirpdag::*;

#[hirpdag_module]
mod datamodel {
    #[hirpdag(normalizer)]
    struct EvenNumber {
        // pub so a rewriter defined outside the module can read this field.
        pub a: u32,
    }

    #[hirpdag]
    struct Holder {
        x: EvenNumber,
    }

    impl EvenNumber {
        pub fn new(a: u32) -> EvenNumber {
            // Mask to subtract 1 from odd numbers.
            EvenNumber::spawn(a & !1)
        }
    }
}

use datamodel::*;

// A rewriter defined outside the hirpdag module, against the generated
// public API (the HirpdagRewriter trait, HirpdagRewriteMemoized, and the
// pub `a` field).
struct AddN {
    n: u32,
}

impl AddN {
    pub fn new(x: u32) -> HirpdagRewriteMemoized<Self> {
        HirpdagRewriteMemoized::new(Self { n: x })
    }
}

impl HirpdagRewriter for AddN {
    fn rewrite_EvenNumber(&self, x: &EvenNumber) -> EvenNumber {
        EvenNumber::new(x.a + self.n)
    }
}

#[test]
fn round_down_test() {
    println!("========");
    let a: EvenNumber = EvenNumber::new(2);
    let b: EvenNumber = EvenNumber::new(3);
    let c: EvenNumber = EvenNumber::new(4);

    assert_eq!(a, b);
    assert_ne!(b, c);
}

#[test]
fn rewrite_round_down_test() {
    println!("========");
    let a: EvenNumber = EvenNumber::new(2);
    let b: EvenNumber = EvenNumber::new(3);
    let c: EvenNumber = EvenNumber::new(4);

    assert_eq!(a, b);
    assert_ne!(b, c);

    let x: Holder = Holder::new(a);
    let y: Holder = Holder::new(b);
    let z: Holder = Holder::new(c);

    assert_eq!(x, y);
    assert_ne!(y, z);

    let add3 = AddN::new(3);
    let x3 = add3.rewrite(&x);

    assert_eq!(x3, z);
}
