// SPDX-License-Identifier: Apache-2.0
//
// scratch_isolation.rs -- a test's scratch directory belongs to one
// checkout and one run, and is taken away again when the run ends (#343).
//
// The bug this covers is not in the transpiler: it is in the suite. Every
// scratch path was built from a fixed name under `$TMPDIR`
// (`oz_static_corpus_compile`, `oz_static_fptr_probe.c`,
// `oz_static_test_<stem>`), so it did not depend on the checkout -- and
// each site cleared the directory on entry. Two `cargo test` runs in two
// worktrees therefore destroyed each other's output mid-run:
//
//     error/slab_reuse_after_free.m: .../slab_reuse_after_free.c:3:10:
//       fatal error: 'slab_reuse_after_free.h' file not found
//
// A generated header, missing because the neighbour deleted the directory
// between this run writing it and the compiler opening it. This is the
// same collision #315 fixed for twister, where two sweeps shared
// `/tmp/twister-out` and `-c` made it fatal rather than merely wasteful;
// the justfile's `outdir` has been keyed on the checkout ever since, and
// the Rust suite was simply never given the same treatment.
//
// It also fails in the *other* direction, which is worse than a missing
// file: a run whose directory is recreated by a neighbour can pass on the
// neighbour's output.
//
// So what is asserted here is the property, not a symptom -- two
// checkouts cannot land on one name, two runs of one checkout cannot
// either, and the directory does not outlive the run that made it. A
// reproduction would have to be a second checkout and a race, which is
// neither hermetic nor quick.

mod common;
use common::{checkout_key, checkout_key_of, test_scratch_dir, ScratchDir};

/// The path a checkout's scratch name is derived from is its *own*, so two
/// checkouts differ. This is the whole fix, and the only part of it a
/// single-checkout test can reach directly -- hence `checkout_key_of`
/// taking the directory rather than reading `CARGO_MANIFEST_DIR` itself.
#[test]
fn two_checkouts_get_different_keys() {
    let primary = "/Users/dev/objective-z/tools/oz_static";
    let worktree = "/tmp/wt343/tools/oz_static";
    assert_ne!(
        checkout_key_of(primary),
        checkout_key_of(worktree),
        "two checkouts hashed to one key, so their scratch directories would be shared"
    );

    /* Stable, not merely different: the name has to be the same on every
     * run of one checkout, or `test_scratch_dir` would leak a directory
     * per run the way the pid-keyed `ScratchDir` deliberately does not. */
    assert_eq!(checkout_key_of(primary), checkout_key_of(primary));

    let key = checkout_key();
    assert_eq!(key.len(), 6, "expected six hex digits, got {:?}", key);
    assert!(key.chars().all(|c| c.is_ascii_hexdigit()), "not a path-safe key: {:?}", key);
}

/// A `ScratchDir` names this checkout *and* this process, so it is also
/// isolated from a second `cargo test` in this same checkout -- which the
/// checkout key alone does not cover.
#[test]
fn a_scratch_path_names_this_checkout_and_this_run() {
    let scratch = ScratchDir::new("isolation_probe");
    let name = scratch.path().file_name().unwrap().to_str().unwrap().to_string();

    assert!(name.contains(&checkout_key()), "{} does not name this checkout", name);
    assert!(
        name.contains(&std::process::id().to_string()),
        "{} does not name this run",
        name
    );
    assert_ne!(
        scratch.path(),
        std::env::temp_dir().join("oz_static_isolation_probe").as_path(),
        "still the old checkout-independent name"
    );
    assert!(scratch.path().is_dir(), "the directory was not created");
}

/// The guard takes the directory with it. Without this a pid-keyed path
/// would be strictly worse than the fixed name it replaces: a fresh
/// directory left in `$TMPDIR` on every single run.
#[test]
fn the_directory_is_gone_once_the_guard_drops() {
    let path = {
        let scratch = ScratchDir::new("isolation_drop");
        std::fs::write(scratch.join("out.c"), "int main(void) { return 0; }\n").unwrap();
        assert!(scratch.join("out.c").is_file());
        scratch.path().to_path_buf()
    };
    assert!(!path.exists(), "{} outlived its guard", path.display());
}

/// And it takes it while a panic unwinds, which is the case that matters:
/// a failing assertion is exactly when a scratch directory is largest and
/// least likely to be cleaned up by hand.
#[test]
fn a_panic_still_takes_the_directory_with_it() {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(None));
    let sink = std::sync::Arc::clone(&recorded);

    /* The hook is silenced only so a deliberate panic does not print a
     * backtrace into a passing run's output. */
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(move || {
        let scratch = ScratchDir::new("isolation_panic");
        *sink.lock().unwrap() = Some(scratch.path().to_path_buf());
        panic!("a case failing inside its scratch directory");
    });
    std::panic::set_hook(previous);

    assert!(outcome.is_err(), "the panic did not propagate, so nothing was unwound");
    let path = recorded.lock().unwrap().clone().expect("the guard never recorded its path");
    assert!(!path.exists(), "{} survived the unwind", path.display());
}

/// `compile_and_run`'s directory is keyed on the checkout too. It keeps a
/// stable name across runs on purpose -- see `test_scratch_dir` -- so the
/// assertion is about the checkout, not the run.
#[test]
fn compile_and_run_scratch_is_per_checkout() {
    let dir = test_scratch_dir("some_case");
    let name = dir.file_name().unwrap().to_str().unwrap().to_string();

    assert!(name.starts_with("oz_static_test_some_case_"), "unexpected name {}", name);
    assert!(name.ends_with(&checkout_key()), "{} does not name this checkout", name);
    assert_ne!(
        dir,
        std::env::temp_dir().join("oz_static_test_some_case"),
        "still the old checkout-independent name"
    );
    /* Spelled out rather than probed for the absence of a pid: five
     * decimal digits can occur inside six hex ones, so a `!contains`
     * would fail on one run in a few thousand for no reason at all. */
    assert_eq!(
        name,
        format!("oz_static_test_some_case_{}", checkout_key()),
        "keyed on more than the checkout, which would leak a directory per run"
    );
}
