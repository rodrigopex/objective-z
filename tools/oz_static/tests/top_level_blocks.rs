// SPDX-License-Identifier: Apache-2.0
//
// top_level_blocks.rs -- blocks are lowered to function pointers at the top
// level too, not only inside a method or a function body (#272).
//
// A block *is* a function pointer in generated C, and every position routing
// through `collect::render_type` already said so: an ivar becomes
// `void (*_blk)(int)`, a method parameter `void (*b)(struct k_timer *)`, a
// local `void (*local)(int)`. Three positions did not, all of them assembled
// by patching the original text in `emit::walk_top_level`, where no edit
// lowered a block type:
//
//   1. a `block_literal` in an unclaimed top-level node -- a file-scope
//      block variable's initializer, `static void (^g)(int) = ^(int v){...};`
//   2. a file-scope block variable, `static void (^g)(int);`
//   3. a free function's signature, prototype and definition alike
//
// All three reached the C compiler with the `^` intact. Blocks are a Clang
// extension rather than ISO C, so this was not a weaker type but text no GCC
// target can parse: `arm-zephyr-eabi-gcc` reports `expected ')' before '^'
// token`. Nothing in the repository writes any of the three shapes, which is
// why they went unnoticed. Each one *is* valid Objective-C, verified against
// `clang -x objective-c -fobjc-arc -fblocks`, so each was a real
// valid-in / invalid-out defect rather than a shape nobody may write.
//
// What #272 was *filed* for -- a Zephyr definition macro taking an inline
// block, `ZBUS_LISTENER_DEFINE(n, ^(...){ ... })` -- turns out not to be
// writable at all, and for a reason outside oz_static: Objective-C refuses
// block-to-function-pointer conversion in every position, so a file spelling
// it is rejected by Clang however well oz_static lowers it. That finding is
// recorded in
// `a_macro_invocation_is_hoisted_though_the_shape_is_not_valid_objc`, which
// is the one case here whose shape is not an advertised idiom.
//
// Same family as gaps Q, V and R, and as #246 / #250 / #251: the top-level or
// free-function path getting a reduced version of what a method body gets.
//
// **Running the output is not enough to catch this, and nearly hid it.**
// `compile_and_run` compiles with the host `cc`, which on macOS is Apple
// clang -- and clang enables blocks by default, so a surviving `^` compiles
// there as a perfectly good Clang block. The first draft of
// `file_scope_block_variable_with_a_literal_initializer` therefore *passed
// with the fix disabled*: the declaration and its initializer were both left
// as blocks, so they agreed with each other and the host compiler was happy.
// The real toolchain is not: `arm-zephyr-eabi-gcc` has no blocks support at
// all, and `clang -fno-blocks` reports `blocks support disabled` on the same
// text.
//
// So every test here asserts on the *generated text* via
// `assert_no_block_caret`, which is the only instrument that can see the
// defect on this host, and runs the program on top of that to prove the
// callback still reaches the right function. Same shape as gap Y's finding
// that an ARM `-Wpedantic` sweep written the obvious way reports a clean
// result on output that is not clean -- a green run whose subject is not what
// the reader thinks.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src};

/// No block `^` survives into any generated file, outside a comment.
///
/// The discriminating check for all of #272, for the reason in this file's
/// header: the host compiler accepts what the target cannot parse, so
/// compiling and running says nothing about whether the lowering happened.
/// Banner comments deliberately echo the original Objective-C, `^` and all,
/// so comment lines are excluded.
fn assert_no_block_caret(src: &str) {
    let out = oz_static::transpile(src).expect("should transpile");
    // Every file the single-file assembler produces, not just the primary
    // source: a block-typed declaration can land in the companion header.
    let text = format!("{}\n{}\n{}", out.source_c, out.companion_h, out.companion_c);
    let offenders: Vec<&str> = text
        .lines()
        .filter(|l| l.contains('^'))
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("/*") && !t.starts_with('*')
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "a block `^` reached the generated C, which no GCC target can parse:\n{}",
        offenders.join("\n")
    );
}

