// Dropping a deep graph must not be limited by the stack.
//
// Freeing a node drops its data, which drops the child references in its
// fields, which frees each child whose last reference that was, and so on
// down the graph: one level of recursion (several stack frames) per node.
// Dropping a chain about 8,000 nodes deep (debug) or 64,000 (release)
// overflows a 2 MiB stack and aborts the process.
//
// Each preset gets the test, because the reference type decides how a free
// happens: `RefLeak` never frees, the strong concurrent tables keep every node
// alive themselves, and `RefTlc` frees a chain one level per flush of its
// deferred decrements, then all at once, recursively, when the thread exits.
//
// The chain is built on a thread with a large stack (building does not
// recurse), then its only reference is moved to a thread with a small stack
// and dropped there. 64 KiB holds about 2,000 levels in release, so 10,000
// overflows it in both profiles. (The `arc_arcswap` table copies itself on
// every insert, which is why the chain is not deeper.)
//
// Known failures, ignored until fixed. A stack overflow aborts the whole test
// binary, so run each test on its own:
//   cargo test -p hirpdag_test_suite --all-features --test deep_drop -- --ignored --exact <name>

#[macro_use]
mod support;

const DEPTH: u32 = 10_000;
const SMALL_STACK: usize = 64 * 1024;
const LARGE_STACK: usize = 512 * 1024 * 1024;

fn on_stack<T: Send + 'static>(
    name: &str,
    size: usize,
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    std::thread::Builder::new()
        .name(name.to_string())
        .stack_size(size)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

hirpdag_test_configs! {
    #[hirpdag]
    pub struct Chain {
        pub value: u32,
        pub next: Option<Chain>,
    }

    #[test]
    #[ignore = "dropping a deep graph overflows the stack; run with --ignored"]
    fn dropping_a_deep_chain_on_a_small_stack() {
        let chain = super::on_stack("build", super::LARGE_STACK, || {
            let mut chain = Chain::new(0, None);
            for value in 1..super::DEPTH {
                chain = Chain::new(value, Some(chain));
            }
            chain
        });
        let name = format!("drop on small stack: {}", module_path!());
        super::on_stack(&name, super::SMALL_STACK, move || {
            // Dropped at the end of this block, on the small stack.
            let _chain = chain;
        });
    }
}
