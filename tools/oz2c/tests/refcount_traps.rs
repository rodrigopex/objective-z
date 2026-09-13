// SPDX-License-Identifier: Apache-2.0
//
// refcount_traps.rs - the traps under `OZ_DEBUG_REFCOUNT` actually
// fire, and do not fire on a correct program (#452).
//
// **The tree had no way to assert that a trap fires.** Every run helper
// asserts `status.success()`, and a firing trap aborts, so the existing
// exhaustion-trap test compiles with the macro and deliberately does not run
// -- which proves the trap is real C and says nothing about whether it ever
// triggers. `common::expect_trap` is the missing half: compile with the flag,
// run, require a non-zero exit, and hand back stdout and stderr together.
//
// Both streams, because a trap's evidence is split across them. The class
// name goes to stdout through `OZ_PLATFORM_PRINT` and the assertion text to
// stderr through `oz_assert_msg`, and that split is not decoration:
// `oz_assert_msg` takes a plain `const char *` with no format arguments
// (`include/platform/oz_assert.h`), so naming the class cannot happen inside
// the assert.
//
// The releases here go through the **C API**, calling `oz_release`
// directly, because that is the only spelling available: ARC owns
// `-release` and a send of it is a hard located error (#428). It is the same
// reason `samples/smp_shared` declares the ABI by hand (#437) -- there is no
// Objective-C way to drive a refcount.
//
// **Why the over-release is staged on a live object rather than by
// releasing a freed one.** The obvious fixture is two releases: the first
// deallocates, the second is the over-release. It is not a test, because
// reaching the trap means reading `_meta` and `oz_refcount` out of storage
// the allocator has taken back, and what it finds there is the allocator's
// business. Measured, the same fixture disagrees across hosts:
//
//   * arm64 macOS -- the trap fires, and the class prints as `?`, because
//     malloc left something <= 0 in the refcount word and a `class_id`
//     matching no class.
//   * Linux/glibc in CI -- the program **runs to completion and exits 0**.
//     glibc writes a tcache `next` pointer over the first word, `_meta` is
//     the root struct's first member, and the pointer's bit 12 is
//     `_meta.immortal` -- so `oz_release` returns at the immortal check
//     *above* the trap and the over-release is never seen.
//
// That is not a trap that sometimes misses; it is a test whose subject is
// undefined behaviour. So these tests drive the refcount to 0 on an object
// that is still alive -- `oz_atomic_init(&w->base.oz_refcount, 0)`, the
// same reach into the root struct `behavior_edge.rs` already makes -- and
// then release it. That is exactly the trap's condition ("released at
// refcount 0"), it is fully defined, it fires identically on every
// platform, and because the object is live `oz_class_name` can read a real
// `class_id` and **name the class**.
//
// The freed case is left untested deliberately. `poison_emission.rs`
// explains what poisoning can and cannot recover there.

mod common;
use common::{compile_and_run, expect_trap, ozobject_src as PREAMBLE};

