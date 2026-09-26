// Rewriting a deep graph must not be limited by the stack.
//
// A rule recurses through the driver inside its own body
// (`x.default_rewrite(driver)` calls `driver.rewrite(&field)` for each field),
// so every level of depth keeps a rule's frames, and the driver's, alive until
// the level below returns. The memoized driver adds its cache lookup and
// closure to each level. About 2,000 levels (debug) or 18,000 (release)
// overflow a 2 MiB stack and abort the process with the memoized driver; the
// direct driver manages about 2.5 times as many. 64 KiB holds about 600
// levels of memoized rewrite in release, so 10,000 overflows it in both
// profiles.
//
// The chain is built, and the result checked and dropped, on a thread with a
// large stack (dropping a deep graph recurses too; see deep_drop.rs). Only the
// rewrite runs on the small stack. The input stays referenced by the large
// thread and the output is returned to it, so nothing deep is freed on the
// small stack.
//
// Known failures, ignored until fixed. A stack overflow aborts the whole test
// binary, so run each test on its own:
//   cargo test -p hirpdag_test_suite --all-features --test deep_rewrite -- --ignored --exact <name>

use hirpdag::*;

#[hirpdag_module]
mod datamodel {
    #[hirpdag]
    pub struct Chain {
        pub value: u32,
        pub next: Option<Chain>,
    }
}

use datamodel::*;

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

/// A chain of `DEPTH` nodes whose values are `first..first + DEPTH`, the
/// deepest holding `first`.
fn chain(first: u32) -> Chain {
    let mut chain = Chain::new(first, None);
    for value in first + 1..first + DEPTH {
        chain = Chain::new(value, Some(chain));
    }
    chain
}

/// Adds one to every value: every node changes, so the whole chain is rebuilt.
struct Increment;

impl HirpdagRewriter for Increment {
    fn rewrite_Chain<D: HirpdagRewriteDriver>(&self, x: &Chain, driver: &D) -> Chain {
        Chain::new(x.value + 1, driver.rewrite(&x.next))
    }
}

#[test]
#[ignore = "rewriting a deep graph overflows the stack; run with --ignored"]
fn direct_rewrite_of_a_deep_chain_on_a_small_stack() {
    on_stack("large", LARGE_STACK, || {
        let input = chain(0);
        let expected = chain(1);
        let to_rewrite = input.clone();
        let output = on_stack("direct rewrite on small stack", SMALL_STACK, move || {
            Increment.rewrite(&to_rewrite)
        });
        assert!(output == expected);
    });
}

#[test]
#[ignore = "rewriting a deep graph overflows the stack; run with --ignored"]
fn memoized_rewrite_of_a_deep_chain_on_a_small_stack() {
    on_stack("large", LARGE_STACK, || {
        let input = chain(0);
        let expected = chain(1);
        let to_rewrite = input.clone();
        let (output, memoizer) =
            on_stack("memoized rewrite on small stack", SMALL_STACK, move || {
                let memoizer = HirpdagRewriteMemoized::new(Increment);
                let output = memoizer.rewrite(&to_rewrite);
                // Returned, so its cache is dropped on the large stack.
                (output, memoizer)
            });
        assert!(output == expected);
        drop(memoizer);
    });
}
