// SPDX-License-Identifier: Apache-2.0
//
// bare_macro_statements.rs -- a top-level function-like macro invocation
// written without a trailing `;` must not absorb the construct after it
// (#288, #289).
//
// `ZBUS_OBS_DECLARE(x)` terminates its own expansion, so it is spelled
// without a semicolon: that is Zephyr's idiom, what
// `include/oz_sdk/Foundation/OZMacro.h` documents, and what
// `samples/zbus_service/src/main.m` writes. To tree-sitter the invocation
// then reads as a *type*, the next construct becomes the declarator of a
// function definition, and everything up to the following `{ ... }` is
// swallowed into one node -- so that construct never reaches its own arm
// in `emit::walk_top_level`.
//
// One cause, and the victim is whatever came next:
//
//   - an `@implementation`, emitted verbatim as Objective-C (#288)
//   - a second `OZM`, whose block literal survives at its call site (#289)
//   - a `static Foo *p;`, reaching the C compiler untagged (OZ-004, #37)
//
// `parse::repair_bare_macro_statements` writes a `;` over one whitespace
// byte -- length-preserving, because every offset in the file is a span
// into this text -- and `walk_top_level` writes the space back when it
// copies the text through, so the output keeps the source as written.

mod common;
use common::{compile_and_run, ozobject_src};

/// A stand-in with `ZBUS_OBS_DECLARE`'s shape: a declaration macro that
/// supplies its own terminating `;`, which is exactly why a caller writes
/// none.
const BARE_MACRO: &str = "\
#include <stdio.h>
#define OBS_DECLARE(n) extern const int n;
#define ADD_OBS(c, n, prio) extern const int n;
";

/* The absorption needs a second call-shaped line after the unterminated
 * one: the unterminated macro only supplies the return *type*, and it is
 * that next line tree-sitter takes for the function declarator. `zbus`
 * writes exactly this pair -- `ZBUS_OBS_DECLARE(x)` then
 * `ZBUS_CHAN_ADD_OBS(chan, x, prio);` -- which is what these tests
 * reproduce, down to the argument shape: three arguments ending in a
 * number is what derails the parameter list and makes it consume forward.
 * Two arguments, or no second line at all, and tree-sitter recovers on its
 * own -- and the test then passes with the repair disabled, vacuously.
 * Both weaker shapes were tried here first and did exactly that. */

/// #288 -- the absorbed construct is a class implementation.
#[test]
fn an_implementation_after_a_bare_macro_is_still_translated() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        BARE_MACRO,
        "\
@interface Counter : OZObject
- (int)bump;
@end

OBS_DECLARE(marker)

ADD_OBS(some_chan, marker, 4);

@implementation Counter {
	int _n;
}
- (int)bump
{
	_n = _n + 1;
	return _n;
}
@end

const int marker = 1;

int main(void) {
	Counter *c = [[Counter alloc] init];
	printf(\"bump=%d marker=%d\\n\", [c bump], marker);
	return 0;
}
"
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    /* The original declarations are echoed as banner comments, so the
     * question is whether a *line* still begins with the Objective-C -- the
     * shape that reaches GCC as "stray '@' in program". */
    assert!(
        !out.source_c.lines().any(|l| l.starts_with("@implementation")),
        "the @implementation should be translated, not copied through:\n{}",
        out.source_c
    );
    let stdout = compile_and_run(&src, "an_implementation_after_a_bare_macro_is_still_translated");
    assert_eq!(stdout, "bump=1 marker=1\n");
}

/// #289 -- the absorbed construct is a second `OZM`, whose block literal
/// would otherwise reach the C compiler with its `^`.
#[test]
fn a_second_ozm_after_a_bare_macro_still_hoists_its_block() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        BARE_MACRO,
        "\
struct fake_obs {
	void (*cb)(int);
};
#define FAKE_OBS_DEFINE(name, fn) \\
	static struct fake_obs name = { .cb = (fn) }

