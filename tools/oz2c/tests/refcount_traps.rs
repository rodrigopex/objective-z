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
struct OZObject *oz_retain(struct OZObject *self);

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

/* ------------------------------------------------------------------------
 * The freed-slot sentinel (#490)
 *
 * `_oz_free` stamps `OZ_REFCOUNT_FREED` into `oz_refcount`, and these four
 * cases pin the trap that reads it back. They are staged on a **live**
 * object, for the same reason the over-release above is: a genuinely freed
 * object is storage the allocator owns, and what a read finds there is its
 * business. The claim these make is about the *trap* -- its condition, its
 * message and its position -- which is fully defined on every platform.
 *
 * The other half of the claim, that the sentinel is still there to be read
 * after a real `k_mem_slab_free`, is a fact about the allocator rather than
 * about oz2c, and no host harness can establish it -- on arm64 macOS malloc
 * wipes the whole block. It is measured on target by
 * `tests/zephyr/src/test_freed_slot.c`, which asserts the survival and the
 * layout that guarantees it. Neither test is sufficient alone: this one
 * would pass on a target where the marker never survives, and that one
 * would pass if the trap read the marker and did nothing with it.
 * ---------------------------------------------------------------------- */

/// Releasing a slot carrying the freed sentinel aborts, and names the slot
/// by address.
///
/// **By address rather than by class, and that is not a shortcut.** After a
/// real free the class is genuinely unrecoverable: `class_id` is at offset 0
/// and the allocator's free-list link is written over it -- measured, it
/// reads back as the link's low bits (588 on mps2/an385, 376 on
/// qemu_cortex_a53) and `oz_class_name` answers `?`. The address is the one
/// thing still true of the object, and it identifies the slot, which is
/// what a slab debug session works from.
#[test]
fn releasing_a_slot_carrying_the_freed_sentinel_aborts() {
    let src = program(
        "\
	oz_atomic_init(&w->base.oz_refcount, OZ_REFCOUNT_FREED);
	oz_release((struct OZObject *)w);
",
    );
    let out = expect_trap(&src, "trap_release_after_free", &["-DOZ_DEBUG_REFCOUNT"]);
    assert!(
        out.contains("release of a freed object"),
        "the trap must distinguish a use-after-free from an over-release -- they are \
         different faults with different fixes; got:\n{}",
        out
    );
    assert!(
        out.contains("release after free"),
        "the assertion text is the half that survives on stderr when the print's stream \
         is discarded, so it must carry the fault too; got:\n{}",
        out
    );
    assert!(
        !out.contains("over-release"),
        "a freed slot must not be reported as an over-release; the sentinel exists \
         precisely so the two are told apart; got:\n{}",
        out
    );
    assert!(
        !out.contains("survived"),
        "it must abort at the release, not carry on to the end of main; got:\n{}",
        out
    );
}

/// **The check runs above the immortal check**, which is the whole of #490's
/// fix and the one property a later edit is most likely to undo.
///
/// `_meta` is the free-list link after a free, and bit 12 of that link *is*
/// `_meta.immortal`. Measured on mps2/an385 the link was `0x2000324c` and
/// that bit read back **1** -- so with the freed check below the immortal
/// check, `oz_release` returns before reaching it and a release of a freed
/// object is **silent on the primary target**. On qemu_cortex_a53 the link
/// was `0x40062978` and the same bit read back **0**, so there the release
/// would have gone on to the trap. The bits are what was measured; which
/// board aborts follows from them and from `oz_release`'s shape. One
/// fixture, two verdicts, decided by an address.
///
/// This stages that deterministically: a live object with the sentinel *and*
/// `immortal` set. Move the check back below the immortal test and this case
/// prints "survived" and exits 0 -- which is how it was falsified.
#[test]
fn the_freed_check_outranks_the_immortal_bit() {
    let src = program(
        "\
	w->base._meta.immortal = 1;
	oz_atomic_init(&w->base.oz_refcount, OZ_REFCOUNT_FREED);
	oz_release((struct OZObject *)w);
",
    );
    let out = expect_trap(&src, "trap_freed_outranks_immortal", &["-DOZ_DEBUG_REFCOUNT"]);
    assert!(
        out.contains("release of a freed object"),
        "an `immortal` bit read out of a free-list link must not route around the trap; \
         got:\n{}",
        out
    );
    assert!(
        !out.contains("survived"),
        "the release must abort; reaching the end of main is exactly the silence measured \
         on mps2/an385 before this check moved; got:\n{}",
        out
    );
}

/// Retaining a slot carrying the sentinel aborts too, and above the
/// `deallocating` check for the same reason.
///
/// A use-after-free that *retains* is the worse of the two: the slot is
/// already on a free list, so the retain hands out a reference to storage
/// the next allocation will get.
#[test]
fn retaining_a_slot_carrying_the_freed_sentinel_aborts() {
    let src = program(
        "\
	oz_atomic_init(&w->base.oz_refcount, OZ_REFCOUNT_FREED);
	(void)oz_retain((struct OZObject *)w);
",
    );
    let out = expect_trap(&src, "trap_retain_after_free", &["-DOZ_DEBUG_REFCOUNT"]);
    assert!(
        out.contains("retain of a freed object"),
        "the retain path needs its own message: the fault is a reference handed out of a \
         free list, not a resurrection during dealloc; got:\n{}",
        out
    );
    assert!(
        !out.contains("during its own dealloc"),
        "a freed slot must not be reported as a retain-during-dealloc -- `deallocating` is \
         bit 11 of the same clobbered word, so that message would be an accident of the \
         address; got:\n{}",
        out
    );
    assert!(
        !out.contains("survived"),
        "it must abort at the retain; got:\n{}",
        out
    );
}

/// The paired negative: the check is an **equality on the sentinel**, not a
/// threshold.
///
/// Without this row, a check written as an inequality would satisfy every
/// assertion above while trapping on a refcount that is merely large.
///
/// **Both sides, and the second one is why this test was rewritten.** The
/// first version tested only `OZ_REFCOUNT_FREED - 1`, and then survived the
/// falsification it was written for: substituting `>=` for `==` left it
/// green, because the below-neighbour does not satisfy `>=` either. One
/// neighbour catches an inequality in one direction only. `- 1` catches
/// `<=`, an above-value catches `>=`; it takes both to mean "equality".
///
/// **The above-value is `+ 2`, not `+ 1`, and the reason is a real property
/// of the scheme rather than a convenience.** A live refcount one *above*
/// the sentinel decrements *into* it, so `+ 1` traps -- correctly, at the
/// scope-end release rather than the explicit one, having passed the check
/// and then become the sentinel. Two above is the closest value this
/// fixture's two releases cannot walk onto. The collision is noted on
/// `OZ_REFCOUNT_FREED` in the companion header and is not worth designing
/// away: reaching it needs a quarter of a billion live references.
#[test]
fn a_refcount_beside_the_sentinel_is_not_a_freed_slot() {
    for (offset, label) in [("- 1", "below"), ("+ 2", "above")] {
        let src = program(&format!(
            "\
	oz_atomic_init(&w->base.oz_refcount, OZ_REFCOUNT_FREED {});
	oz_release((struct OZObject *)w);
",
            offset
        ));
        let out = common::compile_and_run_with_cc_flags(
            &src,
            &format!("trap_beside_sentinel_{}", label),
            &["-DOZ_DEBUG_REFCOUNT"],
        );
        assert_eq!(
            out, "alive\nsurvived\n",
            "the freed check must fire on exactly the sentinel; {} it, the value is an \
             ordinary live refcount; got:\n{}",
            label, out
        );
    }
}
