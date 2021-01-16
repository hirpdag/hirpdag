# Hirpdag &emsp; [![Latest Version]][crates.io]

[Latest Version]: https://img.shields.io/crates/v/hirpdag.svg
[crates.io]: https://crates.io/crates/hirpdag

Hirpdag is a library and procedural macro for creating data structures which are:

-   **H**ash Consed
-   **I**mmutable
-   **R**eference Counted
-   **P**ersistent
-   **D**irected **A**cyclic **G**raph

Hirpdag generates the data structure specific boilerplate code to implement these features,
and code for performing DAG rewriting on Hirpdag objects.

Hirpdag supports different hashconsing implementations and includes a
benchmarking suite for evaluating performance of different hashconsing implementations.

## Example

```rust
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

fn nary_expr_normalize(...) { ... } // See full code in test suite.

impl Expr {
    // Because we added the normalizer attribute on Expr, we implement Expr::new(...).
    // To produce a normalized Expr, this function will use Expr::spawn(...).
    fn new(x: ExprKind) -> Expr {
        // Normalization
        match x {
            ExprKind::Mul(original_factors) => {
                return nary_expr_normalize(original_factors,
                    1, // Identity when combining constants
                    |a, b| a * b, // Combine constants
                    |x| match x { ExprKind::Mul(e) => Some(&e), _ => None }, // Flatten nested Mul
                    |e| Expr::spawn(ExprKind::Mul(e))); // Spawn as Mul
            }
            ExprKind::Add(original_terms) => {
                return nary_expr_normalize(original_terms,
                    0, // Identity when combining constants
                    |a, b| a + b, // Combine constants
                    |x| match x { ExprKind::Add(e) => Some(&e), _ => None }, // Flatten nested Add
                    |e| Expr::spawn(ExprKind::Add(e))); // Spawn as Add
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
```

## Building Documentation

You can build the book locally with:

```
$ cargo install mdbook
$ mdbook build book
```

## License

Licensed under either of [MIT License][licensemit] or [Apache License 2.0][licenseapache] at your option.

[licensemit]: LICENSE-MIT
[licenseapache]: LICENSE-APACHE

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
