// SPDX-License-Identifier: Apache-2.0
//
// forin_receiver_class.rs -- a `for-in` loop variable used as a message
// receiver resolves to its declared class, so a `+1` the send returns is
// released (#502).
//
// Found while measuring #483's claim that the `arc`/emitter resolution gap
// is unbounded. It is, and this was the live instance: the emitter resolved
// the loop variable and emitted a **static** call, `arc` did not resolve it,
// so `dispatch_ownership` polled every reachable implementor, two disagreed,
// `Ambiguous` read as borrowed, and the `+1` leaked.
//
//     struct Thing *t = Owner_build((struct Owner *)(it));   /* no release */
//
// Identical mechanism to #481, and it matters that it is not identical in
// *cause*. #481 was a missing node kind -- `method_parameter` -- and adding
// it to `arc::collect_declared_types`'s match closed that one instance.
// **No addition to that match could have closed this one.** The for-in
// header is
//
//     for ( <type parts…> <declarator> in <collection> ) <body>
//
// whose parts are *siblings* of `for_statement`. There is no `declaration`,
// `parameter_declaration` or `method_parameter` node anywhere in it, so the
// binding is not a kind at all -- it is a scope entry the emitter
// synthesises while rendering. A gate enumerating node kinds, which is what
// #483 lists as its cheapest option, would have passed green while this
// leaked.
//
// So the fix reads the header through `emit::forin_binding`, the same parse
// the emitter uses, rather than slicing it a second time in `arc`. That is
// the difference from #481: one reader answers for both, instead of two
// lists that have to track each other.
//
// The generalisation worth keeping, because it bounds where the next one can
// be: **the dangerous condition is not a kind `arc` does not know, it is
// asymmetry in one direction -- the emitter resolves and `arc` does not.**
// Both failing together is safe. Measured on a block parameter, which
// neither resolves: the send goes dynamic and
// `emit::dynamic_dispatch_call`'s existing `Ambiguous` refusal catches it
// with a located error. Only the emitter succeeding alone produces a static
// call carrying a polled answer.

mod common;
use common::{compile_and_run, iterator_protocol_src, ozarray_src, ozobject_src};

/// `Thing` counts its own deallocations, which is the oracle throughout.
const THING: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag
{
	return 7;
}
- (void)dealloc
{
	g_deallocs = g_deallocs + 1;
}
@end

@interface Owner : OZObject
- (Thing *)build;
@end
@implementation Owner
- (Thing *)build
{
	return [[Thing alloc] init];
}
@end
";

/// A second implementor of `-build` that **disagrees** about ownership:
/// `Owner` hands back a `+1`, `Lender` hands back what it keeps. That
/// disagreement is what makes the poll ambiguous, and ambiguity is what
/// reads as borrowed.
const LENDER: &str = "\
@interface Lender : OZObject
{
	Thing *_held;
}
- (Thing *)build;
@end
@implementation Lender
- (Thing *)build
{
	return _held;
}
@end
";

fn program(extra: &str, body: &str) -> String {
    format!(
        "/* oz-pool: Thing=8,Owner=4,Lender=2,P=1,OZArray=2 */\n{}{}{}{}{}{}",
        ozobject_src(),
        iterator_protocol_src(),
        ozarray_src(),
        THING,
        extra,
        format!(
            "\
@interface P : OZObject
- (int)run:(OZArray *)arr;
@end
@implementation P
- (int)run:(OZArray *)arr
{{
	int last = 0;

{}
	return last;
}}
@end

#include <stdio.h>
int main(void)
{{
	Owner *o = [Owner alloc];
	OZArray *arr = @[o];
	P *p = [P alloc];
	int v = [p run:arr];

	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}}
",
            body
        )
    )
}

const FORIN_BODY: &str = "\
	for (Owner *it in arr) {
		Thing *t = [it build];

		last = [t tag];
	}
";

const LOCAL_BODY: &str = "\
	{
		Owner *it = [arr objectAtIndex:0];
		Thing *t = [it build];

		last = [t tag];
	}
";

