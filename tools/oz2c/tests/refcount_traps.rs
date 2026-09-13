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
/// than wishing otherwise -- see the comment there.
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
    /* **The class prints as `?`, and that is the honest answer today.** The
     * first release already ran `_oz_free`, so `oz_class_name` reads
     * `_meta.class_id` out of freed memory and falls to its `default:` arm.
     * On a true double free the class is simply not knowable *from the
     * object* -- the evidence is in the storage that was returned.
     *
     * Which is the concrete reason #452 pairs the trap with freed-slot
     * poisoning rather than shipping the trap alone: once `_oz_free` stamps
     * a reserved `OZ_CLASS_ID_FREED`, this line reads "freed" instead of
     * "?", and the diagnostic goes from "something was over-released" to
     * "something already freed was released again". **This assertion is
     * expected to change when that lands**, and is written to fail loudly
     * rather than silently pass, so it is a marker and not a hazard. */
    assert!(
        out.contains("over-release of ?"),
        "the class is unknowable after the free -- `?` until poisoning stamps a reserved \
         class_id (#452's second part); got:\n{}",
        out
    );
    assert!(
        !out.contains("survived"),
        "it must abort at the second release, not carry on to the end of main; got:\n{}",
        out
    );
}

/// The control: one release, the flag still on, and the program runs to
/// completion.
///
/// Without this the test above would pass just as well if the trap fired on
/// every release, or on program exit, or unconditionally -- and a trap that
/// always fires is worse than none, because it makes the instrument
/// unusable rather than merely absent.
#[test]
fn a_correct_release_does_not_trip_the_trap() {
    let src = program("\toz_release((struct OZObject *)w);\n");
    let out = compile_and_run(&src, "trap_correct_release");
    assert_eq!(
        out, "alive\nsurvived\n",
        "one release is correct and must be silent; got:\n{}",
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
