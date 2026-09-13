// SPDX-License-Identifier: Apache-2.0
//
// bridging_cast_ownership.rs -- three bridging casts, three meanings, and
// the two that transfer a reference are refused (#460).
//
// ARC spec § 1.3.4 defines `(__bridge T)`, `(__bridge_retained T)` and
// `(__bridge_transfer T)`. They mean three different things:
// the first transfers nothing, the second retains and hands a `+1` out to
// C, the third takes a `+1` over from C and releases it.
//
// `arc::is_bridging_cast` matches all three by name and treats them
// identically -- as a signal to hold ownership *back*, so none of the
// three ownership questions looks through them. That is exactly right for
// plain `__bridge` and wrong for the other two, in opposite directions:
//
//   * `__bridge_retained` emitted no retain, so the local was still
//     released at scope exit and the C side was handed a freed slot --
//     `heap-use-after-free` under ASan. The stale read *succeeded* first
//     and printed the right value, which is the trap `docs/STATUS.md`
//     records: a use-after-free is silent until the allocator reuses the
//     block.
//   * `__bridge_transfer` emitted no release, stranding the `+1` it took
//     over. A leak.
//
// Both from sources `clang -fobjc-arc -Weverything` accepts with zero
// diagnostics.
//
// **Refused, not implemented, and the decision was measured.** Across
// `src/`, `include/`, `samples/`, `tests/` and px-keyboard there are zero
// uses of either kind; all six bridging casts in the tree are plain
// `__bridge` and all six are correct. So this refuses nothing that exists
// and turns two silent memory bugs into build errors -- the #430 → #458
// precedent. Implementing them needs new emission and is sequenced after
// #462's respelling of the emitted ABI; it is a product question (is
// CF-style hand-off supported?) rather than a correctness one.
//
// What this file therefore has to pin is *both* halves: the two refusals,
// and that plain `__bridge` is untouched -- including that it is still
// **opaque to the ownership questions**. Narrowing
// `arc::is_bridging_cast` to one spelling would make the other two fall
// through to the ordinary cast path, which looks *through* a cast and is
// how #332 double-released. The refusal at the bar and the opacity in the
// analysis are complementary, not redundant, and the last test here is
// what says so.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

const DECLS: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag {
	return 7;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end
";

/// `__bridge_retained` is a located error naming the kind and the
/// consequence.
#[test]
fn bridge_retained_is_refused() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
static void *g_raw;

@interface P : OZObject
- (void)handOut;
@end
@implementation P
- (void)handOut {
	Thing *t = [[Thing alloc] init];
	g_raw = (__bridge_retained void *)t;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("__bridge_retained"),
        "the diagnostic must name the cast kind:\n{}",
        diags
    );
    assert!(
        diags.contains("use-after-free"),
        "and say what goes wrong, since the symptom is silent:\n{}",
        diags
    );
    assert!(
        diags.contains("__bridge"),
        "and offer the writable remedy -- a plain '(__bridge T)' cast:\n{}",
        diags
    );
}

/// `__bridge_transfer` likewise, with the leak named rather than the
/// use-after-free.
///
/// Separate assertions per kind because the two diagnostics differ on
/// purpose: an author who reads "use-after-free" for a leak learns the
/// wrong thing about their own code.
#[test]
fn bridge_transfer_is_refused() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (int)takeOver:(void *)p;
@end
@implementation P
- (int)takeOver:(void *)p {
	Thing *t = (__bridge_transfer Thing *)p;
	return [t tag];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("__bridge_transfer"),
        "the diagnostic must name the cast kind:\n{}",
        diags
    );
    assert!(
        diags.contains("leak"),
        "and name the leak rather than the other kind's fault:\n{}",
        diags
    );
}

