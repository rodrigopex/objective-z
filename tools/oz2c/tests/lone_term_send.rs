// SPDX-License-Identifier: Apache-2.0
//
// lone_term_send.rs -- `[ value]` names one term where a send needs two
// (#551).
//
// The shape was already refused, and refused with the wrong diagnosis.
// `parse_message` accepts a two-child `message_expression` as
// receiver-plus-selector, and tree-sitter recovers `[ value]` as exactly
// that: the receiver `value`, plus a MISSING identifier whose text is the
// empty string. So the selector was "", the receiver's type had degraded
// to `id`, and the error read
//
//     cannot statically resolve the receiver type for selector ''
//     (receiver type is 'id')
//
// naming an empty selector, blaming a type that was never the problem,
// and offering "declare the receiver as the class that implements ''" --
// advice with nothing in it to act on.
//
// Which half the author omitted is genuinely undecidable, and the
// diagnostic says so rather than guessing. Clang binds the single term as
// the receiver too and answers `use of undeclared identifier 'value'`, so
// neither tool can tell you whether `value` was meant as the selector or
// as the receiver -- verified against clang-19, not assumed.
//
// `[]` and `[self :1]` are *not* this shape: tree-sitter returns an
// `ERROR` node for both, so they never reach a `message_expression` walk
// at all. That is why `oz2c-challenges`' MUTATIONS.md grades M06 and M10
// as caught by Clang rather than by oz2c, and why this file does not
// cover them.

mod common;
use common::{expect_reject, ozobject_src};

const PRELUDE: &str = "\
@interface Probe : OZObject {
	int _n;
}
- (int)value;
- (int)run;
@end
";

fn program(body: &str) -> String {
    format!("{}{}\n@implementation Probe\n- (int)value\n{{\n\treturn _n;\n}}\n{}\n@end\n", ozobject_src(), PRELUDE, body)
}

/// The case M07 filed, verbatim in shape: a selector with no receiver in
/// front of it.
#[test]
fn a_send_naming_one_term_is_refused_by_shape_not_by_receiver_type() {
    let src = program(
        "\
- (int)run
{
	int x = [ value];

	return x;
}
",
    );
    let err = expect_reject(&src);

    assert!(
        err.contains("names one term where a message send needs two"),
        "expected the lone-term diagnosis; got:\n{}",
        err
    );
    assert!(
        err.contains("'[ value]'"),
        "the diagnostic must quote the offending send, the way M01's does; got:\n{}",
        err
    );

    /* The absence half. Asserting only the new text would still pass if
     * the old diagnostic were emitted *alongside* it, which is the shape
     * that makes a guard pass while the property it guards is gone. */
    assert!(
        !err.contains("selector ''"),
        "the empty-selector diagnosis must be gone, not merely joined by a better one; got:\n{}",
        err
    );
    assert!(
        !err.contains("receiver type is 'id'"),
        "the receiver's type is not the complaint any more; got:\n{}",
        err
    );

    /* Located, like every other refusal. The mutation sweep's headline
     * finding was that not one oz2c refusal was unlocated, and a new
     * diagnostic is exactly where that would stop being true.
     *
     * `LINE:COL: message` read from the *left*: the message itself
     * contains colons, so splitting from the right finds prose. Same
     * reasoning as `malformed_send.rs`'s own location check. */
    assert!(
        err.lines().any(|l| {
            let mut parts = l.trim_start().splitn(3, ':');
            let line = parts.next().unwrap_or("");
            let col = parts.next().unwrap_or("");
            !line.is_empty()
                && line.chars().all(|c| c.is_ascii_digit())
                && !col.is_empty()
                && col.chars().all(|c| c.is_ascii_digit())
        }),
        "the diagnostic must carry a file position; got:\n{}",
        err
    );
}

/// Both remedies are offered, because the source does not say which was
/// meant. A diagnostic that picked one would be right half the time and
/// send the other half looking in the wrong place.
#[test]
fn both_readings_of_the_lone_term_are_offered_as_remedies() {
    let src = program(
        "\
- (int)run
{
	int x = [ value];

	return x;
}
",
    );
    let err = expect_reject(&src);

    assert!(
        err.contains("if 'value' is the selector"),
        "the selector reading must be offered; got:\n{}",
        err
    );
    assert!(
        err.contains("if 'value' is the receiver"),
        "the receiver reading must be offered; got:\n{}",
        err
    );
    assert!(
        err.contains("[self value]"),
        "the selector reading needs a concrete fix to copy; got:\n{}",
        err
    );
}

/// `[obj]` is the same shape without the leading space, and it is the
/// spelling a real program reaches by deleting a selector rather than a
/// receiver. One code path serves both; this is what says so.
#[test]
fn a_bare_bracketed_identifier_is_the_same_shape() {
    let src = program(
        "\
- (int)run
{
	Probe *p = self;

	return [p];
}
",
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("names one term where a message send needs two"),
        "expected the lone-term diagnosis for '[p]'; got:\n{}",
        err
    );
    assert!(
        err.contains("if 'p' is the receiver"),
        "the remedies must quote this send's own term; got:\n{}",
        err
    );
}

/// The control that keeps the split honest: a send with a *dropped colon*
/// is a different malformation and must keep its own message.
///
/// Without this, collapsing both shapes back onto one message would pass
/// every assertion above.
#[test]
fn a_dropped_colon_keeps_its_own_diagnosis() {
    let src = program(
        "\
- (int)run
{
	return [self addingWith other];
}
",
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("each keyword needs a ':' before its argument"),
        "the missing-colon diagnosis must survive the split; got:\n{}",
        err
    );
    assert!(
        !err.contains("names one term where a message send needs two"),
        "a three-term send is not the lone-term shape; got:\n{}",
        err
    );
}
