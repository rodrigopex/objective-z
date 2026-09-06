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
use common::{compile_and_run, compile_and_run_strict, ozobject_src as PREAMBLE};

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

/// A callback field that does not return `int` -- #303.
///
/// The reason this needed its own fix: the hoisted function's return type
/// came from a *guess* at the body (`int` for any return-with-value), and
/// writing the type on the literal to correct it made things worse rather
/// than better, because an explicit return type moved the parameter list
/// to a place `render_block` did not look. `^uint32_t(int seed)` was
/// hoisted `int f(void)`, so the body's own `seed` was undeclared and the
/// error came from GCC about a signature the author never wrote.
///
/// `uint32_t` is Zephyr's `bt_conn_auth_cb.app_passkey`, which is what
/// found this. `px-keyboard` had to keep a named C function for that one
/// callback while the two `void` ones beside it stayed blocks.
///
/// Deliberately `_strict`: the host `cc` is Apple clang, which only
/// *warns* on assigning `int (*)(int)` to a `uint32_t (*)(int)` field,
/// while Zephyr's GCC errors under `-Werror`. Without
/// `-Werror=incompatible-pointer-types` the wrong return type still runs
/// here and the test passes for the wrong reason.
#[test]
fn an_explicit_return_type_survives_the_hoist() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        "\
#include <stdio.h>
#include <stdint.h>
struct fake_auth_cb {
	uint32_t (*app_passkey)(int);
	void (*cancel)(int);
};
",
        "\
static struct fake_auth_cb auth_cbs = {
	.app_passkey = OZFN(^uint32_t(int seed) {
		return (uint32_t)seed + 1U;
	}),
	.cancel = OZFN(^(int reason) {
		printf(\"cancel=%d\\n\", reason);
	}),
};

int main(void) {
	printf(\"passkey=%u\\n\", auth_cbs.app_passkey(555554));
	auth_cbs.cancel(3);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run_strict(&src, "ozfn_explicit_return_type"),
        "passkey=555555\ncancel=3\n"
    );
}

/// The exact signature, so a regression names itself instead of arriving
/// as a puzzling compile error in the test above.
///
/// Both halves are asserted because they failed independently: the return
/// type was `int` whether or not one was written, and the parameter list
/// was dropped only when one was.
#[test]
fn the_hoisted_signature_carries_both_the_type_and_the_parameters() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
#include <stdint.h>
static void *held = OZFN(^uint32_t(int seed) {
	return (uint32_t)seed + 1U;
});
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    let signature = out
        .source_c
        .lines()
        .find(|l| l.contains("oz_block_") && l.trim_end().ends_with(';'))
        .unwrap_or_else(|| panic!("no hoisted prototype:\n{}", out.source_c))
        .to_string();
    assert!(
        signature.starts_with("uint32_t "),
        "the written return type should be carried, not guessed `int`: {}",
        signature
    );
    assert!(
        signature.contains("(int seed)"),
        "an explicit return type must not cost the parameter list: {}",
        signature
    );
}

/// A pointer return type, which is where the fix could most easily go
/// wrong in the other direction.
///
/// The return type and the parameter list are chained through the same
/// declarator nodes, so a walk that collects `*` without stopping at the
/// parameters counts theirs too -- `^void *(char *s)` becomes
/// `void **`, a pointer level the author never wrote. One star, from the
/// return type alone.
#[test]
fn a_pointer_return_type_does_not_collect_the_parameters_stars() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
static void *held = OZFN(^const char *(char *s, char *t) {
	return s ? s : t;
});
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    let signature = out
        .source_c
        .lines()
        .find(|l| l.contains("oz_block_") && l.trim_end().ends_with(';'))
        .unwrap_or_else(|| panic!("no hoisted prototype:\n{}", out.source_c))
        .to_string();
    assert!(
        signature.starts_with("const char* "),
        "exactly one star, and the qualifier kept: {}",
        signature
    );
}