/// The four cells that localise the defect.
///
/// The leak needed **both** conditions: the receiver bound by a for-in
/// header *and* implementors that disagree. Either alone is correct, which
/// is why it had never been seen -- and why a one-cell test would not have
/// told anyone what the cause was.
///
/// The `local` rows are the controls that say this was about the binding
/// form rather than about ambiguity: the same disagreement through a local
/// receiver has always released correctly, because a local *is* a
/// `declaration` and `arc` has always read those.
#[test]
fn a_forin_receiver_releases_like_a_local_one() {
    let cells: &[(&str, &str, &str)] = &[
        ("for-in, disagreeing implementors", LENDER, FORIN_BODY),
        ("for-in, one implementor", "", FORIN_BODY),
        ("local, disagreeing implementors", LENDER, LOCAL_BODY),
        ("local, one implementor", "", LOCAL_BODY),
    ];
    for (name, extra, body) in cells {
        let src = program(extra, body);
        let stem = format!(
            "forin_recv_{}",
            name.replace(", ", "_").replace(' ', "_").replace('-', "")
        );
        assert_eq!(
            compile_and_run(&src, &stem),
            "v=7 deallocs=1\n",
            "{}: the Thing that -build returned must be released exactly once. Before #502 \
             the first row read deallocs=0 -- the emitter resolved `it` and emitted \
             `Owner_build(...)` statically while `arc` polled and read the ambiguity as \
             borrowed",
            name
        );
    }
}

/// Two loops binding the same name in one method, which is ordinary and
/// must not be read as a shadowing conflict.
///
/// Each `for_statement` is walked from the enclosing body, so both headers
/// are seen and `count` reaches 2 -- which would make `declared_class_of`
/// decline and put the ownership answer back on the poll. This case exists
/// because that is the obvious way to get the fix wrong, and its oracle is
/// two releases rather than one.
#[test]
fn two_loops_binding_the_same_name_still_resolve() {
    let src = program(
        LENDER,
        "\
	for (Owner *it in arr) {
		Thing *t = [it build];

		last = [t tag];
	}
	for (Owner *it in arr) {
		Thing *t = [it build];

		last = [t tag];
	}
",
    );
    assert_eq!(
        compile_and_run(&src, "forin_recv_two_loops"),
        "v=7 deallocs=2\n",
        "both loops allocate and both must release -- if the two headers count as two \
         declarations of one name, `declared_class_of` declines and the disagreeing poll \
         leaks both"
    );
}

/// **A known defect, asserted to still be defective.** Two loops binding
/// one name to *different* classes still leak both sends.
///
/// The `KNOWN_DEFECTS` convention `ownership_matrix.rs` uses: fixing this
/// fails this test, which forces the entry out in the same change, so the
/// record cannot rot into a silently-skipped shape.
///
/// **Why #502 does not fix it, and the mechanism is not the obvious one.**
/// The count in `declared_class_of` is over the *whole method*, so two
/// bindings of one name reach two and the lookup declines back onto the
/// ambiguous poll. C scoping says the nearest enclosing binding wins, and
/// implementing that -- walk up from the send, take the first enclosing
/// for-in that binds the name -- closed this leak and **introduced a
/// segfault**. Measured both ways: on `main` this program leaks and runs;
/// with nearest-enclosing resolution it exits on signal 11.
///
/// The first account of that was wrong in a way worth recording, because
/// it would send a reader into `arc` looking for a read it does not
/// perform. **The wrong-class static call is already `main`'s behaviour.**
/// Diffing the emitted C for the lying program across both binaries: both
/// emit `Lender_build((struct Lender *)(it))` on an object the array holds
/// as an `Owner`, byte for byte, because the *emitter* has always resolved
/// the loop variable from the header's declared type. That garbage read is
/// paid for already, and `arc` adds nothing to it.
///
/// What resolution added was the **only** differing operation:
/// `oz_release(t)`. `Lender_build` reads `_held` at some offset inside an
/// `Owner`-shaped allocation -- in-bounds nonsense, survivable, which is
/// why `main` runs. Handing that nonsense to `oz_release` is not.
///
/// So: **the leak was load-bearing.** It was the only thing standing
/// between a bad value and a bad free, and teaching `arc` a binding form
/// converts a silent wrong *read* into a *free* of a garbage pointer. That
/// is why resolution coverage is not monotonically safe -- not because
/// resolution creates the type lie, but because it makes ARC act on a value
/// the lie already produced.
///
/// Reverted rather than shipped. The honest fix needs the *collection's*
/// element type, which nothing in the tree tracks.
///
/// That is #483's territory -- whole-scope versus per-send resolution --
/// and it is recorded there rather than widened into this fix.
#[test]
fn known_defect_two_loops_binding_different_classes_leak() {
    let src = program(
        LENDER,
        "\
	for (Owner *it in arr) {
		Thing *t = [it build];

		last = [t tag];
	}
	for (Lender *it in arr) {
		Thing *t = [it build];

		last = last + [t tag];
	}
",
    );
    assert_eq!(
        compile_and_run(&src, "forin_recv_known_defect"),
        "v=14 deallocs=0\n",
        "KNOWN DEFECT (#483): both sends leak, because two bindings of one name make the \
         whole-method count decline. If this now reads deallocs=2, the defect is fixed -- \
         delete this test and say so in #483"
    );
}
