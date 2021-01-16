use hirpdag::*;

#[hirpdag(normalizer)]
struct Expr {
    x: ExprKind,
}

#[hirpdag]
enum ExprKind {
    Num(u32),
    Add(Vec<Expr>),
    Mul(Vec<Expr>),
    Var(String),
}

#[hirpdag]
struct Variables {
    x: Expr,
}

#[hirpdag_end]
pub struct HirpdagEndMarker;

fn nary_expr_normalize<CombineFn, FlatternElementsFn, CreateFn>(
    mut original_elements: Vec<Expr>,
    identity: u32,
    combine: CombineFn,
    flattern: FlatternElementsFn,
    create: CreateFn,
) -> Expr
where
    CombineFn: std::ops::Fn(u32, u32) -> u32,
    FlatternElementsFn: std::ops::Fn(&ExprKind) -> Option<&Vec<Expr>>,
    CreateFn: std::ops::FnOnce(Vec<Expr>) -> Expr,
{
    let mut elements: Vec<Expr> = Vec::with_capacity(original_elements.len());
    let mut combined = 1;
    let mut process = |e: Expr| {
        match &e.x {
            ExprKind::Num(n) => {
                // - Combine constant Num
                //   Mul([Num(2), Num(3), Num(4), Var(a)]) => Mul([Num(24), Var(a)])
                //   Add([Num(2), Num(3), Num(4), Num(a)]) => Add([Num(9), Var(a)])
                combined = combine(combined, *n);
            }
            _ => {
                elements.push(e);
            }
        }
    };
    for e in original_elements.drain(..) {
        // - Flattern nested:
        //   Mul([Num(2), Mul([Var(a), Var(b)]), Var(c)]) => Mul([Num(2), Var(a), Var(b), Var(c)])
        //   Add([Num(2), Add([Var(a), Var(b)]), Var(c)]) => Add([Num(2), Var(a), Var(b), Var(c)])
        if let Some(inner_elements) = flattern(&e.x) {
            for i in inner_elements.iter() {
                process(i.clone()); // Unecessary clone here in the case of Num to make code shorter.
            }
        } else {
            process(e);
        }
    }
    if combined != identity {
        elements.push(Expr::spawn(ExprKind::Num(combined)));
    }
    // - Yield single element:
    //   Mul([Var(a)]) => Var(a)
    //   Add([Var(a)]) => Var(a)
    if elements.len() == 1 {
        return elements.pop().unwrap();
    }
    // - Sort:
    //   Mul([Var(c), Var(b), Var(a)]) => Mul([Var(a), Var(b), Var(c)])
    //   Add([Var(c), Var(b), Var(a)]) => Add([Var(a), Var(b), Var(c)])
    elements.sort();
    return create(elements);
}

impl Expr {
    // Because we added the normalizer attribute on Expr, we implement Expr::new(...).
    // To produce a normalized Expr, this function will use Expr::spawn(...).
    fn new(x: ExprKind) -> Expr {
        // Normalization
        match x {
            ExprKind::Mul(original_factors) => {
                return nary_expr_normalize(
                    original_factors,
                    1,            // Identity when combining constants
                    |a, b| a * b, // Combine constants
                    |x| match x {
                        ExprKind::Mul(e) => Some(&e),
                        _ => None,
                    }, // Flatten nested Mul
                    |e| Expr::spawn(ExprKind::Mul(e)), // Spawn as Mul
                );
            }
            ExprKind::Add(original_terms) => {
                return nary_expr_normalize(
                    original_terms,
                    0,            // Identity when combining constants
                    |a, b| a + b, // Combine constants
                    |x| match x {
                        ExprKind::Add(e) => Some(&e),
                        _ => None,
                    }, // Flatten nested Add
                    |e| Expr::spawn(ExprKind::Add(e)), // Spawn as Add
                );
            }
            _ => Expr::spawn(x),
        }
    }
}

#[test]
fn expr_normalizer_test() {
    let n2: Expr = Expr::new(ExprKind::Num(2));
    let n3: Expr = Expr::new(ExprKind::Num(3));
    let n6: Expr = Expr::new(ExprKind::Num(6));
    assert_ne!(n2, n3);
    assert_ne!(n2, n6);
    assert_ne!(n3, n6);

    let n2n3: Expr = Expr::new(ExprKind::Mul(vec![n2.clone(), n3.clone()]));

    // 2 * 3 == 6
    assert_eq!(n2n3, n6);

    let va: Expr = Expr::new(ExprKind::Var("a".to_string()));

    let n2n3va: Expr = Expr::new(ExprKind::Mul(vec![n2, n3, va.clone()]));
    let n6va: Expr = Expr::new(ExprKind::Mul(vec![n6, va]));

    // 2 * 3 * a == 6 * a
    // Note that this uses pointer equality, not a deep tree comparison.
    assert_eq!(n2n3va, n6va);
}

struct Substitute {
    var: String,
    s: Expr,
}

impl Substitute {
    fn new(var: String, s: Expr) -> HirpdagRewriteMemoized<Self> {
        HirpdagRewriteMemoized::new(Self { var: var, s: s })
    }
}

impl HirpdagRewriter for Substitute {
    fn rewrite_Expr(&self, x: &Expr) -> Expr {
        if let ExprKind::Var(name) = &x.x {
            if *name == self.var {
                return self.s.clone();
            }
        }
        x.default_rewrite(self)
    }
}

#[test]
fn expr_substitute_test() {
    let n2: Expr = Expr::new(ExprKind::Num(2));
    let n3: Expr = Expr::new(ExprKind::Num(3));
    let n6: Expr = Expr::new(ExprKind::Num(6));

    let va: Expr = Expr::new(ExprKind::Var("a".to_string()));
    let vb: Expr = Expr::new(ExprKind::Var("b".to_string()));

    let vavb: Expr = Expr::new(ExprKind::Mul(vec![va.clone(), vb.clone()]));
    let n2vb: Expr = Expr::new(ExprKind::Mul(vec![n2.clone(), vb.clone()]));

    let sub_va_n2 = Substitute::new("a".to_string(), n2);
    let vavb_va_n2 = sub_va_n2.rewrite(&vavb);
    assert_eq!(vavb_va_n2, n2vb);

    let sub_vb_n3 = Substitute::new("b".to_string(), n3);
    let n2vb_vb_n3 = sub_vb_n3.rewrite(&n2vb);
    assert_eq!(n2vb_vb_n3, n6);
}
