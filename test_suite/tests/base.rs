use hirpdag::*;

#[hirpdag]
struct MessageA {
    a: i32,
    b: String,
    c: Option<MessageA>,
    d: i32,
}

#[hirpdag]
enum EnumB {
    Foo(i32),
    Bar(String),
    Baz(Option<MessageA>),
    Brr(String),
}

#[hirpdag]
pub struct MessageC {
    d: i32,
    e: EnumB,
}

#[hirpdag_end]
pub struct HirpdagEndMarker;

fn print_hash<T: std::hash::Hash>(t: &T) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    println!("{}", s.finish());
}

#[test]
fn foobar1() {
    println!("========");
    let a: MessageA = MessageA::new(32, "sup".to_string(), None, 1);
    let b: MessageA = MessageA::new(32, "dog".to_string(), Some(a.clone()), 1);
    let c: MessageA = MessageA::new(32, "sup".to_string(), None, 1);
    let d: MessageA = MessageA::new(32, "dog".to_string(), Some(c.clone()), 1);

    let h: MessageC = MessageC::new(32, EnumB::Brr("aaa".to_string()));
    println!("{:?}", h.e);
    let h2: MessageC = MessageC::new(32, EnumB::Brr("aaa".to_string()));
    println!("{:?}", h2.e);

    print_hash(&a);
    print_hash(&b);
    print_hash(&c);
    print_hash(&d);

    print_hash(&h);
    print_hash(&h2);

    assert_eq!(a, c);
    assert_eq!(b, d);
}

struct MessageAExtendLeaf {
    doot: MessageA,
}

impl MessageAExtendLeaf {
    fn new() -> HirpdagRewriteMemoized<Self> {
        let extension = MessageA::new(0, "DOOT".to_string(), None, 7007);
        HirpdagRewriteMemoized::new(Self { doot: extension })
    }
}

impl HirpdagRewriter for MessageAExtendLeaf {
    fn rewrite_MessageA(&self, x: &MessageA) -> MessageA {
        if x.c.is_none() {
            return MessageA::new(x.a, x.b.clone(), Some(self.doot.clone()), x.d);
        }

        // In the case where we don't want to make changes to extend the leaf,
        // we want to apply the default rewrite which will apply the rewrite
        // transitively to all applicable members.
        x.default_rewrite(self)
    }
}

#[test]
fn foobar3() {
    println!("========");
    let a: MessageA = MessageA::new(32, "sup".to_string(), None, 0);
    let b: MessageA = MessageA::new(32, "dog".to_string(), Some(a.clone()), 0);

    let t = MessageAExtendLeaf::new();
    eprintln!("a\n{:?}", a);
    let ta = t.rewrite(&a);
    eprintln!("t(a)\n{:?}", ta);

    let t = MessageAExtendLeaf::new();
    eprintln!("b\n{:?}", b);
    let tb = t.rewrite(&b);
    eprintln!("t(b)\n{:?}", tb);
}

#[test]
fn foobar4() {
    println!("========");
    let a: MessageA = MessageA::new(32, "sup".to_string(), None, 0);
    let b: MessageA = MessageA::new(32, "dog".to_string(), Some(a.clone()), 0);

    let c: MessageC = MessageC::new(4, EnumB::Baz(Some(b)));
    eprintln!("c\n{:?}", c);

    let t = MessageAExtendLeaf::new();
    let tc = t.rewrite(&c);
    eprintln!("t(c)\n{:?}", tc);
}
