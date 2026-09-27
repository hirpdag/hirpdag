// Dropping a deep graph must not be limited by the stack.
//
// Freeing a node drops its data, which drops the child references in its
// fields, which frees each child whose last reference that was, and so on
// down the graph. Done by plain recursion, that is one level (several stack
// frames) per node: a chain about 8,000 nodes deep (debug) or 64,000 (release)
// overflowed a 2 MiB stack and aborted the process. Every child reference is
// now an `IterativeDrop`, which queues the frees below the first one and runs
// them in a loop (see `hirpdag_hashconsing::drop_queue`).
//
// Each preset gets the test, because the reference type decides how a free
// happens: `RefLeak` never frees, the strong concurrent tables keep every node
// alive themselves, and `RefTlc` defers the free to its next flush or to thread
// exit, where the drop test's thread ends.
//
// The chain is built on a thread with a large stack, then its only reference
// is moved to a thread with a small stack and dropped there. 64 KiB held about
// 2,000 levels of recursive drop in release, so 10,000 would overflow it in
// both profiles. (The `arc_arcswap` table copies itself on every insert, which
// is why the chain is not deeper.)

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
