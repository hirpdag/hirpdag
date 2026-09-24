// The field types a `#[hirpdag]` type can hold, through everything the
// generated code does with a field: interning, metadata, rewriting and both
// archive formats.
//
// Leaves and containers are unit tested in hirpdag::base::field against
// stand-ins; this checks them inside real nodes, and in particular that nodes
// nested in containers (a tuple in a `Vec`, an `Option` in a tuple) are still
// counted, rewritten and archived. See `hirpdag::base::field` for the rules.

use hirpdag::base::HirpdagComputeMeta;
use hirpdag::*;

#[hirpdag_module]
mod datamodel {
    /// A leaf type defined by the user: one line makes it a field type.
    #[derive(
        Clone,
        Copy,
        Debug,
        Hash,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        hirpdag::serde::Serialize,
        hirpdag::serde::Deserialize,
    )]
    #[serde(crate = "hirpdag::serde")]
    pub enum Colour {
        Red,
        Green,
    }

    impl hirpdag::base::HirpdagLeaf for Colour {}

    #[hirpdag(root)]
    pub struct Leaves {
        pub flag: bool,
        pub letter: char,
        pub small: i8,
        pub wide: u128,
        pub size: usize,
        pub text: String,
        pub unit: (),
        pub colour: Colour,
    }

    #[hirpdag(root)]
    pub struct Tree {
        pub name: String,
        pub children: Vec<(Colour, Tree)>,
        pub extra: (bool, Option<Tree>),
        pub triple: Option<(u8, char, Vec<Tree>)>,
    }

    #[hirpdag]
    pub enum Payload {
        Flag(bool),
        Pair((char, Tree)),
    }

    #[hirpdag(root)]
    pub struct Holder {
        pub payload: Payload,
    }
}

use datamodel::*;

fn leaves() -> Leaves {
    Leaves::new(
        true,
        'λ',
        -3,
        u128::MAX,
        7,
        "text".to_string(),
        (),
        Colour::Green,
    )
}

fn leaf(name: &str) -> Tree {
    Tree::new(name.to_string(), vec![], (false, None), None)
}

/// A tree whose nodes are reached through every container: a tuple in a
/// `Vec`, an `Option` in a tuple, and a `Vec` in a tuple in an `Option`.
fn tree() -> Tree {
    let shared = leaf("shared");
    Tree::new(
        "root".to_string(),
        vec![(Colour::Red, shared.clone()), (Colour::Green, leaf("b"))],
        (true, Some(leaf("c"))),
        Some((1, 'x', vec![shared, leaf("d")])),
    )
}

#[test]
fn leaves_intern() {
    assert_eq!(leaves(), leaves());
    assert_ne!(leaves(), leaves().to_builder().flag(false).build());
    assert_ne!(leaves(), leaves().to_builder().colour(Colour::Red).build());
}

#[test]
fn leaves_contribute_no_metadata() {
    let meta = leaves().hirpdag_compute_meta();
    assert_eq!((meta.get_count(), meta.get_height()), (1, 1));
}

#[test]
fn nodes_in_containers_count_towards_metadata() {
    // root + shared, b, c, shared (again), d: counted once per reference.
    let meta = tree().hirpdag_compute_meta();
    assert_eq!((meta.get_count(), meta.get_height()), (6, 2));
}

/// Renames every leaf tree, wherever it sits.
struct RenameLeaves;

impl HirpdagRewriter for RenameLeaves {
    fn rewrite_Tree<D: HirpdagRewriteDriver>(&self, x: &Tree, driver: &D) -> Tree {
        if x.children.is_empty() && x.extra.1.is_none() && x.triple.is_none() {
            return leaf(&format!("{}'", x.name));
        }
        x.default_rewrite(driver)
    }
}

#[test]
fn rewriting_reaches_nodes_in_containers() {
    let shared = leaf("shared'");
    let expected = Tree::new(
        "root".to_string(),
        vec![(Colour::Red, shared.clone()), (Colour::Green, leaf("b'"))],
        (true, Some(leaf("c'"))),
        Some((1, 'x', vec![shared, leaf("d'")])),
    );
    assert_eq!(RenameLeaves.rewrite(&tree()), expected);
    assert_eq!(
        HirpdagRewriteMemoized::new(RenameLeaves).rewrite(&tree()),
        expected
    );
}

#[test]
fn rewriting_reaches_nodes_in_enum_payloads() {
    let holder = Holder::new(Payload::Pair(('p', leaf("e"))));
    let expected = Holder::new(Payload::Pair(('p', leaf("e'"))));
    assert_eq!(RenameLeaves.rewrite(&holder), expected);

    let flag = Holder::new(Payload::Flag(true));
    assert_eq!(RenameLeaves.rewrite(&flag), flag);
}

fn roots() -> HirpdagArchiveRoots {
    HirpdagArchiveRoots {
        roots_Leaves: vec![leaves()],
        roots_Tree: vec![tree()],
        roots_Holder: vec![
            Holder::new(Payload::Pair(('p', leaf("e")))),
            Holder::new(Payload::Flag(false)),
        ],
    }
}

#[test]
fn binary_round_trip() {
    let bytes = hirpdag_serialize(&roots()).unwrap();
    assert_eq!(hirpdag_deserialize(&bytes).unwrap(), roots());
}

#[test]
fn json_round_trip() {
    let text = hirpdag_serialize_json(&roots()).unwrap();
    assert_eq!(hirpdag_deserialize_json(&text).unwrap(), roots());
}

#[test]
fn a_node_shared_through_containers_is_archived_once() {
    let roots = HirpdagArchiveRoots {
        roots_Tree: vec![tree()],
        ..Default::default()
    };
    let text = hirpdag_serialize_json(&roots).unwrap();
    assert_eq!(text.matches("\"shared\"").count(), 1, "{}", text);
}
