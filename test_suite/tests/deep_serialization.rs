// Serializing a deep graph must not be limited by the stack. The collect
// phase used to recurse once per level of depth, and overflowed the stack on
// chains a few thousand nodes deep.
//
// The chain is serialized on a thread with a small stack, deep enough that the
// old recursive walk overflows it in debug and release builds. Building a
// chain does not recurse, but dropping one still does (a separate, known
// limitation), so the chain is built, checked and dropped on a thread with a
// large stack.

use hirpdag::*;

#[hirpdag_module]
mod datamodel {
    #[hirpdag(root)]
    pub struct Chain {
        pub value: u32,
        pub next: Option<Chain>,
    }
}

use datamodel::*;

const DEPTH: u32 = 100_000;
const SMALL_STACK: usize = 256 * 1024;
const LARGE_STACK: usize = 512 * 1024 * 1024;

fn on_stack<T: Send + 'static>(size: usize, f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(size)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

#[test]
fn a_deep_chain_round_trips_on_a_small_stack() {
    on_stack(LARGE_STACK, || {
        let mut chain = Chain::new(0, None);
        for value in 1..DEPTH {
            chain = Chain::new(value, Some(chain));
        }
        let roots = HirpdagArchiveRoots {
            roots_Chain: vec![chain],
        };

        let to_serialize = roots.clone();
        let (bytes, json) = on_stack(SMALL_STACK, move || {
            let bytes = hirpdag_serialize(&to_serialize).unwrap();
            let json = hirpdag_serialize_json(&to_serialize).unwrap();
            // Dropping the clone releases one reference per root, not the chain.
            (bytes, json)
        });

        // Deserializing re-interns every node, landing on the chain still held
        // by `roots`.
        assert!(hirpdag_deserialize(&bytes).unwrap() == roots);
        assert!(hirpdag_deserialize_json(&json).unwrap() == roots);
    });
}