OZM(FAKE_OBS_DEFINE, first_obs, ^(int v) {
	printf(\"first=%d\\n\", v);
});

OBS_DECLARE(marker)

ADD_OBS(some_chan, marker, 4);

OZM(FAKE_OBS_DEFINE, second_obs, ^(int v) {
	printf(\"second=%d\\n\", v);
});

const int marker = 1;

int main(void) {
	first_obs.cb(1);
	second_obs.cb(2);
	return 0;
}
"
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    /* Named explicitly rather than by absence of `^`: the invocation is
     * left standing for the preprocessor, so the assertion is that its
     * callback argument became a hoisted function's name. */
    for obs in ["first_obs", "second_obs"] {
        assert!(
            out.source_c.contains(&format!("OZM(FAKE_OBS_DEFINE, {}, oz_block_", obs)),
            "{}'s block literal should be hoisted and named at the call site:\n{}",
            obs,
            out.source_c
        );
    }
    let stdout = compile_and_run(&src, "a_second_ozm_after_a_bare_macro_still_hoists_its_block");
    assert_eq!(stdout, "first=1\nsecond=2\n");
}

/// A guard on the repair rather than a regression: with the repair removed
/// there is no inserted `;` for this to find, so it passes either way. It
/// exists so a future change that stops undoing the insertion is caught.
///
/// The third face: with no `{ ... }` after the absorbed construct, the
/// bogus node is a `declaration` rather than a `function_definition`, and
/// what it swallows is a file-scope declaration whose class name then never
/// gets tagged -- `static Counter *p;` reaching the C compiler as
/// `Counter *`, which is not a C type there (OZ-004, #37).
#[test]
fn a_file_scope_declaration_after_a_bare_macro_is_still_tagged() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        BARE_MACRO,
        "\
@interface Counter : OZObject
- (int)bump;
@end

OBS_DECLARE(marker)

ADD_OBS(some_chan, marker, 4);

static Counter *sShared;

@implementation Counter
- (int)bump
{
	return 7;
}
@end

const int marker = 1;

int main(void) {
	sShared = [[Counter alloc] init];
	printf(\"bump=%d\\n\", [sShared bump]);
	return 0;
}
"
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("static struct Counter *sShared;"),
        "the file-scope declaration should have its class name tagged:\n{}",
        out.source_c
    );
    let stdout =
        compile_and_run(&src, "a_file_scope_declaration_after_a_bare_macro_is_still_tagged");
    assert_eq!(stdout, "bump=7\n");
}

/// The repair is for the parse only. A macro that terminates its own
/// expansion would get a second `;` and leave a stray empty declaration at
/// file scope -- `-Wextra-semi`, an ISO C violation `just test-pedantic`
/// gates on -- so the output has to keep the line as written.
#[test]
fn the_repair_adds_no_semicolon_to_the_output() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        BARE_MACRO,
        "\
@interface Counter : OZObject
- (int)bump;
@end

OBS_DECLARE(marker)

ADD_OBS(some_chan, marker, 4);

@implementation Counter
- (int)bump
{
	return 1;
}
@end

const int marker = 1;

int main(void) {
	return 0;
}
"
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("OBS_DECLARE(marker)\n"),
        "the invocation should be unchanged, with no `;` added:\n{}",
        out.source_c
    );
    assert!(
        !out.source_c.contains("OBS_DECLARE(marker);"),
        "a `;` reached the output, which doubles the macro's own:\n{}",
        out.source_c
    );
}

/// Also a guard, not a regression -- it passes with the repair removed,
/// since removing it can only make the repair fire less.
///
/// The guard on the repair: a genuine macro *return type* keeps its
/// declarator on the same line, so it must be left alone. Only a line break
/// between the two marks the shape this repairs, where they were written as
/// separate statements.
#[test]
fn a_macro_return_type_on_one_line_is_untouched() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        "#include <stdio.h>\n#define RESULT(t) t\n",
        "\
RESULT(int) five(void)
{
	return 5;
}

int main(void) {
	printf(\"five=%d\\n\", five());
	return 0;
}
"
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("RESULT(int) five(void)"),
        "a real macro return type should be copied through unchanged:\n{}",
        out.source_c
    );
    let stdout = compile_and_run(&src, "a_macro_return_type_on_one_line_is_untouched");
    assert_eq!(stdout, "five=5\n");
}