/// Both kinds are refused in a plain C function too, not only in a method.
///
/// The walk is whole-root for the reason the four checks beside it are:
/// none of `staticbar`'s body-scoped entry points sees every position, and
/// `walk_for_reject` treats a block literal as opaque. Without this test a
/// method-only check would look finished.
#[test]
fn both_kinds_are_refused_outside_a_method_body() {
    for kind in ["__bridge_retained", "__bridge_transfer"] {
        let src = format!(
            "{}{}{}",
            PREAMBLE(),
            DECLS,
            format!(
                "\
static void *g_raw;

static int borrow_in_c(Thing *arg)
{{
	g_raw = ({kind} void *)arg;
	return 0;
}}
"
            )
        );
        let diags = expect_reject(&src);
        assert!(
            diags.contains(kind),
            "'{}' must be refused in a plain C function as well:\n{}",
            kind,
            diags
        );
    }
}

/// Plain `__bridge` is untouched -- in both directions, and it emits no
/// refcount traffic of its own.
///
/// This is the shape the tree actually uses:
/// `px-keyboard/src/PXLEDController.m:61,143` hands `self` to a Zephyr
/// `k_timer` user_data and reads it back.
#[test]
fn plain_bridge_is_accepted_and_transfers_nothing() {
    let src = format!(
        "/* oz-pool: Thing=2,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
{
	Thing *_owned;
}
- (instancetype)initOwned;
- (void *)raw;
- (int)peek:(void *)p;
@end
@implementation P
- (instancetype)initOwned {
	_owned = [[Thing alloc] init];
	return self;
}
- (void *)raw {
	return (__bridge void *)_owned;
}
/* A plain `__bridge` transfers nothing, so `p` belongs to whoever gave it
   to us and this method must release nothing -- however many times it
   runs. */
- (int)peek:(void *)p {
	Thing *t = (__bridge Thing *)p;
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [[P alloc] initOwned];
	void *raw = [p raw];
	int first = [p peek:raw];
	int second = [p peek:raw];
	int third = [p peek:raw];
	printf(\"%d %d %d deallocs=%d\\n\", first, second, third, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "plain_bridge_transfers_nothing");
    /* Three round-trips through C and the ivar's object is still alive:
     * `deallocs=0` is the whole claim. Before #459 this same shape could
     * report 1 for an unrelated reason, which is why the fixture's locals
     * are named distinctly from every other method's. */
    assert_eq!(
        out, "7 7 7 deallocs=0\n",
        "a plain __bridge must not move ownership in either direction: {}",
        out
    );
}

/// A plain `__bridge` stays **opaque** to the ownership questions, and
/// narrowing `arc::is_bridging_cast` would break that quietly.
///
/// The predicate lists all three kinds, and it still does after this
/// change: its job is to stop a bridging cast being looked *through*. An
/// ordinary cast is looked through on purpose (#332), so a bridging cast
/// that fell into that path would have its operand's ownership read as the
/// binding's -- `Thing *t = (__bridge Thing *)[Thing alloc];` would be
/// treated as `+1` twice over.
///
/// Asserted behaviourally because the predicate is private: the local here
/// must get exactly one release, not two.
#[test]
fn a_bridging_cast_is_not_looked_through_like_an_ordinary_one() {
    let src = format!(
        "/* oz-pool: Thing=2,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (int)roundTrip;
@end
@implementation P
- (int)roundTrip {
	Thing *made = [[Thing alloc] init];
	void *opaque = (__bridge void *)made;
	Thing *back = (__bridge Thing *)opaque;
	return [back tag];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int v = [p roundTrip];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "bridging_cast_not_looked_through");
    /* `made` owns the only reference and is released once at scope exit.
     * `back` is the same pointer arrived at through C and owns nothing --
     * if the bridging cast were looked through, `back` would be read as a
     * second `+1` and released too, which is a double free. One dealloc. */
    assert_eq!(
        out, "v=7 deallocs=1\n",
        "the round-tripped pointer must not be counted as a second reference: {}",
        out
    );
}
