// SPDX-License-Identifier: Apache-2.0
//
// diagnostic_co_reporting.rs -- one build shows every independent problem
// (#540).
//
// A selector-collision error used to return before emit ran, so an
// unrelated per-site refusal -- a `@try`, a block capture -- three classes
// away was invisible. The consequence compounds for a consumer: adding a
// file surfaced an error about two *other* files, and fixing it revealed a
// second error that had been there all along. Each rebuild showed one
// layer.
//
// The collision rule itself is contract (#290) and unchanged. What was not
// contract is that a whole-program name check ran early enough to suppress
// per-site diagnostics.
//
// **The pipeline's asymmetry is the subject here.** `Collect` and the AST
// checks still gate hard, because a `collect` diagnostic means the
// `Program` may be inconsistent and emit indexes it directly (#205, #501).
// `Generics` and `Pools` defer instead. `progress_observer.rs` asserts the
// phase sequence for both halves; this file asserts what a reader actually
// sees.

mod common;
use common::ozobject_src as PREAMBLE;

/// The shape #540 reported: a `-run` collision in one pair of classes, and
/// a `@try` in a third that has nothing to do with it.
///
/// Before, one build showed only the collision.
#[test]
fn a_collision_no_longer_hides_an_unrelated_refusal() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Alpha : OZObject
- (void)run;
@end
@implementation Alpha
- (void)run { }
@end

@interface Beta : OZObject
- (int)run;
- (int)guarded;
@end
@implementation Beta
- (int)run { return 1; }
- (int)guarded
{
	@try {
		return 1;
	} @catch (id e) {
		return 0;
	}
}
@end
"
    );
    let diags = oz2c::transpile(&src).err().expect("both problems must be refused");
    let text = diags.iter().map(|d| format!("{}", d)).collect::<Vec<_>>().join("\n");

    assert!(
        text.contains("Alpha returns 'void' and Beta returns 'int'"),
        "the collision must still be reported:\n{}",
        text
    );
    assert!(
        text.contains("@try/@catch is not supported"),
        "the per-site refusal must be reported in the SAME build -- this is the whole \
         of #540:\n{}",
        text
    );
    assert!(
        diags.len() >= 2,
        "two independent problems, two diagnostics; got {}:\n{}",
        diags.len(),
        text
    );
}

/// Order matters to a reader: pipeline order, earliest cause first.
///
/// Asserted because the merge could as easily have put emit's first, and
/// then the message a consumer reads on line one would be the *later* of
/// two unrelated causes.
#[test]
fn the_front_ends_diagnostics_come_first() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Alpha : OZObject
- (void)run;
@end
@implementation Alpha
- (void)run { }
@end

@interface Beta : OZObject
- (int)run;
- (int)guarded;
@end
@implementation Beta
- (int)run { return 1; }
- (int)guarded
{
	@try {
		return 1;
	} @catch (id e) {
		return 0;
	}
}
@end
"
    );
    let diags = oz2c::transpile(&src).err().expect("refused");
    let first = format!("{}", diags[0]);
    assert!(
        first.contains("dispatched dynamically"),
        "the generics refusal is earlier in the pipeline, so it reports first:\n{}",
        first
    );
}

/// The control, and the one that keeps this from being a licence to run
/// emit over anything: a **collect** refusal still stops the pipeline, so
/// emit never walks a `Program` whose superclass strings may not be keys.
///
/// If this ever reports a second diagnostic from a later pass, the gate has
/// been widened past what is safe -- see #205 and #501, where exactly that
/// produced an unlocated panic naming neither the class nor the file.
#[test]
fn a_collect_refusal_still_stops_before_emit() {
    /* No preamble on purpose: `OZObject` is undeclared, which `collect`
     * refuses. */
    let src = "@interface Orphan : OZObject\n- (void)greet;\n@end\n\
               @implementation Orphan\n- (void)greet {\n}\n@end\n";
    let diags = oz2c::transpile(src).err().expect("refused");
    let text = diags.iter().map(|d| format!("{}", d)).collect::<Vec<_>>().join("\n");
    assert!(
        text.contains("no class 'OZObject' is defined"),
        "diagnostics:\n{}",
        text
    );
    assert!(
        !text.contains("@try/@catch"),
        "nothing from a later pass may appear -- emit must not have run:\n{}",
        text
    );
}