/// All three positions in one source, so the edits are exercised together
/// rather than only one at a time.
#[test]
fn no_block_caret_survives_at_file_scope() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>
static void (^g_blk)(int);
static void (^g_init)(int) = ^(int v) { printf(\"i=%d\\n\", v); };
static void take_cb(void (^cb)(int));
static void take_cb(void (^cb)(int)) { cb(1); }

int main(void) {
	g_blk = ^(int v) { printf(\"g=%d\\n\", v); };
	take_cb(g_blk);
	g_init(2);
	return 0;
}
"
    );
    assert_no_block_caret(&src);
    let stdout = compile_and_run(&src, "no_block_caret_survives_at_file_scope");
    assert_eq!(stdout, "g=1\ni=2\n");
}

/// Position 3, on the definition: a free function taking a block parameter.
/// The identical *method* parameter has always been lowered.
#[test]
fn free_function_block_parameter_is_lowered() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>
static void apply_twice(void (^cb)(int))
{
	cb(1);
	cb(2);
}

int main(void) {
	apply_twice(^(int v) { printf(\"v=%d\\n\", v); });
	return 0;
}
"
    );
    assert_no_block_caret(&src);
    let stdout = compile_and_run(&src, "free_function_block_parameter_is_lowered");
    assert_eq!(stdout, "v=1\nv=2\n");
}

/// Position 3, on a separate prototype. A declaration without a body is a
/// different node from a `function_definition` and reaches the passthrough
/// arm instead, so it needs its own case -- asserting only the definition
/// would pass against a build that lowered just that one.
#[test]
fn free_function_block_prototype_is_lowered() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>
static void apply_once(void (^cb)(int));

int main(void) {
	apply_once(^(int v) { printf(\"p=%d\\n\", v); });
	return 0;
}

static void apply_once(void (^cb)(int))
{
	cb(9);
}
"
    );
    assert_no_block_caret(&src);
    let stdout = compile_and_run(&src, "free_function_block_prototype_is_lowered");
    assert_eq!(stdout, "p=9\n");
}

/// Position 2, found while implementing rather than while filing: a file-scope
/// block *variable*. Assigned and called, so the lowered type has to be right
/// and not merely parseable.
#[test]
fn file_scope_block_variable_is_lowered() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>
static void (^g_blk)(int);

int main(void) {
	g_blk = ^(int v) { printf(\"g=%d\\n\", v); };
	g_blk(4);
	return 0;
}
"
    );
    assert_no_block_caret(&src);
    let stdout = compile_and_run(&src, "file_scope_block_variable_is_lowered");
    assert_eq!(stdout, "g=4\n");
}

/// A file-scope block literal with an initializer written directly, rather
/// than through a macro -- positions 1 and 2 in one declaration, so the two
/// edits have to compose on the same node.
#[test]
fn file_scope_block_variable_with_a_literal_initializer() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>
static void (^g_blk)(int) = ^(int v) { printf(\"i=%d\\n\", v); };

int main(void) {
	g_blk(5);
	return 0;
}
"
    );
    assert_no_block_caret(&src);
    let stdout = compile_and_run(&src, "file_scope_block_variable_with_a_literal_initializer");
    assert_eq!(stdout, "i=5\n");
}

/// The static bar now runs over a file-scope block's body, which had no scan
/// at all -- the top-level twin of the free-function scan gap Q added. This is
/// the one case here that must keep *failing*: without the scan the `@try`
/// would be accepted and then reach `emit`, which has no lowering for it, so
/// the user would get a compiler error against generated code they never
/// wrote instead of a located diagnostic.
#[test]
fn a_rejected_construct_in_a_file_scope_block_is_still_located() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
static void (^g_blk)(int) = ^(int v) {
	@try {
		(void)v;
	} @catch (id e) {
	}
};

int main(void) {
	g_blk(1);
	return 0;
}
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@try/@catch"), "diagnostics: {}", diags);
}

/// A top-level macro invocation carrying a block literal is hoisted too --
/// `walk_top_level`'s passthrough arm is where this and a declaration both
/// land, so it follows from the same edit and needs no knowledge of the macro.
///
/// This is what lets a Zephyr definition macro take an inline block, which is
/// what #272 was filed for, and **whether it works turns entirely on how the
/// macro treats the argument under `__OBJC__`.** Clang parses the same file
/// -- `cmake/oz_static.cmake` dumps one AST per source as oz2c's ownership
/// oracle -- so the source has to be valid Objective-C, and Objective-C
/// refuses block-to-function-pointer conversion in every position:
///
/// ```text
/// error: initializing 'void (*)(int)' with an expression of incompatible
///        type 'void (^)(int)'
/// ```
///
/// A macro that *consumes* the argument in its ObjC expansion therefore
/// cannot be used -- writing `ZBUS_LISTENER_DEFINE` directly is rejected,
/// because `struct zbus_observer::callback` is a function pointer.
///
/// A macro that *discards* it under `__OBJC__` can:
///
/// ```objc
/// #ifdef __OBJC__
/// #define OZ_TIMER_DEFINE(name, ...) static struct k_timer name
/// #else
/// #define OZ_TIMER_DEFINE(name, ...) K_TIMER_DEFINE(name, __VA_ARGS__)
/// #endif
/// ```
///
/// An argument whose parameter does not appear in the replacement list is
/// discarded rather than expanded or parsed, so Clang never type-checks the
/// block at all -- it only has to lex. `...` absorbs any unprotected comma in
/// the block body. Under the C arm oz_static has already replaced the literal
/// with its hoisted function's name, so the argument really is a function
/// pointer and this is a plain `K_TIMER_DEFINE`. Verified both ways by hand:
/// valid under `clang -x objective-c -fobjc-arc -fblocks`, and the generated
/// C clean under `-std=c17 -pedantic-errors`.
///
/// The consuming shape is used below because it is the one that pins the
/// *hoisting* without needing an SDK macro to exist yet. Its source is not
/// valid Objective-C, which is why this case asserts on the generated text
/// alone; `file_scope_block_variable_with_a_literal_initializer` covers the
/// same code path on a shape that is.
#[test]
fn a_macro_invocation_carrying_a_block_literal_is_hoisted() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#define REGISTER_CB(_name, _cb) static void (*_name##_slot)(int) = _cb
REGISTER_CB(lis, ^(int v) {
	(void)v;
});

int main(void) { return 0; }
"
    );
    assert_no_block_caret(&src);
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("REGISTER_CB(lis, oz_block_"),
        "the literal should be replaced by its hoisted function's name:\n{}",
        out.source_c
    );
}

