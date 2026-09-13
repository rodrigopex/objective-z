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
// name goes to stdout through `oz_platform_print` and the assertion text to
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

mod common;
use common::{compile_and_run, expect_trap, ozobject_src as PREAMBLE};

/// A program with one class, and a hand-declared ABI so the test can drive
/// the refcount. `extra` is spliced into `main`.
///
/// **`w` carries one release that is not in `extra`.** `[Widget alloc]` is
/// `+1` and `w` is a strong local, so ARC releases it at the end of the
/// scope -- after the `printf("survived")` below. Every count here is
/// therefore one higher than the explicit calls suggest, which matters for
/// *where* a trap fires as much as whether:
///
/// | explicit | total | flagged outcome |
/// |---|---|---|
/// | 0 | 1 | correct; runs to completion |
/// | 1 | 2 | the second release is ARC's, so the trap fires *after* "survived" |
/// | 2 | 3 | the trap fires at the second explicit call, before "survived" |
///
/// This was found the hard way: the control below used to pass with one
/// explicit release because it never defined the flag, so the trap it was
/// controlling for was not compiled in.
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

/// Releasing an object whose refcount is already 0 aborts instead of
/// returning as though nothing happened.
///
/// It does *not* name the class, and the assertion below pins that rather
/// than wishing otherwise -- the comment there records why poisoning cannot
/// change it, measured on both allocators.
///
/// Two releases: the first takes it 1 -> 0 and deallocates, the second is the
/// over-release. Without the flag this is silent -- `oz_atomic_dec_and_test`
/// is a fetch-sub compared against 1, so the second leaves -1 and returns.
#[test]
fn an_over_release_aborts_rather_than_returning_silently() {
    let src = program(
        "\
	oz_release((struct OZObject *)w);
	oz_release((struct OZObject *)w);
",
    );
    let out = expect_trap(&src, "trap_over_release", &["-DOZ_DEBUG_REFCOUNT"]);
    assert!(
        out.contains("over-release"),
        "the trap must say what the fault was; got:\n{}",
        out
    );
    /* **The class prints as `?`, and poisoning did not change that.** This
     * assertion was written expecting to flip to "freed" once `_oz_free`
     * stamped `OZ_CLASS_ID_FREED`. The stamp is now emitted -- see
     * `poison_emission.rs` -- and this still reads `?`, measured rather
     * than assumed. The reason is the one thing the plan did not account
     * for: **the allocator owns the block the moment it is handed back, and
     * writes its own bookkeeping into it.**
     *
     *   * Zephyr's `k_mem_slab_free` links the block into its free list by
     *     writing the next pointer *into the block*: `*(char **) mem =
     *     slab->free_list` (`kernel/mem_slab.c`). `_meta` is the root
     *     struct's first member, so `class_id` is at offset 0 -- exactly
     *     what that pointer lands on.
     *   * The host's `oz_slab_free` is `free()`, and malloc is worse than
     *     that: probed on arm64 macOS, an object whose ivar was
     *     `0x11111111` and whose body poison was `0xA5` read back
     *     `0x00000003` after the free, so *neither* the stamp nor the body
     *     poison survives here. On the host that is fine -- ASan is the
     *     stronger instrument and the corpora already run under it.
     *
     * So the `?` is not a shortfall waiting on a feature. **On a true
     * double free the class is not knowable from the object**, and the only
     * designs that would change that are the ones #452 rejected: a
     * quarantine (a slot held back is a slot the next allocation cannot
     * have, and `pools.rs` counts one slot per site) or a new root field at
     * an offset the free-list link misses (four bytes on every object, and
     * an ABI that differs between flagged and unflagged builds).
     *
     * What poisoning does buy is on target, where the body past the root
     * prefix is untouched by the free-list write -- so a use-after-free
     * that reads an *ivar* gets `0xA5A5A5A5` instead of plausible stale
     * data. That is not observable from this harness, which is host-only;
     * it needs the Zephyr backend. */
    assert!(
        out.contains("over-release of ?"),
        "the class is not knowable from a freed object -- the allocator overwrites `_meta` \
         when it takes the block back, so the stamp is gone by the time the trap reads it; \
         got:\n{}",
        out
    );
    assert!(
        !out.contains("survived"),
        "it must abort at the second release, not carry on to the end of main; got:\n{}",
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
///     it: see the table on `program`.
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

/// And the same program, unflagged, survives the double release.
///
/// This is the behaviour the old `double_release_guard` case asserted, and it
/// is worth keeping as a row rather than deleting: turning the instruments on
/// must not be the only way the program is *safe*, only the way the fault is
/// *visible*. The guard still absorbs the second release; the trap is what
/// tells you it happened.
#[test]
fn without_the_flag_a_double_release_is_survived_silently() {
    let src = program(
        "\
	oz_release((struct OZObject *)w);
	oz_release((struct OZObject *)w);
",
    );
    let out = compile_and_run(&src, "trap_double_release_unflagged");
    assert_eq!(
        out, "alive\nsurvived\n",
        "unflagged, the second release returns quietly -- which is exactly the silence \
         #452 exists to break; got:\n{}",
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
/// as a GNU extension at *every* expansion of `oz_platform_print` -- not
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
