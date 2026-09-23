// `hirpdag_reset_tables()`, generated per module under the `reset-tables`
// feature, empties the module's interning tables. The benchmarks rely on it to
// start each memory measurement cold; this checks it for every preset.
//
// Each preset gets its own module, and so its own tables, and this is the only
// test in each module, so resetting cannot disturb a test running in parallel.

#![cfg(feature = "reset-tables")]

#[macro_use]
mod support;

hirpdag_test_configs! {
    #[hirpdag]
    pub struct Node {
        pub value: i64,
        pub child: Option<Node>,
    }

    #[test]
    fn reset_forgets_interned_nodes() {
        let before = Node::new(1, None);
        let parent = Node::new(2, Some(before.clone()));
        assert_eq!(before, Node::new(1, None));

        hirpdag_reset_tables();

        // Interning after a reset starts from nothing: an equal value is a new
        // node, not the one interned before.
        let after = Node::new(1, None);
        assert_ne!(before, after, "the reset table still held the old node");
        assert_eq!(after, Node::new(1, None), "interning works after a reset");

        // Nodes interned before the reset stay valid.
        assert_eq!(parent.child, Some(before.clone()));
        assert_eq!(before.value, 1);
    }
}
