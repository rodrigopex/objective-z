// SPDX-License-Identifier: Apache-2.0
//
// goto_scope_releases.rs -- the fourth jump, and the last one to get an
// arm (#454).
//
// Every ARC release here is emitted explicitly at each exit, because there
// is no `__attribute__((cleanup))` and no unwinding. So each kind of exit
// needs its own arm: normal scope end (`arc_exit`), `return`
// (`render_return_statement`), `break` and `continue`
// (`render_loop_jump`). `goto` had none.
//
// Worse than merely missing: `is_jump_statement` has counted
// `goto_statement` all along, and that *suppresses* the trailing scope
// release on the reasoning that a jump emits its own. True of the other
// three, false of `goto` -- so the release was suppressed and never
// replaced.
//
// And `needs_translation` did not list it, which is the half that made the
// arm unreachable for the spelling that matters. `if (n) { goto done; }`
// is an Objective-C-free subtree, so it was copied verbatim and no
// renderer saw the jump at all. Exactly #283's shape one jump kind later:
// that issue added `return_statement` to the same list after LeakSanitizer
// found a `return` nested in an ObjC-free `if` skipping its loop's
// release.
//
// **Two things I expected to be true here were not, and both were settled
// by measurement rather than argument.**
//
// I expected this to need a scope *graph* rather than the emitter's
// `Vec<ArcScope>` stack. It does not. `goto_statement` carries its label
// as a `statement_identifier` and `labeled_statement` carries the
// matching one, so the release set is "live scopes that do not also
// contain the label" -- `releases_up_to_jump_target`'s existing
// comparison with a different target search.
//
// I also expected a backward `goto` to be a hazard, because it forms a
// loop `staticbar::LOOP_KINDS` cannot see (it lists `for_statement`,
// `while_statement`, `do_statement`), so the loop-escape bar never asks
// whether the reference outlives the iteration. Measured, there is
// nothing there: a scope that allocates and closes inside the loop body
// releases per iteration, so one slab slot suffices and the emitted C was
// already correct. `a_backward_goto_forming_a_loop_needs_one_slot` is
// that non-finding, pinned so nobody re-derives it.
//
// Clang refuses the two shapes that would be genuinely hard -- jumping
// *into* a scope and jumping *over* a declaration -- with `cannot jump
// from this goto statement to its label` (ARC § 2.6.6), verified against
// this project's own flags. So what reaches the emitter is exactly what
// this file covers.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

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

