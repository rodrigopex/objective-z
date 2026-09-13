// SPDX-License-Identifier: Apache-2.0
//
// literal_element_ownership.rs -- a collection literal takes over a `+1`
// element rather than retaining it again (#449).
//
// A boxed `@[...]` or `@{...}` owns its elements: `OZArray_oz_free`
// releases each one. So each element is either passed through as an
// existing `+1` or retained first, and `emit::is_fresh_alloc` decided
// which. It matched four node kinds -- a numeric `at_expression`, a boxed
// `string_literal`, a nested `array_literal` and a nested
// `dictionary_literal` -- and answered `false` for everything else.
//
// `@[[Thing alloc]]` is everything else. `Thing_oz_alloc()` is already
// `+1`, the retain took it to `+2`, and the array released one of the two
// at its own teardown. **One reference leaked per element**, and the same
// for a dictionary's keys and values.
//
// The eleventh ownership decision keyed on a syntactic form, and the form
// here is the element's node kind. What makes it the familiar shape rather
// than an oversight is the reason that was written down: the predicate's
// doc said it drew the same line as the retired Python oracle "for the
// same reason: it has no general-purpose ownership analysis either".
// oz2c does have one. The reason expired and the line did not move
// with it -- so the fix is to ask `arc::binds_ownership`, the predicate
// every other binding site already consults.
//
// The four literal kinds stay in `is_fresh_alloc` deliberately: a nested
// literal is desugared *by this emitter* into a builder call, so its `+1`
// exists nowhere `arc.rs` can see. That part is genuinely this function's
// own knowledge.
//
// **Why these tests count deallocs rather than grepping for a retain.**
// The defect is a retain that should not be there, so the tempting
// assertion is `!body.contains("oz_retain")`. That is an absence claim,
// and today has produced four cases where an absence claim passed while
// the property was gone -- including one in this crate that went vacuous
// across a symbol rename. A dealloc counter asks whether the object
// actually died, which is the question, and a one-slot pool proves the
// slot came back.

mod common;
use common::{
    compile_and_run, ozarray_src, ozdictionary_src, oznumber_src, ozobject_src as PREAMBLE,
    ozstring_src,
};

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

