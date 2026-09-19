// SPDX-License-Identifier: Apache-2.0
//
// ozfn_argument_validation.rs -- `OZFN`'s argument, and a block the parser
// had to guess the end of (#548).
//
// `OZFN(...)` expands to `0` for Clang and to `__VA_ARGS__` for C, so its
// argument is invisible to Clang **by construction** -- and that is not an
// oversight. `OZMacro.h` gives the reason: to reach a static initializer
// the expansion must be a null pointer constant, so the block has to go
// unparsed. oz2c is therefore the only gate on it, and it validated
// nothing at all.
//
// The worst shape was a block literal missing its closing brace.
// tree-sitter recovers by **inserting** one, so oz2c received a well-formed
// `block_literal` ending at the macro's `)`, hoisted it, wrote the hoisted
// name back, and exited 0. The emitted body was not the one the author
// wrote, and nothing said so -- the same "quietly shortened" failure #494
// removed for sends, still live inside a macro argument. Outside one, the
// identical mutation is caught, because there Clang sees it.
//
// Detected from the parser, not by counting braces: a recovered node is
// marked `is_missing()`. The inserted `}` is a **descendant** of the block
// (it belongs to the `compound_statement` inside it), not a direct child --
// a direct-child test found nothing and left the headline case unreported.
// Settled by dumping the tree, which is the only way these questions have
// ever been settled here.
//
// Every case uses the function-pointer-field shape `ozfn_escape.rs` uses,
// rather than a Zephyr `K_TIMER_DEFINE`. That is deliberate: the host stub
// for `K_TIMER_DEFINE` cannot compile a hoisted callback, so a control
// written on it fails for a reason that has nothing to do with the check --
// which is how these three controls failed first time round. On-target
// evidence for the real macro is `just test-boards` over
// `samples/transpiled_blocks` and `samples/zbus_objc`.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// A stand-in for a target that stores callbacks in function-pointer
/// fields -- the same one `ozfn_escape.rs` uses.
const TARGET: &str = "\
#include <stdio.h>
struct fake_conn_cb {
	void (*connected)(int);
};
#define FAKE_CONN_CB_DEFINE(name) static struct fake_conn_cb name
";

/// A block missing its closing `}` -- refused rather than completed for
/// the author (#548, M40).
#[test]
fn an_unclosed_block_in_a_macro_argument_is_refused() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "\
FAKE_CONN_CB_DEFINE(cbs) = {
	.connected = OZFN(^(int err) {
		printf(\"connected=%d\\n\", err);
	),
};
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("missing its closing '}'"), "diagnostics:\n{}", diags);
    /* The note must say *why* this one is not caught for the author, or a
     * reader who knows Clang checks their other blocks reads it as a
     * tooling bug rather than as their own typo. */
    assert!(
        diags.contains("only check on it") || diags.contains("hides it from Clang"),
        "the diagnostic must say why oz2c is the only gate here:\n{}",
        diags
    );
}

/// An empty argument (#548, M37). It used to reach GCC as
/// `expected expression before ')'`, against generated C.
#[test]
fn an_empty_ozfn_argument_is_refused() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "FAKE_CONN_CB_DEFINE(cbs) = {\n\t.connected = OZFN(),\n};\n"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("it is empty"), "diagnostics:\n{}", diags);
}

/// An integer where a callback belongs (#548, M55 and M38).
///
/// In a typed slot GCC reported `initialization of 'void (*)(int)' from
/// 'int'` against generated C; in a discarded expression --
/// `(void)OZFN(42)` -- nothing complained at all.
#[test]
fn a_non_callable_ozfn_argument_is_refused() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "FAKE_CONN_CB_DEFINE(cbs) = {\n\t.connected = OZFN(42),\n};\n"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("neither a block literal nor the name of a function"),
        "diagnostics:\n{}",
        diags
    );
}

/// The discarded-expression form of the same thing, which is the one that
/// produced **no** complaint from anybody (#548, M38).
#[test]
fn a_discarded_non_callable_ozfn_argument_is_refused() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "int main(void) {\n\t(void)OZFN(42);\n\treturn 0;\n}\n"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("neither a block literal nor the name of a function"),
        "a discarded OZFN was accepted silently before this:\n{}",
        diags
    );
}

/// **The control that matters most.** A well-formed block still hoists,
/// compiles and runs -- `samples/transpiled_blocks` and
/// `samples/zbus_objc` are built on this shape, so a check that
/// over-reached would break the samples, not the tests.
#[test]
fn a_well_formed_ozfn_block_still_works() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "\
FAKE_CONN_CB_DEFINE(cbs) = {
	.connected = OZFN(^(int err) {
		printf(\"connected=%d\\n\", err);
	}),
};

int main(void) {
	cbs.connected(7);
	return 0;
}
"
    );
    let out = oz2c::transpile(&src).expect("a well-formed OZFN block must transpile");
    assert!(
        out.source_c.contains(".connected = OZFN(oz_block_"),
        "the literal is hoisted and the OZFN left standing:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run(&src, "ozfn_wellformed_control"), "connected=7\n");
}

/// A function *name* is the other legal argument, and must stay legal.
#[test]
fn an_ozfn_function_name_is_accepted() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "\
static void on_connected(int err) { printf(\"named=%d\\n\", err); }

FAKE_CONN_CB_DEFINE(cbs) = {
	.connected = OZFN(on_connected),
};

int main(void) {
	cbs.connected(4);
	return 0;
}
"
    );
    assert_eq!(compile_and_run(&src, "ozfn_function_name_control"), "named=4\n");
}

/// A block that *is* closed, nested inside another that is closed, must
/// pass -- the recursive search must not blame an outer block for an inner
/// one, nor report the same block twice.
#[test]
fn nested_well_formed_blocks_are_accepted() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "\
FAKE_CONN_CB_DEFINE(cbs) = {
	.connected = OZFN(^(int err) {
		void (^inner)(int) = ^(int n) {
			printf(\"inner=%d\\n\", n);
		};

		inner(err);
	}),
};

int main(void) {
	cbs.connected(9);
	return 0;
}
"
    );
    assert_eq!(compile_and_run(&src, "ozfn_nested_blocks"), "inner=9\n");
}