/// A forward `goto` out of a scope holding an owned local releases it.
///
/// The defect's own shape, and note where the `goto` sits: inside
/// `if (n) { ... }`, an ObjC-free subtree. That is not incidental -- it is
/// the spelling that made the arm unreachable, so a fixture with the
/// `goto` at the scope's top level would pass without the
/// `needs_translation` half of the fix.
#[test]
fn a_forward_goto_releases_the_scope_it_leaves() {
    let src = format!(
        "/* oz-pool: Thing=1,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (int)jumpOut;
@end
@implementation P
- (int)jumpOut {
	int n = 0;
	{
		Thing *t = [[Thing alloc] init];
		n = [t tag];
		if (n) {
			goto done;
		}
	}
done:
	return n;
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int v = [p jumpOut];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "forward_goto_releases_scope");
    assert_eq!(
        out, "v=7 deallocs=1\n",
        "the jumped-out-of scope must release its local exactly once: {}",
        out
    );
}

/// The fall-through path still releases exactly once.
///
/// The other half of the same scope: with the `goto` not taken, the
/// release at the scope's end is the one that runs. A fix that moved the
/// release to the jump instead of adding one would pass the test above and
/// leak here.
#[test]
fn the_path_that_does_not_jump_still_releases_once() {
    let src = format!(
        "/* oz-pool: Thing=1,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (int)maybeJump:(int)take;
@end
@implementation P
- (int)maybeJump:(int)take {
	int n = 0;
	{
		Thing *t = [[Thing alloc] init];
		n = [t tag];
		if (take) {
			goto done;
		}
	}
done:
	return n;
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int a = [p maybeJump:0];
	int after_fallthrough = g_deallocs;
	int b = [p maybeJump:1];
	printf(\"a=%d b=%d fallthrough=%d total=%d\\n\",
	       a, b, after_fallthrough, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "goto_not_taken_still_releases");
    /* One dealloc per call whichever path ran: the fall-through releases at
     * the scope's end, the jump releases before leaving. Two calls, two
     * deallocs, and a one-slot pool proves the first was returned before
     * the second allocated. */
    assert_eq!(
        out, "a=7 b=7 fallthrough=1 total=2\n",
        "both paths out of the scope must release exactly once: {}",
        out
    );
}

/// A `goto` that leaves *two* nested scopes releases both, innermost
/// first.
///
/// The release set is a set, not a single local, and the order matters for
/// the same reason `arc_exit` releases in reverse declaration order: a
/// later local may hold a reference to an earlier one.
#[test]
fn a_goto_leaving_two_scopes_releases_both() {
    let src = format!(
        "/* oz-pool: Thing=2,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (int)jumpOutOfTwo;
@end
@implementation P
- (int)jumpOutOfTwo {
	int n = 0;
	{
		Thing *outer = [[Thing alloc] init];
		n += [outer tag];
		{
			Thing *inner = [[Thing alloc] init];
			n += [inner tag];
			if (n) {
				goto done;
			}
		}
	}
done:
	return n;
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int v = [p jumpOutOfTwo];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "goto_leaving_two_scopes");
    assert_eq!(
        out, "v=14 deallocs=2\n",
        "both scopes the jump leaves must release: {}",
        out
    );
}

/// A `goto` that leaves *no* scope releases nothing, and the output stays
/// byte-identical.
///
/// This is the shape the tree actually contains --
/// `samples/zbus_service/src/TemperatureService.m:39` jumps to a
/// `cleanup:` label from a plain C function whose locals are all C
/// structs. It was the one `goto` in any `.m` when this was written, and
/// it owes no release, which is why the defect had never been observed.
#[test]
fn a_goto_owing_nothing_emits_nothing() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (int)plainJump:(int)c;
@end
@implementation P
- (int)plainJump:(int)c {
	int n = 0;
	if (c) {
		goto done;
	}
	n = 1;
done:
	return n;
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("inside the static subset");
    let body = one_function(&out.source_c, "int P_plainJump_");
    /* Positively first, because an assertion of *absence* cannot tell
     * "correctly emitted nothing" from "never emitted at all" -- and after
     * the #462 respelling it could not even tell the symbol apart: this
     * test asserted the absence of the *pre-rename* release symbol, and
     * once the emitted name became `oz_release` that assertion was true
     * however the emitter behaved. A negative assertion against a renamed
     * symbol goes vacuous silently, and stays green while doing it.
     *
     * The retired spelling is deliberately not written here:
     * `naming_tool_identity.rs` greps the whole tree for it and exempts
     * only `docs/STATUS.md`, so prose that quotes it belongs in that
     * archive and not in a test. That guard caught this very comment.
     *
     * So: prove this is the right function and the jump survived, and only
     * then claim the absence. `one_function` panicking covers the
     * never-emitted case; this covers the wrong-function and
     * lost-the-goto cases. */
    assert!(
        body.contains("goto done"),
        "the fixture's goto must survive into this function, or the absence \
         claim below is about the wrong code:\n{}",
        body
    );
    assert!(
        !body.contains("oz_release"),
        "a goto that leaves no owned local must emit no release:\n{}",
        body
    );
}

/// A backward `goto` forming a loop needs one slab slot, not N.
///
/// **A non-finding, pinned deliberately.** I expected this to be a hazard:
/// `staticbar::LOOP_KINDS` is `["for_statement", "while_statement",
/// "do_statement"]`, so the loop-escape bar cannot see a loop built from a
/// backward `goto`, and `pools.rs` counts one slot per allocation site
/// without asking whether the reference outlives the iteration.
///
/// It is not a hazard, because the scope that allocates also closes inside
/// the loop body, so the slot is returned before the next iteration asks.
/// A one-slot pool running three iterations is the proof: a second slot
/// would be needed only if the first instance outlived its scope.
///
/// Kept so that the reasoning is on record rather than re-derived, and so
/// that a future change to `ArcScope` or to the loop bar that *does* break
/// it fails here rather than in a sample.
#[test]
fn a_backward_goto_forming_a_loop_needs_one_slot() {
    let src = format!(
        "/* oz-pool: Thing=1,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (int)backwardLoop;
@end
@implementation P
- (int)backwardLoop {
	int i = 0;
	int sum = 0;
again:
	{
		Thing *t = [[Thing alloc] init];
		sum += [t tag];
	}
	if (++i < 3) {
		goto again;
	}
	return sum;
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int v = [p backwardLoop];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "backward_goto_one_slot");
    /* Three iterations, three allocations, three deallocs, one slot. If the
     * slot were not returned each time the second `Thing_oz_alloc` would
     * hand back nil and `-tag` would fault. */
    assert_eq!(
        out, "v=21 deallocs=3\n",
        "a goto-loop must release per iteration on a one-slot pool: {}",
        out
    );
}

/// A backward `goto` from *inside* a scope releases it before jumping.
///
/// The direction the one rule has to cover as well: the label precedes the
/// `goto`, so the innermost ancestor spanning both is the function body,
/// and every scope opened inside it is left.
#[test]
fn a_backward_goto_out_of_a_scope_releases_it() {
    let src = format!(
        "/* oz-pool: Thing=1,P=1 */\n{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface P : OZObject
- (int)jumpBackOut;
@end
@implementation P
- (int)jumpBackOut {
	int i = 0;
	int sum = 0;
again:
	{
		Thing *t = [[Thing alloc] init];
		sum += [t tag];
		if (++i < 3) {
			goto again;
		}
	}
	return sum;
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int v = [p jumpBackOut];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "backward_goto_out_of_scope");
    /* The jump leaves the scope holding `t`, so it must release before
     * going back -- otherwise the one slot is still occupied when the next
     * iteration allocates. Three iterations, three deallocs. */
    assert_eq!(
        out, "v=21 deallocs=3\n",
        "a backward goto must release the scope it leaves: {}",
        out
    );
}

/// One generated function's text, by brace matching, skipping prototypes.
fn one_function(c: &str, signature: &str) -> String {
    let start = c
        .match_indices(signature)
        .find(|(at, _)| {
            c[*at..]
                .find(|ch| ch == '{' || ch == ';')
                .is_some_and(|k| c[*at..].as_bytes()[k] == b'{')
        })
        .map(|(at, _)| at)
        .unwrap_or_else(|| panic!("no definition of {:?} in:\n{}", signature, c));
    let rest = &c[start..];
    let mut depth = 0usize;
    let mut opened = false;
    for (i, ch) in rest.char_indices() {
        match ch {
            '{' => {
                depth += 1;
                opened = true;
            }
            '}' => {
                depth -= 1;
                if opened && depth == 0 {
                    return rest[..=i].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after {:?}", signature)
}
