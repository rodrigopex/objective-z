// SPDX-License-Identifier: Apache-2.0
//
// protocol_optional_members.rs -- `@optional` protocol members may be
// absent from a conformer (#536).
//
// `emit::render_interface` required *every* method of every protocol a
// class conforms to, so a conformer omitting an `@optional` member was
// refused outright -- which is the entire case the keyword exists for.
// px-app's torture suite carried `WA-011` for it: delete the `@optional`
// section and redeclare the method on the implementing class's own
// `@interface`.
//
// The check is right to exist and is narrowed, not removed. A missing
// protocol method used to yield a NULL vtable entry and a silent crash
// (OZ-033, `px-app CHANGELOG.md:37`), so the *required* half still refuses.
//
// The tests below come in pairs on purpose. Lifting a requirement is easy
// to over-do in two directions at once: refuse nothing (which puts OZ-033
// back), or lift the requirement *and* the dispatch, which would leave an
// optional member undeclared and `-respondsToSelector:` unable to answer
// for it.

mod common;
use common::{
    compile_and_run_with_reflection, expect_reject, ozobject_src as PREAMBLE,
};

/// The reported shape: two conformers, one implementing the `@optional`
/// member and one deliberately not.
#[test]
fn an_optional_member_may_be_omitted() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Calibratable
- (int)calibrate;
@optional
- (int)selfTest;
@end

@interface Thermistor : OZObject <Calibratable>
- (int)calibrate;
- (int)selfTest;
@end
@implementation Thermistor
- (int)calibrate { return 1; }
- (int)selfTest { return 2; }
@end

@interface StrainGauge : OZObject <Calibratable>
- (int)calibrate;
@end
@implementation StrainGauge
- (int)calibrate { return 3; }
@end
"
    );
    oz2c::transpile(&src).expect("an '@optional' member may be absent from a conformer");
}

/// The class side of `@optional`, which nothing exercised until #606.
///
/// `emit`'s conformance walk does `if required.is_optional { continue; }`
/// *before* it consults `required.is_class_method`, so the class side was
/// always going to behave -- but "always going to" is not a verdict, and
/// `docs/OBJECTIVE_C_DIALECT.md` carried `protocol.optional.class` as
/// `UNEXAMINED` for exactly that reason. Every other `@optional` test in
/// this file uses instance methods.
///
/// Both directions, because one passing says nothing about the other: a
/// conformer that omits the optional class method must be accepted, and
/// one that supplies it must get its `_cls` entry point.
#[test]
fn an_optional_class_method_may_be_omitted_or_supplied() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Spawnable
- (int)tag;
@optional
+ (int)spawn;
@end

@interface Absent : OZObject <Spawnable>
- (int)tag;
@end
@implementation Absent
- (int)tag { return 1; }
@end

@interface Present : OZObject <Spawnable>
- (int)tag;
+ (int)spawn;
@end
@implementation Present
- (int)tag { return 2; }
+ (int)spawn { return 42; }
@end
"
    );
    let out = oz2c::transpile(&src)
        .expect("an '@optional' class method may be absent from a conformer");

    /* The conformer that supplies it gets the class-side entry point. A
     * class method takes no receiver, so the shape is `(void)`. */
    assert!(
        out.source_c.contains("int Present_spawn_cls(void)"),
        "the conformer that implements the optional class method must get its \
         class-side entry point; got:\n{}",
        out.source_c
    );

    /* And the one that omits it contributes nothing under that name --
     * paired with the presence check above, because a test that only
     * looked for an absence would pass against an empty header. */
    assert!(
        !out.source_c.contains("Absent_spawn"),
        "the conformer that omits it must not acquire an accessor; got:\n{}",
        out.source_c
    );
}

/// The half that must not move. A **required** member is still required,
/// because the generated dispatch has no entry to fall back to.
#[test]
fn a_required_member_is_still_required() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Calibratable
- (int)calibrate;
@optional
- (int)selfTest;
@end

@interface StrainGauge : OZObject <Calibratable>
- (int)selfTest;
@end
@implementation StrainGauge
- (int)selfTest { return 2; }
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("declares conformance to 'Calibratable' but doesn't implement 'calibrate'"),
        "{}",
        diags
    );
    /* The `help:` tier #536 asks for -- the message could not previously
     * say what to do, and the middle one is the fix for the case the issue
     * was actually filed about. */
    assert!(diags.contains("move it under '@optional'"), "{}", diags);
    assert!(diags.contains("-respondsToSelector:"), "{}", diags);
}

/// **The property `WA-011` preserved, and the one a careless fix loses.**
///
/// Lifting the conformance requirement must not lift the *dispatch*: an
/// optional member still needs its `OZ_PROTOCOL_SEND_*` function, because
/// `-respondsToSelector:` is how a caller tests for one and then sends it.
/// So `Program::all_protocol_methods` must keep returning optional members
/// even though the conformance check now skips them -- two consumers of one
/// list, wanting opposite things.
///
/// Run rather than inspected: the yes/no contrast between the two
/// conformers is the whole observable behaviour of `@optional`.
#[test]
fn responds_to_selector_still_distinguishes_the_two_conformers() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Calibratable
- (int)calibrate;
@optional
- (int)selfTest;
@end

@interface Thermistor : OZObject <Calibratable>
- (int)calibrate;
- (int)selfTest;
@end
@implementation Thermistor
- (int)calibrate { return 1; }
- (int)selfTest { return 2; }
@end

@interface StrainGauge : OZObject <Calibratable>
- (int)calibrate;
@end
@implementation StrainGauge
- (int)calibrate { return 3; }
@end

#include <stdio.h>
int main(void) {
	Thermistor *t = [Thermistor alloc];
	StrainGauge *g = [StrainGauge alloc];
	SEL st = @selector(selfTest);

	printf(\"t=%d g=%d cal=%d\\n\",
	       [t respondsToSelector:st],
	       [g respondsToSelector:st],
	       [t calibrate] + [g calibrate]);
	return 0;
}
"
    );
    let out = compile_and_run_with_reflection(&src, "optional_responds_to_selector");
    assert_eq!(out, "t=1 g=0 cal=4\n", "unexpected: {}", out);
}

/// `@required` after `@optional` **resets**, so the marker is per block and
/// not inherited.
///
/// This is why the flag is read on entry to each
/// `qualified_protocol_interface_declaration` rather than passed down from
/// an enclosing one: tree-sitter makes the two markers *siblings*, which
/// was verified on a dump rather than assumed. Had they nested, this test
/// is what would have caught the difference -- `mustHave` would have
/// inherited `@optional` and the omission would be wrongly accepted.
#[test]
fn a_required_marker_resets_a_previous_optional() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Staged
@optional
- (int)mayHave;
@required
- (int)mustHave;
@end

@interface Partial : OZObject <Staged>
- (int)mayHave;
@end
@implementation Partial
- (int)mayHave { return 1; }
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("doesn't implement 'mustHave'"), "{}", diags);
    /* And the optional one before it is still not required, so the reset
     * is a reset and not a blanket. */
    assert!(!diags.contains("'mayHave'"), "the '@optional' member was required too:\n{}", diags);
}

/// An unmarked declaration -- before any marker -- is required. That is the
/// `false` the outer call starts from, and it is what every protocol in the
/// tree relies on, since none of them used a marker before this change.
#[test]
fn an_unmarked_member_is_required() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Plain
- (int)needed;
@end

@interface Empty : OZObject <Plain>
@end
@implementation Empty
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("doesn't implement 'needed'"), "{}", diags);
}