/// An array element that is already `+1` is taken over, not retained.
///
/// The defect's own shape. With the extra retain the element reaches `+2`,
/// the array releases one at teardown, and the object never deallocs --
/// which a one-slot pool turns from a leak into a visible failure on the
/// second allocation.
#[test]
fn an_array_element_from_a_send_is_taken_over() {
    let src = format!(
        "/* oz-pool: Thing=1,P=1,OZArray=2 */\n{}{}{}{}",
        PREAMBLE(),
        ozarray_src(),
        DECLS,
        "\
@interface P : OZObject
- (int)once;
@end
@implementation P
- (int)once {
	OZArray *a = @[[[Thing alloc] init]];
	return (a != nil);
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int first = [p once];
	int after_first = g_deallocs;
	/* A one-slot Thing pool: this only succeeds if the first element's
	 * slot came back, which needs the array's release to have been the
	 * only one owed. */
	int second = [p once];
	printf(\"first=%d second=%d after_first=%d total=%d\\n\",
	       first, second, after_first, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "array_element_from_send");
    assert_eq!(
        out, "first=1 second=1 after_first=1 total=2\n",
        "the array must own the element's existing +1, not a second reference: {}",
        out
    );
}

/// A *borrowed* element is still retained, which is the other half.
///
/// A fix that simply stopped retaining would pass the test above and leave
/// the array holding a reference it does not own -- the corrupting
/// direction. The caller's object must outlive the array here.
#[test]
fn a_borrowed_array_element_is_still_retained() {
    let src = format!(
        "/* oz-pool: Thing=1,P=1,OZArray=2 */\n{}{}{}{}",
        PREAMBLE(),
        ozarray_src(),
        DECLS,
        "\
@interface P : OZObject
- (int)hold:(Thing *)t;
@end
@implementation P
- (int)hold:(Thing *)t {
	OZArray *a = @[t];
	return [t tag] + (a != nil);
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	Thing *mine = [[Thing alloc] init];
	int v = [p hold:mine];
	/* The array was built and released inside -hold:. `mine` is still
	 * the caller's and must be alive: if the array had taken over the
	 * reference instead of retaining it, this reads freed memory and
	 * g_deallocs is already 1. */
	int after = g_deallocs;
	printf(\"v=%d after=%d tag=%d\\n\", v, after, [mine tag]);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "borrowed_array_element_retained");
    assert_eq!(
        out, "v=8 after=0 tag=7\n",
        "a borrowed element must be retained, so the caller's object survives: {}",
        out
    );
}

/// A dictionary's **values** take over a `+1` too.
///
/// The dictionary path is a separate loop over keys and values, and it
/// asked the same predicate -- so it had the same defect twice per pair.
/// Enumerating the sibling rather than fixing only the reported site is
/// this project's standing rule.
#[test]
fn a_dictionary_value_from_a_send_is_taken_over() {
    let src = format!(
        "/* oz-pool: Thing=1,P=1,OZDictionary=2,OZString=4 */\n{}{}{}{}{}",
        PREAMBLE(),
        ozstring_src(),
        ozdictionary_src(),
        DECLS,
        "\
@interface P : OZObject
- (int)once;
@end
@implementation P
- (int)once {
	OZDictionary *d = @{@\"k\": [[Thing alloc] init]};
	return (d != nil);
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int first = [p once];
	int after_first = g_deallocs;
	int second = [p once];
	printf(\"first=%d second=%d after_first=%d total=%d\\n\",
	       first, second, after_first, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "dict_value_from_send");
    assert_eq!(
        out, "first=1 second=1 after_first=1 total=2\n",
        "a dictionary must own its value's existing +1: {}",
        out
    );
}

/// An element produced by an analysis-derived **factory** is taken over as
/// well.
///
/// `arc::binds_ownership` recognises a plain C function whose every return
/// path is `+1` (`arc::analyze`'s `functions` set), which no node-kind test
/// ever could. This is the case that shows the fix delegates rather than
/// enumerating a longer list of shapes.
#[test]
fn an_array_element_from_a_c_factory_is_taken_over() {
    let src = format!(
        "/* oz-pool: Thing=1,P=1,OZArray=2 */\n{}{}{}{}",
        PREAMBLE(),
        ozarray_src(),
        DECLS,
        "\
static Thing *makeThing(void)
{
	return [[Thing alloc] init];
}

@interface P : OZObject
- (int)once;
@end
@implementation P
- (int)once {
	OZArray *a = @[makeThing()];
	return (a != nil);
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int first = [p once];
	int after_first = g_deallocs;
	int second = [p once];
	printf(\"first=%d second=%d after_first=%d total=%d\\n\",
	       first, second, after_first, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "array_element_from_c_factory");
    assert_eq!(
        out, "first=1 second=1 after_first=1 total=2\n",
        "an analysis-derived factory's +1 must be taken over: {}",
        out
    );
}

/// A nested literal still passes through, and this is the row that keeps
/// the four literal kinds in `is_fresh_alloc`.
///
/// A nested `@[...]` is desugared by the emitter into a builder call, so
/// its `+1` exists nowhere `arc.rs` can see -- `binds_ownership` answers
/// `false` for it. Delegating *everything* would therefore have retained
/// it and leaked the inner array, which is the mistake available to
/// someone simplifying this predicate later.
#[test]
fn a_nested_literal_element_still_passes_through() {
    let src = format!(
        "/* oz-pool: Thing=1,P=1,OZArray=4,OZNumber=4 */\n{}{}{}{}{}",
        PREAMBLE(),
        oznumber_src(),
        ozarray_src(),
        DECLS,
        "\
@interface P : OZObject
- (int)nested;
@end
@implementation P
- (int)nested {
	OZArray *outer = @[@[@1, @2]];
	return (outer != nil);
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int v = [p nested];
	printf(\"v=%d\\n\", v);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "nested_literal_passes_through");
    assert_eq!(
        out, "v=1\n",
        "a nested literal is the emitter's own +1 and must pass through: {}",
        out
    );
}

/// **A known gap, asserted to stay broken.** A conditional element still
/// gets a retain, and the reason is not #449's.
///
/// This is the `KNOWN_DEFECTS` pattern `ownership_matrix.rs` uses: the
/// defective behaviour is pinned so that fixing it *fails this test* and
/// forces the row to be updated, rather than leaving a hole nobody can
/// find again.
///
/// The two defects compose and are worth telling apart, because the fix
/// for one is not the fix for the other:
///
///   * **#449 — which predicate the site asks.** Fixed here. The element
///     slot used to consult a node-kind whitelist; it now consults
///     `arc::binds_ownership`, like every other binding site.
///   * **The `conditional_expression` gap — what the predicate
///     recognises.** `conditional_expression` appears **zero** times in
///     all of `arc.rs`: `is_owning_expr` has no arm for it, and
///     `hoists_owning_operand` returns early unless the kind is
///     `message_expression` or `call_expression`. So a `+1` in a ternary
///     is invisible in *value* position too --
///     `Thing *t = c ? [[Thing alloc] init] : b;` emits no release at all,
///     which is the more serious half and is filed separately.
///
/// Measured rather than reasoned: with a conditional element this site
/// emits `oz_retain(c ? ... : b)`, so the `+1` branch reaches `+2` and
/// leaks one reference while the borrowed branch is correctly retained.
///
/// **That #449's fix inherits this gap is the argument for its shape.**
/// Delegating to `binds_ownership` means this site gets every future
/// improvement to that predicate for free -- when the ternary arm lands,
/// this test goes red and the element slot is already correct. The old
/// whitelist would have needed its own ternary case added by hand, which
/// is how a whitelist accumulates eleven instances of the same bug.
#[test]
fn a_conditional_element_is_still_retained_and_that_is_not_this_issue() {
    let src = format!(
        "{}{}{}{}",
        PREAMBLE(),
        ozarray_src(),
        DECLS,
        "\
@interface P : OZObject
- (int)pick:(int)c other:(Thing *)b;
@end
@implementation P
- (int)pick:(int)c other:(Thing *)b {
	OZArray *a = @[c ? [[Thing alloc] init] : b];
	return (a != nil);
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("inside the static subset");
    let body = one_function(&out.source_c, "int P_pick_other_");
    /* Positively, so the assertion cannot pass by looking at the wrong
     * function or by the symbol having been renamed -- today produced four
     * cases where an absence claim held while the property was gone. */
    assert!(
        body.contains("oz_retain"),
        "the conditional element is expected to STILL be retained until the \
         ternary arm lands in arc.rs. If this now fails, that gap is fixed and \
         this test should become a passing case rather than a pinned gap:\n{}",
        body
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
