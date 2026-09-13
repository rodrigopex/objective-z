// SPDX-License-Identifier: Apache-2.0
//
// naming_get_prefix.rs - `get` marks a function that writes through a
// caller's pointer, and the log-precision reader does not (#417, #413).
//
// `_oz_get_log_precision` was three problems in one name: **public** while
// carrying `_oz_`, which is the *per-class internal* prefix; and misusing
// `get`, which #413 settled as marking a method or function that writes
// through a caller's pointer -- `-getBytes:length:range:`,
// `-getDescription:maxLength:`. It returned its value instead, so `get` was
// a lie, and `-usedBytes` and `-cString` are the precedent for leaving it
// off. It is `oz_log_precision` now.
//
// Two assertions, and the pairing is the point. The **positive** one is what
// carries the weight: an absence check on the retired spelling goes
// vacuously true the moment the emitter stops emitting the symbol at all,
// which is a green test asserting nothing. Asserting the new declaration is
// *present* fails in that case, so the two together say what one cannot.
// (That shape bit a sibling lane during #462's rename, where
// `!body.contains(<old name>)` stayed green against output that no longer
// contained either spelling.)
//
// The retired name is built with `concat!` rather than written literally so
// that a future tree-wide guard on it -- the shape `naming_tool_identity.rs`
// uses for the transpiler's old name -- would not trip on this file. Nothing
// forbids it today; this is cheap insurance, not a current constraint.

mod common;
use common::ozobject_src;

const RETIRED: &str = concat!("_oz_", "get_log_precision");

fn generated(src: &str) -> String {
	let out = oz2c::transpile(src).expect("should transpile");
	format!("{}\n{}\n{}", out.companion_h, out.companion_c, out.source_c)
}

/// The companion declares the reader unconditionally and defines a weak
/// fallback, so a program that never links `src/OZLog.c` still resolves it.
/// Both spellings are pinned because they are two separate emission sites
/// (`companion.rs`'s header and source builders), and a rename that reaches
/// one and not the other is a conflicting declaration rather than a warning.
#[test]
fn the_log_precision_reader_is_emitted_without_a_get_prefix() {
	let src = format!("{}\n{}", ozobject_src(), "int main(void) { return 0; }\n");
	let all = generated(&src);

	for expected in [
		"int oz_log_precision(void);",
		"__attribute__((weak)) int oz_log_precision(void) { return -1; }",
	] {
		assert!(
			all.contains(expected),
			"expected `{}` in generated C -- if the reader moved again, \
			 `include/oz_sdk/Foundation/OZLog.h` and `docs/STATUS.md` move with \
			 it:\n{}",
			expected,
			all
		);
	}

	assert!(
		!all.contains(RETIRED),
		"the retired spelling is still emitted; `get` marks writing through a \
		 caller's pointer and this returns its value (#413, #417):\n{}",
		all
	);
}

/// The SDK header and the companion both declare this function, and the SDK
/// headers are *spliced into* generated C -- so the two land in one
/// translation unit. Identical, they are redundant and legal; differing in
/// the return type or the parameter list, they are a conflicting declaration
/// and nothing compiles. #418 established this invariant for the refcount
/// reader; this is the second function it governs, and it had no test.
#[test]
fn the_sdk_header_and_the_companion_declare_it_identically() {
	const SDK: &str = include_str!("../../../include/oz_sdk/Foundation/OZLog.h");
	const DECL: &str = "int oz_log_precision(void);";

	assert!(
		SDK.contains(DECL),
		"`OZLog.h` no longer declares `{}` -- Clang resolves calls to it while \
		 dumping the AST, before any generated header exists, so this \
		 declaration cannot simply move to the companion",
		DECL
	);
	assert!(
		!SDK.contains(RETIRED),
		"`OZLog.h` still carries the retired spelling"
	);

	let src = format!("{}\n{}", ozobject_src(), "int main(void) { return 0; }\n");
	let out = oz2c::transpile(&src).expect("should transpile");
	assert!(
		out.companion_h.contains(DECL),
		"the companion header must declare it with the SDK header's exact \
		 signature, or the spliced translation unit has two conflicting \
		 declarations:\n{}",
		out.companion_h
	);
}