/// A program with one class, and a hand-declared ABI so the test can drive
/// the refcount. `extra` is spliced into `main`.
///
/// **`w` carries one release that is not in `extra`.** `[Widget alloc]` is
/// `+1` and `w` is a strong local, so ARC releases it at the end of the
/// scope -- after the `printf("survived")` below. Any explicit release in
/// `extra` is therefore *additional*, which is why a balanced program here
/// releases nothing explicitly.
///
/// That cost a wrong test: the control below used to pass with one explicit
/// release, because it never defined the flag and so the trap it was
/// controlling for was not compiled in. With the flag on, one explicit
/// release plus ARC's is an over-release, and the trap fires after
/// "survived".
fn program(extra: &str) -> String {
    format!(
        "{}{}{}{}",
        PREAMBLE(),
        "\
#include <stdio.h>

@interface Widget : OZObject {
	int _v;
}
@end
@implementation Widget
@end

/* The companion's own signatures, declared by hand for the same reason
 * samples/smp_shared does it: ARC owns -release, so there is no Objective-C
 * spelling that drives a refcount. A differing signature here would be a
 * conflicting redeclaration in the generated C. */
struct OZObject;
void oz_release(struct OZObject *self);

int main(void)
{
	Widget *w = [Widget alloc];

	printf(\"alive\\n\");
",
        extra,
        "\
	printf(\"survived\\n\");
	return 0;
}
"
    )
}

/// Releasing an object whose refcount is already 0 aborts, and names the
/// class while doing it.
///
/// The refcount is driven to 0 on a **live** object, so the trap's
/// condition is met with nothing freed -- see the note at the top of this
/// file for why the freed variant is not a test. Two things follow that the
/// freed variant could not show:
///
///   * it fires on every platform, because nothing here reads storage the
///     allocator owns;
///   * `oz_class_name` reads a real `class_id` and prints **`Widget`**,
///     which is what the print-then-assert shape exists for. The class is
///     unknowable only *after* a free.
#[test]
fn an_over_release_aborts_and_names_the_class() {
    let src = program(
        "\
	oz_atomic_init(&w->base.oz_refcount, 0);
	oz_release((struct OZObject *)w);
",
    );
    let out = expect_trap(&src, "trap_over_release", &["-DOZ_DEBUG_REFCOUNT"]);
    assert!(
        out.contains("over-release"),
        "the trap must say what the fault was; got:\n{}",
        out
    );
    assert!(
        out.contains("over-release of Widget"),
        "a live over-release must name the class -- that is the whole reason the trap prints \
         before it asserts, `oz_assert_msg` taking no format arguments; got:\n{}",
        out
    );
    assert!(
        !out.contains("survived"),
        "it must abort at the release, not carry on to the end of main; got:\n{}",
        out
    );
}

/// The control: a correctly balanced program, **with the flag on**, runs to
/// completion.
///
/// Without this the test above would pass just as well if the trap fired on
/// every release, or on program exit, or unconditionally -- and a trap that
/// always fires is worse than none, because it makes the instrument
/// unusable rather than merely absent.
///
/// Two things about it were wrong when it was written, and both made it
/// controls for nothing:
///
///   * **It used `compile_and_run`, which defines no macros**, so
///     `OZ_DEBUG_REFCOUNT` was off and the trap it claimed to control for
///     was not in the binary. Its doc comment said "the flag still on". It
///     now goes through `compile_and_run_with_cc_flags` and passes the flag.
///   * **It performed an explicit release**, which with ARC's scope-end
///     release for `w` is *two* -- a real over-release. Turning the flag on
///     made the trap fire, correctly, after "survived". The balanced
///     program is the one that releases nothing explicitly and lets ARC do
///     it, which is what `program`'s comment now spells out.
#[test]
fn a_correct_release_does_not_trip_the_trap() {
    let src = program("");
    let out = common::compile_and_run_with_cc_flags(
        &src,
        "trap_correct_release",
        &["-DOZ_DEBUG_REFCOUNT"],
    );
    assert_eq!(
        out, "alive\nsurvived\n",
        "a balanced program must be silent with the instruments on; got:\n{}",
        out
    );
}

/// The same over-release, unflagged, is survived in silence.
///
/// This is the behaviour the Unity `double_release_guard` case was named
/// for, and it is worth a row rather than deleting: turning the instruments
/// on must not be the only thing making the program *survive*, only the
/// thing making the fault *visible*.
///
/// Deterministic for the same reason as the test above -- the object is
/// live. `oz_atomic_dec_and_test` is `atomic_fetch_sub(t, 1) == 1`, so a
/// release at 0 leaves -1 and reports false, and the ARC release at scope
/// end takes it to -2 and reports false again. Both return quietly, which
/// is exactly the silence #452 exists to break.
#[test]
fn without_the_flag_an_over_release_is_survived_silently() {
    let src = program(
        "\
	oz_atomic_init(&w->base.oz_refcount, 0);
	oz_release((struct OZObject *)w);
",
    );
    let out = compile_and_run(&src, "trap_over_release_unflagged");
    assert_eq!(
        out, "alive\nsurvived\n",
        "unflagged, a release at refcount 0 returns quietly; got:\n{}",
        out
    );
}

/// The flagged C is ISO C17 with no constraint violation.
///
/// **This is the only gate that sees it.** `just test-pedantic` sweeps the
/// generated C on target, and it does not define `OZ_DEBUG_REFCOUNT` -- so
/// every line the instruments add is behind an `#ifdef` that sweep never
/// enters. CLAUDE.md states the hazard in the general case ("code behind an
/// `#ifdef` the sweep does not define can hide a violation indefinitely");
/// this is that case, and the whole of #452's new C falls inside it.
///
/// A balanced program, so it runs to completion: the claim here is about
/// the compiler, and a firing trap would abort before proving anything
/// about it.
///
/// This test is what found the PAL's `, ##__VA_ARGS__`, which clang rejects
/// as a GNU extension at *every* expansion of `OZ_PLATFORM_PRINT` -- not
/// only a zero-argument one. Nothing had expanded that macro before the
/// traps did, so an unreachable non-conformance became a real one the
/// moment #452's C called it.
#[test]
fn the_flagged_c_is_pedantic_iso_c17() {
    let src = program("");
    let out = common::compile_and_run_with_cc_flags(
        &src,
        "trap_pedantic_c17",
        &["-DOZ_DEBUG_REFCOUNT", "-std=c17", "-pedantic-errors"],
    );
    assert_eq!(
        out, "alive\nsurvived\n",
        "the instrumented program must still behave, not merely compile; got:\n{}",
        out
    );
}
