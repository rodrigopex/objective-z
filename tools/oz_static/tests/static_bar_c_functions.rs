// SPDX-License-Identifier: Apache-2.0
//
// static_bar_c_functions.rs -- the accept/reject scan over plain C function
// bodies (#234, case 1).
//
// `staticbar::check_method_body` was entered from exactly one place: the
// `@implementation` method-body renderer in `emit.rs`. A `.m` file's
// file-scope functions -- `main()` above all -- are rendered by a different
// path and were never scanned, so *every* check was skipped there: `@try`,
// reflection selectors, `@selector`/`@protocol`, `@synchronized` bodies with
// an escaping jump, block captures of stack locals, and the allocation rule.
//
// How this was found is worth keeping: the first draft of OZ-098's
// collection-literal loop test (then named `array_literal_escaping_a_loop_
// rejected`, since reshaped into
// `static_bar_rejects::array_literal_accumulated_in_a_loop_rejected`) put the
// offending loop in `main()` and passed vacuously. Moving it into a method
// made it reject as intended.
//
// Each test below pairs a free function with the same construct in a method,
// so what is being asserted is that the two now agree -- not merely that some
// diagnostic appeared.

mod common;
use common::{expect_reject, ozobject_src as PREAMBLE};

fn transpiles(src: &str) -> bool {
    oz_static::transpile(src).is_ok()
}

/// `@try` in `main()` used to be accepted silently and then reach `emit`,
/// which has no lowering for it.
#[test]
fn try_catch_in_a_c_function_rejected() {
    let src = format!(
        "{}\nint main(void)\n{{\n\t@try {{\n\t\tint x = 1;\n\t}} @catch (id e) {{\n\t}}\n\treturn 0;\n}}\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@try/@catch"), "diagnostics: {}", diags);
}

/// Reflection in a free function.
///
/// `@selector` is supported now (#226) -- it resolves to a generated
/// selector record -- so this no longer asserts that it has no lowering.
/// What it still asserts is the thing this file exists for: that the scan
/// reaches a free function's body at all. With `CONFIG_OBJZ_REFLECTION`
/// off the construct is refused, and the diagnostic naming the option has
/// to come from `main()` just as it would from a method.
#[test]
fn selector_expression_in_a_c_function_rejected() {
    let src = format!(
        "{}\nint main(void)\n{{\n\tSEL s = @selector(poke);\n\t(void)s;\n\treturn 0;\n}}\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("CONFIG_OBJZ_REFLECTION"), "diagnostics: {}", diags);
}

/// `@protocol(...)` parses as a generic `at_expression` in
/// tree-sitter-objc 3.0.2, so it reaches a different arm than `@selector`
/// and is worth its own case in this position too.
///
/// Still rejected after #226, but now for a different reason: a protocol
/// has no value representation, so `@protocol(...)` resolves to a
/// conformance bitmap and is accepted only as `-conformsToProtocol:`'s
/// argument. Assigning one to an `id`, as here, is out of position
/// wherever it appears -- which is why this case survives unchanged while
/// its `@selector` sibling above had to be rewritten.
#[test]
fn protocol_expression_in_a_c_function_rejected() {
    let src = format!(
        "{}\n@protocol Pokeable\n- (void)poke;\n@end\n\
         int main(void)\n{{\n\tid p = @protocol(Pokeable);\n\t(void)p;\n\treturn 0;\n}}\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@protocol(...)"), "diagnostics: {}", diags);
}