/// The `OZ_TIMER_DEFINE` shape end to end: a variadic macro that discards its
/// block argument under `__OBJC__`, so the source is valid Objective-C, and
/// expands to the real Zephyr macro in the C oz_static emits.
///
/// This is the case that makes the idiom real rather than merely lowered, and
/// it pins the two properties it rests on: the literal is replaced by the
/// hoisted function's name, and the prototype is emitted *ahead* of the
/// invocation, since `K_TIMER_DEFINE` puts the name in a static initializer
/// (`Z_TIMER_INITIALIZER`'s `.expiry_fn = expiry`) where only an address
/// constant will do.
#[test]
fn a_discarding_variadic_macro_gives_the_zephyr_definition_shape() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
struct k_timer { void (*expiry_fn)(struct k_timer *); };
#define K_TIMER_DEFINE(name, exp, stp) \\
	static struct k_timer name = { .expiry_fn = (exp) }
#ifdef __OBJC__
#define OZ_TIMER_DEFINE(name, ...) static struct k_timer name
#else
#define OZ_TIMER_DEFINE(name, ...) K_TIMER_DEFINE(name, __VA_ARGS__)
#endif

OZ_TIMER_DEFINE(my_timer, ^(struct k_timer *t) {
	int a = 1, b = 2;
	(void)t;
	(void)(a + b);
}, NULL);

int main(void) { return 0; }
"
    );
    assert_no_block_caret(&src);
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("OZ_TIMER_DEFINE(my_timer, oz_block_"),
        "the literal should reach the macro as a hoisted function name:\n{}",
        out.source_c
    );
    let proto = out
        .source_c
        .find("void oz_block_")
        .expect("a prototype should be emitted");
    let use_site = out
        .source_c
        .find("OZ_TIMER_DEFINE(my_timer, oz_block_")
        .expect("the invocation should be present");
    assert!(
        proto < use_site,
        "the prototype must precede the static initializer that names it"
    );
}
