// SPDX-License-Identifier: Apache-2.0
//
// ozm_escape.rs -- `OZM(MACRO, a, b)` becomes `MACRO(a, b)` (#272).
//
// A target's static definition macros consume their callback as a function
// pointer -- `K_TIMER_DEFINE(name, expiry_fn, stop_fn)`,
// `ZBUS_LISTENER_DEFINE(name, callback)` -- and Objective-C refuses
// block-to-function-pointer conversion in every position, by cast or by
// initialization, with ARC or without:
//
//     error: initializing 'void (*)(int)' with an expression of
//            incompatible type 'void (^)(int)'
//
// So an inline block cannot be handed to one directly, and Clang is not
// optional here: `cmake/oz_static.cmake` dumps one AST per source as the
// ownership oracle (PARITY.md gap N), and the outgoing Python backend
// compiles the same file. Worse, a source Clang rejects fails *silently* --
// the dump is taken with `2>/dev/null || true` -- so the file would quietly
// lose its ARC facts and leak its `id` ivars with a green build.
//
// **A macro is the only construct whose argument Objective-C leaves
// unparsed.** An argument whose parameter is absent from the replacement
// list is discarded rather than expanded, so it need only lex, and `^` is a
// valid punctuator. `include/oz_sdk/Foundation/OZMacro.h` therefore defines
// `OZM(...)` as empty for Clang, and this rewrite puts the real call back
// for the C compiler -- by which point `top_level_block_edits` has turned
// the literal into the name of a function hoisted out of it.
//
// One name serves every target macro, so there is no per-primitive wrapper
// to write and no second arm to keep in step, and the call site still names
// the macro it means.

mod common;
use common::{compile_and_run, ozobject_src};

/// A stand-in with `K_TIMER_DEFINE`'s shape: a definition macro that stores
/// a callback in a function-pointer field of a static initializer, which is
/// the position Objective-C will not accept a block in. Zephyr's own macro
/// is `STRUCT_SECTION_ITERABLE(k_timer, name) = Z_TIMER_INITIALIZER(...)`,
/// whose `.expiry_fn = expiry` needs an address constant -- which a hoisted
/// function's name is.
const TARGET: &str = "\
#include <stdio.h>
struct fake_timer {
	void (*expiry_fn)(int);
};
#define FAKE_TIMER_DEFINE(name, exp) \\
	static struct fake_timer name = { .expiry_fn = (exp) }
";

/// The shape this exists for, run end to end.
#[test]
fn ozm_carries_an_inline_block_to_the_target_macro() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        TARGET,
        "\
OZM(FAKE_TIMER_DEFINE, my_timer, ^(int v) {
	printf(\"expiry=%d\\n\", v);
});

int main(void) {
	my_timer.expiry_fn(7);
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("FAKE_TIMER_DEFINE(my_timer, oz_block_"),
        "OZM should become the target macro, taking the hoisted name:\n{}",
        out.source_c
    );
    assert!(
        !out.source_c.contains("OZM("),
        "no OZM invocation may survive into the generated C:\n{}",
        out.source_c
    );
    let stdout = compile_and_run(&src, "ozm_carries_an_inline_block_to_the_target_macro");
    assert_eq!(stdout, "expiry=7\n");
}

/// An unprotected comma in the block body must not split the argument list.
/// `OZM` is variadic on the Objective-C side for exactly this, and the
/// rewrite counts bracket depth rather than commas.
#[test]
fn a_comma_inside_the_block_body_does_not_split_the_call() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        TARGET,
        "\
OZM(FAKE_TIMER_DEFINE, my_timer, ^(int v) {
	int a = 1, b = 2;
	printf(\"sum=%d\\n\", v + a + b);
});

int main(void) {
	my_timer.expiry_fn(7);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "a_comma_inside_the_block_body_does_not_split_the_call");
    assert_eq!(stdout, "sum=10\n");
}

/// A plain function name rather than a literal: `OZM` is a macro escape, not
/// a block feature, and must not require one.
#[test]
fn ozm_works_without_a_block_at_all() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        TARGET,
        "\
static void on_expiry(int v) { printf(\"fn=%d\\n\", v); }

OZM(FAKE_TIMER_DEFINE, my_timer, on_expiry);

int main(void) {
	my_timer.expiry_fn(3);
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("FAKE_TIMER_DEFINE(my_timer, on_expiry)"),
        "the rewrite should be independent of what the arguments are:\n{}",
        out.source_c
    );
    let stdout = compile_and_run(&src, "ozm_works_without_a_block_at_all");
    assert_eq!(stdout, "fn=3\n");
}

/// `OZM(MACRO)` with nothing after the name still means `MACRO()`.
#[test]
fn ozm_with_no_further_arguments_calls_the_macro_with_none() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>
#define DECLARE_FLAG() static int oz_flag = 5
OZM(DECLARE_FLAG);

int main(void) {
	printf(\"flag=%d\\n\", oz_flag);
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("DECLARE_FLAG()"),
        "an argument-less OZM should still call its macro:\n{}",
        out.source_c
    );
    let stdout = compile_and_run(&src, "ozm_with_no_further_arguments");
    assert_eq!(stdout, "flag=5\n");
}

/// A longer name beginning with those three letters is not `OZM`. The
/// rewrite matches the identifier followed by its opening paren, so a macro
/// somebody else called `OZMETRICS_DEFINE` is left entirely alone -- the
/// same discipline `unused_param_acks` needed for parameter names.
///
/// The one case in this file that passes with the rewrite disabled, and
/// deliberately so: it guards against *over*-matching, a failure only the
/// rewrite can introduce. Read it as a bound on the feature rather than as
/// a regression test for it -- the other four are the discriminating ones,
/// each confirmed to fail without the change.
#[test]
fn a_longer_name_beginning_with_ozm_is_untouched() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>
#define OZMETRICS_DEFINE(name) static int name = 4
OZMETRICS_DEFINE(counter);

int main(void) {
	printf(\"c=%d\\n\", counter);
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("OZMETRICS_DEFINE(counter)"),
        "OZMETRICS_DEFINE must survive verbatim:\n{}",
        out.source_c
    );
    let stdout = compile_and_run(&src, "a_longer_name_beginning_with_ozm_is_untouched");
    assert_eq!(stdout, "c=4\n");
}
