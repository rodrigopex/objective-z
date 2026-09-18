// SPDX-License-Identifier: Apache-2.0
//
// forward_declared_receiver.rs -- a send to a name known only from a
// `@class` names that as the cause, not the `id` it degraded to (#557).
//
// `@class Ghost;` says the name exists. It does not say what shape it
// has, so the class graph has no `@interface` to resolve a send against
// and the receiver's type degrades. The refusal was correct and its
// *reason* was the fallback:
//
//     cannot statically resolve the receiver type for selector 'alloc'
//     (receiver type is 'id')
//
// A reader who spelled a class name and was told the receiver is `id` has
// to work out for themselves that a forward declaration is invisible to
// the graph. Clang refuses the same construct naming the cause ("is a
// forward declaration"); oz2c fires first, because the resolve pass runs
// before Clang sees the file, so the generic answer is the only one the
// author ever gets.
//
// **Two spellings reach that arm with one cause**, and #557 filed only
// the first:
//
//   - `[Ghost alloc]` -- the receiver *is* the class name, so it never
//     became a `class:Ghost` receiver and arrived as `id`;
//   - `[g tick]` where `g` is a `Ghost *` -- the declared type names the
//     class, so the type arrives spelled `Ghost*` and
//     `class_name_from_type` refuses it for want of a `struct` tag.
//
// The second was found by looking for the sibling rather than reported,
// which is the rule the ARC defects of 2026-09 earned: ask the question
// of the reference, never of the form it was written in. Both are
// asserted here, because one passing says nothing about the other.

mod common;
use common::{expect_reject, ozobject_src};

fn program(body: &str) -> String {
    format!("{}{}", ozobject_src(), body)
}

/// The case M71 filed: allocating through a forward declaration.
#[test]
fn a_class_side_send_to_a_forward_declaration_names_the_cause() {
    let src = program(
        "\
@class Ghost;

@interface Probe : OZObject
- (int)run;
@end

@implementation Probe
- (int)run
{
	Ghost *ghost = [[Ghost alloc] init];

	(void)ghost;
	return 71;
}
@end
",
    );
    let err = expect_reject(&src);

    assert!(
        err.contains("'Ghost' is only forward-declared"),
        "the diagnosis must name the forward declaration; got:\n{}",
        err
    );
    assert!(
        err.contains("@class Ghost"),
        "the note must point at the construct responsible; got:\n{}",
        err
    );
    assert!(
        err.contains("@interface Ghost"),
        "the remedy must name what is missing; got:\n{}",
        err
    );

    /* The absence half: the old diagnosis led with the fallback, and
     * that is the part #557 is about.
     *
     * Scoped to the `alloc` send rather than to the whole output,
     * because `[[Ghost alloc] init]` produces a *second* diagnostic and
     * it is not this defect. A refused send recovers as `("0", "int")`,
     * so the outer `init` is then a send to an `int` and gets the
     * generic message naming a type the source never wrote. That
     * cascade is pre-existing and independent: an undeclared name --
     * no `@class` anywhere -- produces exactly the same pair, verified
     * against this same harness. Asserting the generic text absent
     * *anywhere* would therefore be asserting someone else's bug fixed,
     * and would fail on a tree where this one is perfectly correct. */
    let alloc_line = err
        .lines()
        .find(|l| l.contains("'alloc'") || l.contains("only forward-declared"))
        .unwrap_or("");
    assert!(
        !alloc_line.contains("cannot statically resolve the receiver type"),
        "the send that names the forward declaration must not still get the generic \
         message; got:\n{}",
        err
    );
    assert!(
        !err.contains("selector 'alloc' (receiver type is 'id')"),
        "the old fallback-naming message for this send must be gone, not merely joined \
         by a better one; got:\n{}",
        err
    );
}

/// The sibling spelling, in no issue: an *instance* send through a
/// variable whose declared type is the forward-declared class.
///
/// Reaches the same arm with `receiver type is 'Ghost*'` rather than
/// `'id'`, which is why a fix keyed on the `id` spelling alone would have
/// left this live.
#[test]
fn an_instance_send_through_a_forward_declared_type_names_the_cause() {
    let src = program(
        "\
@class Ghost;

@interface Probe : OZObject
- (int)run;
@end

@implementation Probe
- (int)run
{
	Ghost *g = (Ghost *)0;

	return [g tick];
}
@end
",
    );
    let err = expect_reject(&src);

    assert!(
        err.contains("'Ghost' is only forward-declared"),
        "the instance-send spelling must reach the same diagnosis; got:\n{}",
        err
    );
    assert!(
        !err.contains("cannot statically resolve the receiver type"),
        "this spelling must not fall back to the generic message either; got:\n{}",
        err
    );
}

/// The over-refusal control. A `@class` *followed by* a real
/// `@interface` is an ordinary class, and every send to it must still
/// transpile.
///
/// This is the half that would break if the check asked
/// `forward_declared.contains(name)` instead of
/// `is_forward_declared_only(name)` -- and a forward declaration ahead of
/// the interface is ordinary, idiomatic ObjC, so breaking it would refuse
/// working programs.
#[test]
fn a_forward_declaration_followed_by_the_interface_is_an_ordinary_class() {
    let src = program(
        "\
@class Thing;

@interface Thing : OZObject {
	int _n;
}
- (int)n;
@end

@implementation Thing
- (int)n
{
	return _n;
}
@end

int main(void)
{
	Thing *t = [[Thing alloc] init];

	return [t n];
}
",
    );
    oz2c::transpile(&src).expect("a forward declaration ahead of the interface must be accepted");
}

/// The other-cause control: a name declared *nowhere* is not this
/// defect, and must keep the diagnostic it already had.
///
/// Without this, a check that answered "forward-declared" for any
/// unresolvable name would look correct here and mislead on the case
/// #501 fixed.
#[test]
fn a_name_declared_nowhere_keeps_its_own_diagnosis() {
    let src = program(
        "\
@interface Probe : OZObject
- (int)run;
@end

@implementation Probe
- (int)run
{
	return [Absent tick];
}
@end
",
    );
    let err = expect_reject(&src);
    assert!(
        !err.contains("is only forward-declared"),
        "a never-declared name has no forward declaration to blame; got:\n{}",
        err
    );
}

/// `@class A, B;` is one `class_declaration` carrying an identifier per
/// name, so reading only the first would collect `A` and lose `B`
/// silently -- and the loss would show up as the *generic* diagnostic for
/// `B` alone, which is exactly the failure this file exists to remove.
#[test]
fn every_name_in_a_multi_name_forward_declaration_is_collected() {
    let src = program(
        "\
@class First, Second;

@interface Probe : OZObject
- (int)run;
@end

@implementation Probe
- (int)run
{
	return [Second tick];
}
@end
",
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("'Second' is only forward-declared"),
        "the second name in a '@class A, B;' must be collected too; got:\n{}",
        err
    );
}
