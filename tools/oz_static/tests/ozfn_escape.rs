// SPDX-License-Identifier: Apache-2.0
//
// ozfn_escape.rs -- `OZFN(^{ ... })` puts a block where a function pointer
// is wanted, without hiding the enclosing macro from Clang (#300).
//
// `OZM` solves the same conversion problem by discarding the *whole* macro
// invocation on the Objective-C side, which costs two things. What the
// macro declares becomes invisible to Clang, so a reference to it needs a
// hand-written `#ifdef __OBJC__` twin. And a callback that is not a macro
// argument is out of reach entirely -- Zephyr's connection callbacks sit in
// a designated initializer after the `=`:
//
//     BT_CONN_CB_DEFINE(conn_callbacks) = {
//             .connected = ...,
//             .recycled  = ...,
//     };
//
// `OZFN` hides one *expression* instead, so the real macro expands on both
// sides and only the block is unparsed. The transpiler needed no change for
// this: `emit::top_level_block_edits` already hoists a block literal in
// that position, and the two preprocessor halves were the whole gap.
//
// Expanding to `0` is forced rather than chosen. The position wants a value,
// and reaching a *static* initializer means the expansion has to be a null
// pointer constant -- `((blk), 0)` and `((void)sizeof(blk), 0)` have the
// value zero and are not null pointer constants, so a pointer initializer
// rejects both with `-Wint-conversion`. That is also why Clang cannot be
// made to type-check the block here: a constant is required, so the block
// goes unparsed.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// A stand-in for a target that stores callbacks in function-pointer
/// fields, initialized after the `=` where `OZM` cannot reach.
const TARGET: &str = "\
#include <stdio.h>
struct fake_conn_cb {
	void (*connected)(int);
	void (*disconnected)(int);
	void (*recycled)(void);
};
#define FAKE_CONN_CB_DEFINE(name) static struct fake_conn_cb name
";

/// The shape this exists for.
#[test]
fn ozfn_puts_a_block_in_a_function_pointer_field() {
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
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains(".connected = OZFN(oz_block_"),
        "the literal should be hoisted and the OZFN left standing:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run(&src, "ozfn_block_in_function_pointer_field"), "connected=7\n");
}

/// The whole struct, which is what motivated it: three callbacks in one
/// designated initializer, each its own hoisted function.
#[test]
fn ozfn_fills_a_whole_callback_struct() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "\
FAKE_CONN_CB_DEFINE(cbs) = {
	.connected = OZFN(^(int err) {
		printf(\"connected=%d\\n\", err);
	}),
	.disconnected = OZFN(^(int reason) {
		printf(\"disconnected=%d\\n\", reason);
	}),
	.recycled = OZFN(^(void) {
		printf(\"recycled\\n\");
	}),
};

int main(void) {
	cbs.connected(1);
	cbs.disconnected(2);
	cbs.recycled();
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert_eq!(
        out.source_c.matches("OZFN(oz_block_").count(),
        3,
        "each field should carry its own hoisted function:\n{}",
        out.source_c
    );
    assert_eq!(
        compile_and_run(&src, "ozfn_fills_a_whole_callback_struct"),
        "connected=1\ndisconnected=2\nrecycled\n"
    );
}

/// The property `OZM` does not have: the enclosing macro expands on the
/// Objective-C side too, so the symbol it declares is visible to Clang and
/// needs no `#ifdef __OBJC__` twin. Referring to `cbs` in the same file is
/// what proves it -- under `OZM` that reference would not compile without a
/// hand-written declaration.
#[test]
fn the_enclosing_symbol_needs_no_objc_twin() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "\
FAKE_CONN_CB_DEFINE(cbs) = {
	.recycled = OZFN(^(void) {
		printf(\"recycled\\n\");
	}),
};

/* Deliberately no Clang-only declaration of `cbs` anywhere above. */
static struct fake_conn_cb *ref = &cbs;

int main(void) {
	ref->recycled();
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    /* By line start: the source's own prose may mention the directive, and
     * a substring test then matches the comment rather than the code --
     * which is exactly what the first version of this assertion did. */
    assert!(
        !out.source_c.lines().any(|l| l.trim_start().starts_with("#ifdef __OBJC__")),
        "no Clang-only twin should be needed:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run(&src, "ozfn_enclosing_symbol_needs_no_twin"), "recycled\n");
}

/// Variadic, for `OZM`'s reason: a comma at the top level of the block body
/// would otherwise split the argument list, as
/// `too many arguments provided to function-like macro invocation`.
#[test]
fn a_comma_inside_the_block_body_does_not_split_the_call() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        TARGET,
        "\
FAKE_CONN_CB_DEFINE(cbs) = {
	.connected = OZFN(^(int err) {
		int a = 1, b = 2;
		printf(\"sum=%d\\n\", a + b + err);
	}),
};

int main(void) {
	cbs.connected(3);
	return 0;
}
"
    );
    assert_eq!(compile_and_run(&src, "ozfn_comma_in_block_body"), "sum=6\n");
}
