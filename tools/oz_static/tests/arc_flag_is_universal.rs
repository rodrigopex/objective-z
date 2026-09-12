// SPDX-License-Identifier: Apache-2.0
//
// arc_flag_is_universal.rs - every Clang path in this project passes
// `-fobjc-arc`, asserted rather than listed (#443).
//
// `docs/STATUS.md` states the invariant and names the six files that carry
// it, and #428/#436 rest their whole case on it: a send of `retain`,
// `release`, `autorelease`, `dealloc` or `retainCount` is refused *because*
// Clang would refuse it anyway, so the eleven rejections in
// `static_bar_rejects.rs` are enforcing a rule the flag makes true. Drop the
// flag from one path and those tests keep passing while the property they
// describe is gone.
//
// The project has a check for this already -- `scripts/objz_check_compile_db.py`
// lists `-fobjc-arc` in `REQUIRED_FLAGS` -- and it reads a built
// `compile_commands.json`, so it can only run in a job that configures a
// Zephyr build. That is `hw-build-check`, which is not one of `main`'s eight
// required contexts, so the one check proving ARC is switched on could go red
// without blocking a merge. This test needs no build, so it runs in
// `rust-tests`, which is required. The two are complementary: that script
// checks what a real configure *emitted*, this checks what the sources
// *say*.
//
// `include_str!` rather than a runtime read, deliberately. It resolves at
// compile time relative to this file, so renaming or deleting one of these
// six files fails the build with a message naming the path -- a stronger
// failure than an assertion, and one that cannot be skipped.

/// The flag itself. Spelled once here so a typo cannot make every assertion
/// below vacuously true.
const ARC: &str = "-fobjc-arc";

/// Each path, with the sources spliced in at compile time.
///
/// Kept in the order `docs/STATUS.md` lists them, so a reader comparing the
/// two can do it line by line.
const CLANG_PATHS: &[(&str, &str)] = &[
    ("cmake/oz_static.cmake", include_str!("../../../cmake/oz_static.cmake")),
    ("cmake/ObjcClang.cmake", include_str!("../../../cmake/ObjcClang.cmake")),
    ("tests/tools/compile_and_run.py", include_str!("../../../tests/tools/compile_and_run.py")),
    ("tools/oz_static/tests/common/mod.rs", include_str!("common/mod.rs")),
    ("scripts/regen_zephyr_tests.py", include_str!("../../../scripts/regen_zephyr_tests.py")),
    (
        "scripts/objz_check_compile_db.py",
        include_str!("../../../scripts/objz_check_compile_db.py"),
    ),
];

/// Every path names the flag.
#[test]
fn every_clang_path_passes_fobjc_arc() {
    let missing: Vec<&str> =
        CLANG_PATHS.iter().filter(|(_, src)| !src.contains(ARC)).map(|(p, _)| *p).collect();
    assert!(
        missing.is_empty(),
        "{} no longer passes '{}'. ARC being unconditional is what #428 and #436 rest on -- \
         the rejections in static_bar_rejects.rs enforce a rule this flag makes true, so \
         dropping it from one path leaves those tests passing while the property is gone. \
         Either restore the flag, or change the ruling and this test with it.",
        missing.join(", "),
        ARC
    );
}

/// And the flag is required rather than merely mentioned, wherever a path has
/// somewhere to require it.
///
/// The distinction matters: `-fobjc-arc` appearing inside a comment
/// explaining why it is passed would satisfy the check above while the
/// invocation had lost it. Only `objz_check_compile_db.py` has a machine-
/// readable place to state the requirement, and this pins it there -- the
/// tuple is what makes a *configured build* answer for the flag, which is
/// the half this file cannot see.
#[test]
fn the_compile_db_check_requires_the_flag_rather_than_mentioning_it() {
    let src = include_str!("../../../scripts/objz_check_compile_db.py");
    let line = src
        .lines()
        .find(|l| l.trim_start().starts_with("REQUIRED_FLAGS"))
        .expect("objz_check_compile_db.py must define REQUIRED_FLAGS");
    assert!(
        line.contains(ARC),
        "REQUIRED_FLAGS must list '{}' -- it is what makes a configured build's \
         compile_commands.json answer for the flag, rather than the sources merely \
         mentioning it. Got: {}",
        ARC,
        line.trim()
    );
}
