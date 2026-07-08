# Hirpdag &emsp; [![Book Status]][book] [![Build Status]][actions] [![Latest Version]][crates.io]

[Book Status]: https://img.shields.io/github/actions/workflow/status/hirpdag/hirpdag/site.yml?branch=main&label=book
[book]: https://hirpdag.github.io/book
[Build Status]: https://img.shields.io/github/actions/workflow/status/hirpdag/hirpdag/ci.yml?branch=main
[actions]: https://github.com/hirpdag/hirpdag/actions?query=branch%3Amain
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

#[hirpdag_module]
mod expressions {
    #[hirpdag(normalizer)]
    struct Expr {
        // pub so a rewriter defined outside the module can read this field.
        pub x: ExprKind,
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

    fn nary_expr_normalize(...) { ... } // See full code in test suite.

    impl Expr {
        // Because we added the normalizer attribute on Expr, we implement Expr::new(...).
        // To produce a normalized Expr, this function will use Expr::spawn(...).
        pub fn new(x: ExprKind) -> Expr {
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
}
use expressions::*;

// A rewriter defined outside the hirpdag module, against the generated
// public API (the HirpdagRewriter trait, HirpdagRewriteMemoized,
// default_rewrite, and the pub `x` field).
struct Substitute {
    var: String,
    s: Expr,
}

impl Substitute {
    pub fn new(var: String, s: Expr) -> HirpdagRewriteMemoized<Self> {
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

## Builder API

Each `#[hirpdag]` struct gets a generated builder for ergonomic construction and
non-destructive modification of nodes.

```rust
use hirpdag::*;

#[hirpdag_module]
mod points {
    #[hirpdag]
    struct Point {
        x: i32,
        y: i32,
    }
}
use points::*;

// Construct a new node via the builder.
let p: Point = Point::builder()
    .x(1)
    .y(2)
    .build();

assert_eq!(p, Point::new(1, 2));

// Derive a modified copy with to_builder() — the original is unchanged.
let q: Point = p.to_builder().y(99).build();

assert_eq!(q, Point::new(1, 99));
assert_eq!(p, Point::new(1, 2)); // p is unmodified
```

Because hirpdag nodes are hash-consed, `build()` will return the existing
interned node if an identical one already exists, so no duplicate allocation
occurs.

## Serialization

Hirpdag serialization is always DAG-aware: each unique node is written exactly
once (as an entry in a topologically ordered node table), so structural sharing
survives a round trip and output size is proportional to the number of unique
nodes, not the tree expansion.

Struct types that may be serialization roots are marked `#[hirpdag(root)]`.
`#[hirpdag_module]` generates a `HirpdagArchiveRoots` struct (one vector per root
type; multiple roots of different types share one file) and the entry points
`hirpdag_serialize`/`hirpdag_deserialize` (compact binary via [postcard]) and
`hirpdag_serialize_json`/`hirpdag_deserialize_json` (text via [serde_json]).

[postcard]: https://crates.io/crates/postcard
[serde_json]: https://crates.io/crates/serde_json

```rust
#[hirpdag(root, normalizer)]
struct Expr {
    x: ExprKind,
}

// ...

let a: Expr = Expr::new(ExprKind::Var("a".to_string()));
let e: Expr = Expr::new(ExprKind::Mul(vec![a.clone(), a.clone()]));

let bytes: Vec<u8> = hirpdag_serialize(&HirpdagArchiveRoots {
    expr: vec![e.clone()],
    ..Default::default()
}).unwrap();

let out = hirpdag_deserialize(&bytes).unwrap();

// Deserialized nodes are re-interned through the hashcons table,
// so in-process round trips are pointer-equal.
assert_eq!(out.expr[0], e);
```

See `docs/design/serialization.md` and
`docs/adr/0001-serde-dag-aware-serialization.md` for the format and design
rationale.

## Benchmark Results

![Primes2000_p1](https://raw.github.com/hirpdag/hirpdag/main/docs/benchmark_results/primes2000_p1_violin.svg)
![Primes2000_p8](https://raw.github.com/hirpdag/hirpdag/main/docs/benchmark_results/primes2000_p8_violin.svg)
![Primes2000Same_p1](https://raw.github.com/hirpdag/hirpdag/main/docs/benchmark_results/primes2000same_p1_violin.svg)
![Primes2000Same_p8](https://raw.github.com/hirpdag/hirpdag/main/docs/benchmark_results/primes2000same_p8_violin.svg)

At [commit](https://github.com/hirpdag/hirpdag/commit/8907b237374497b55cd4896c6e7bd4e5d152f999) on `AMD Ryzen 9 3900X 12-Core Processor`

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
