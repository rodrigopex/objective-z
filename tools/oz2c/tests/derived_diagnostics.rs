// SPDX-License-Identifier: Apache-2.0
//
// derived_diagnostics.rs -- a refused send must not invent a second
// diagnostic about its own recovery value (#561).
//
// A send oz2c refuses is recovered as `("0", "int")` -- the literal `0`,
// typed `int` -- so emission continues and gathers the rest of the file's
// diagnostics instead of stopping at the first. That is the right trade,
// and it had one bad consequence: when the refused send was itself the
// **receiver** of an outer send, the outer send resolved against `int` and
// reported a second failure naming a type that appears nowhere in the
// source.
//
//     id t = [[Absent alloc] init];        // `Absent` declared nowhere
//
//     error: ... for selector 'alloc' (receiver type is 'id')    <- the mistake
//     error: ... for selector 'init'  (receiver type is 'int')   <- invented
//
// Correcting `Absent` removed both, there was no `init` problem to act on,
// and `int` was oz2c's own recovery value read back as though it were a
// fact about the program.
//
// This is the same family as #540 -- one diagnostic's handling degrading
// another's -- and the opposite direction: #540 was a check that
// *suppressed* unrelated diagnostics, this was a recovery value that
// *invented* one. Which is why the fix is keyed on span **equality** and
// not containment, and why the second test below is the one that matters:
// getting this wrong in the other direction re-creates #540.

mod common;
use common::{expect_reject, ozobject_src as PREAMBLE};

/// The reported shape. One mistake, one diagnostic.
#[test]
fn a_refused_receiver_does_not_produce_a_second_derived_diagnostic() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)run;
@end

@implementation Probe
- (int)run
{
	id t = [[Absent alloc] init];

	(void)t;
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    /* The real mistake is still reported -- an absence check alone would
     * also pass if the whole refusal had stopped working. */
    assert!(
        diags.contains("for selector 'alloc'"),
        "the real mistake stopped being reported:\n{}",
        diags
    );
    /* And the derived one is gone. `int` is the recovery value, so the
     * needle is narrow enough to name only this: no other diagnostic this
     * program can produce mentions a receiver of that type. */
    assert!(
        !diags.contains("receiver type is 'int'"),
        "the derived diagnostic is still invented:\n{}",
        diags
    );
    /* Counted, not just inspected: "one mistake, one diagnostic" is the
     * property, and a substring check cannot see a third. */
    assert_eq!(
        diags.lines().filter(|l| l.contains("cannot statically resolve")).count(),
        1,
        "expected exactly one unresolved-receiver diagnostic:\n{}",
        diags
    );
}

/// **The counter-direction, and the test that keeps this from becoming
/// #540 again.**
///
/// A refusal *nested inside* the receiver is not a refusal *of* the
/// receiver. Here the receiver is `[self wrap:@encode(int)]` -- whose
/// argument is refused (#563) -- while the outer send is unresolvable for
/// its own, unrelated reason. Both must be reported, or the author fixes
/// the first and discovers the second on the next build, one layer at a
/// time.
///
/// This is why `receiver_already_reported` tests span **equality**.
/// Containment is the tempting spelling and it silences this case.
#[test]
fn an_unrelated_refusal_inside_the_receiver_still_reports_both() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)run;
- (id)wrap:(const char *)s;
@end

@implementation Probe
- (id)wrap:(const char *)s { (void)s; return self; }
- (int)run
{
	id t = [[self wrap:@encode(int)] nosuchselector];

	(void)t;
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'@encode' is not in the static subset"), "{}", diags);
    assert!(
        diags.contains("for selector 'nosuchselector'"),
        "the outer send's own diagnostic was swallowed -- this is #540's shape:\n{}",
        diags
    );
}

/// A forward-declared receiver keeps #557's diagnostic, which is a
/// *different* cause reaching the same arm and sits after the new guard.
/// The two cannot both apply -- a forward-declared receiver is a bare
/// name, never a refused send -- but the guard was placed before it, so
/// this asserts the ordering did not cost anything.
#[test]
fn a_forward_declared_receiver_keeps_its_own_diagnostic() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@class Absent;

@interface Probe : OZObject
- (int)run;
@end

@implementation Probe
- (int)run
{
	Absent *a = nil;

	[a poke];
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("is only forward-declared"),
        "#557's diagnostic was lost to #561's guard:\n{}",
        diags
    );
}