/// The rule with a *silent* consequence, and the reason #234 mattered.
///
/// Pool sizing counts an allocation site once however many times it runs
/// (`pools::count_sites`). Accumulating into a C array of pointers keeps every
/// instance live, so the count is a floor the program walks straight through
/// -- and it surfaced at run time as an unexpected nil, not at build time as a
/// diagnostic. In a method this was already rejected; in `main()` it was not.
#[test]
fn accumulating_alloc_in_a_loop_in_a_c_function_rejected() {
    let free_fn = format!(
        "{}\
@interface Counter : OZObject
@end
@implementation Counter
@end

int main(void)
{{
	Counter *kept[100];
	for (int i = 0; i < 100; i++) {{
		kept[i] = [Counter alloc];
	}}
	return kept[0] != 0;
}}
",
        PREAMBLE()
    );
    let diags = expect_reject(&free_fn);
    assert!(diags.contains("escapes the iteration"), "diagnostics: {}", diags);

    // The same construct in a method, to show the two positions agree
    // rather than merely that the free function rejected something.
    let in_method = format!(
        "{}\
@interface Counter : OZObject
- (BOOL)run;
@end
@implementation Counter
- (BOOL)run {{
	Counter *kept[100];
	for (int i = 0; i < 100; i++) {{
		kept[i] = [Counter alloc];
	}}
	return kept[0] != 0;
}}
@end
int main(void) {{ return 0; }}
",
        PREAMBLE()
    );
    let method_diags = expect_reject(&in_method);
    assert!(method_diags.contains("escapes the iteration"), "diagnostics: {}", method_diags);
}

/// The scan must not become over-broad now that it reaches free functions.
/// A fresh per-iteration local is released at the end of each iteration, so
/// one slot is enough and this has always been legitimate -- in `main()` as
/// much as in a method. Every existing sample is built on this shape.
#[test]
fn fresh_per_iteration_local_in_a_c_function_accepted() {
    let src = format!(
        "{}\
@interface Counter : OZObject
@end
@implementation Counter
@end

int main(void)
{{
	for (int i = 0; i < 100; i++) {{
		Counter *c = [Counter alloc];
		(void)c;
	}}
	return 0;
}}
",
        PREAMBLE()
    );
    assert!(transpiles(&src), "a fresh per-iteration local must stay accepted in main()");
}

/// Reassigning a strong local in a free function is bounded by ARC exactly as
/// it is in a method, so extending the scan must not reject it. This is the
/// shape #234's own reproducer used.
#[test]
fn reassigned_strong_local_in_a_c_function_accepted() {
    let src = format!(
        "{}\
@interface Counter : OZObject
@end
@implementation Counter
@end

int main(void)
{{
	Counter *c;
	for (int i = 0; i < 100; i++) {{
		c = [Counter alloc];
	}}
	return c != 0;
}}
",
        PREAMBLE()
    );
    assert!(transpiles(&src), "ARC bounds this at one live instance; it must be accepted");
}

/// A block in a free function capturing one of that function's stack locals.
/// Blocks are hoisted to plain C functions, which have no closure to carry a
/// captured stack variable in.
#[test]
fn block_capturing_a_stack_local_in_a_c_function_rejected() {
    let src = format!(
        "{}\nint main(void)\n{{\n\tint n = 1;\n\tint (^f)(void) = ^{{ return n; }};\n\treturn f();\n}}\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("capture"), "diagnostics: {}", diags);
}

/// A `break` escaping a `@synchronized` body would skip the unlock and leave
/// the lock held. `@synchronized` lowers to an explicit lock/unlock pair, not
/// a scope guard, so this is a deadlock rather than a style question.
#[test]
fn break_escaping_synchronized_in_a_c_function_rejected() {
    let src = format!(
        "{}\
@interface Counter : OZObject
@end
@implementation Counter
@end

int main(void)
{{
	Counter *c = [Counter alloc];
	for (int i = 0; i < 3; i++) {{
		@synchronized (c) {{
			break;
		}}
	}}
	return 0;
}}
",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@synchronized"), "diagnostics: {}", diags);
}

/// A block in a free function capturing a *file-scope* variable is not a
/// capture at all, and must not be flagged. This is `samples/gpio_demo`'s
/// shape -- `static GPIOOutput *led;` with `[led toggle]` inside a block in
/// `main` -- and it is the case that would break if the free-function scan
/// were given some nearby class's ivar names instead of an empty set.
#[test]
fn block_capturing_a_file_scope_static_in_a_c_function_accepted() {
    let src = format!(
        "{}\
@interface Counter : OZObject
- (void)poke;
@end
@implementation Counter
- (void)poke {{ }}
@end

static Counter *g_counter;

int main(void)
{{
	g_counter = [Counter alloc];
	void (^f)(void) = ^{{ [g_counter poke]; }};
	f();
	return 0;
}}
",
        PREAMBLE()
    );
    assert!(
        transpiles(&src),
        "a file-scope static is not a stack capture and must stay accepted"
    );
}
