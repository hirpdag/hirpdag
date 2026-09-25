// Metadata flags set with `#[hirpdag(flags = ...)]`: each node's own bits come
// from the named function, and a node's flags are its own bits ORed with every
// child's, so a flag answers "does anything below here have this property"
// without a traversal.

use hirpdag::base::HirpdagMetaFlagType;
use hirpdag::*;

const HAS_VAR: HirpdagMetaFlagType = 1 << 0;
const HAS_ADD: HirpdagMetaFlagType = 1 << 1;
const HAS_NEGATIVE: HirpdagMetaFlagType = 1 << 2;

#[hirpdag_module]
mod datamodel {
    use super::*;

    #[hirpdag(flags = var_flags)]
    pub struct Var {
        pub name: String,
    }

    fn var_flags(_: &HirpdagStructVar) -> HirpdagMetaFlagType {
        HAS_VAR
    }

    #[hirpdag(flags = super::constant_flags)]
    pub struct Constant {
        pub value: i32,
    }

    #[hirpdag(flags = add_flags)]
    pub struct Add {
        pub lhs: Expr,
        pub rhs: Expr,
    }

    fn add_flags(_: &HirpdagStructAdd) -> HirpdagMetaFlagType {
        HAS_ADD
    }

    #[hirpdag]
    pub enum Expr {
        Var(Var),
        Constant(Constant),
        Add(Add),
    }

    #[hirpdag(flags = tagged_flags)]
    pub enum Tagged {
        Plain(Expr),
        Negated(Expr),
    }

    fn tagged_flags(t: &Tagged) -> HirpdagMetaFlagType {
        match t {
            Tagged::Negated(_) => HAS_NEGATIVE,
            Tagged::Plain(_) => 0,
        }
    }

    #[hirpdag]
    pub struct Statement {
        pub body: Tagged,
    }
}

// A path outside the module works too, and the function may read the data.
fn constant_flags(c: &datamodel::HirpdagStructConstant) -> HirpdagMetaFlagType {
    if c.value < 0 {
        HAS_NEGATIVE
    } else {
        0
    }
}

use datamodel::*;

fn var(name: &str) -> Expr {
    Expr::Var(Var::new(name.to_string()))
}

fn constant(value: i32) -> Expr {
    Expr::Constant(Constant::new(value))
}

fn add(lhs: Expr, rhs: Expr) -> Expr {
    Expr::Add(Add::new(lhs, rhs))
}

fn flags_of(e: &Expr) -> HirpdagMetaFlagType {
    match e {
        Expr::Var(v) => v.hirpdag_get_meta().get_flags(),
        Expr::Constant(c) => c.hirpdag_get_meta().get_flags(),
        Expr::Add(a) => a.hirpdag_get_meta().get_flags(),
    }
}

#[test]
fn a_node_carries_the_flags_its_function_returns() {
    assert_eq!(flags_of(&var("x")), HAS_VAR);
    assert_eq!(flags_of(&constant(3)), 0);
    assert_eq!(flags_of(&constant(-3)), HAS_NEGATIVE);
}

#[test]
fn a_node_carries_the_flags_of_everything_below_it() {
    let e = add(constant(1), add(var("x"), constant(-2)));
    assert_eq!(flags_of(&e), HAS_ADD | HAS_VAR | HAS_NEGATIVE);

    let closed = add(constant(1), constant(2));
    assert_eq!(flags_of(&closed), HAS_ADD);
}

#[test]
fn flags_do_not_change_count_or_height() {
    let e = add(var("x"), constant(-2));
    let meta = match &e {
        Expr::Add(a) => a.hirpdag_get_meta().clone(),
        _ => unreachable!(),
    };
    assert_eq!((meta.get_count(), meta.get_height()), (3, 2));
}

#[test]
fn an_enum_adds_its_own_flags_to_its_payload() {
    let plain = Statement::new(Tagged::Plain(var("x")));
    assert_eq!(plain.hirpdag_get_meta().get_flags(), HAS_VAR);

    let negated = Statement::new(Tagged::Negated(var("x")));
    assert_eq!(
        negated.hirpdag_get_meta().get_flags(),
        HAS_VAR | HAS_NEGATIVE
    );
}
