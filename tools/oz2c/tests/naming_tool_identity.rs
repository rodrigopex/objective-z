// SPDX-License-Identifier: Apache-2.0
//
// naming_tool_identity.rs - `oz2c` names the tool, `oz_`/`OZ_` names the
// code, and the transpiler's former name appears nowhere (#462).
//
// The tool had two names. `oz2c` was the binary, the justfile recipe and the
// head of the pipeline diagram; the other one named the crate, its
// directory, the cmake module, four emitted ABI functions, the per-class id
// macro, the generated dispatch header, the build output directory, the
// diagnostic prefix and the banner the tool signed its output with -- 1851
// occurrences across 183 files.
//
// This pins the end state, in the shape #418 established and #417 reused:
// **a name is retired only when the old spelling is gone from the output.**
// Asserting the new name is present is the weaker half, satisfied by a
// half-done rename; asserting the old one is absent is the half that
// catches it.
//
// Two scopes, because they fail differently:
//
//   1. Generated C. A survivor here is a *link* failure the moment one
//      translation unit carries the old spelling and another the new --
//      which is what a partial rename of an emitter produces.
//   2. Every tracked file. A survivor here is only untidy in most places,
//      but not everywhere: the tree compares this name as data in a
//      diagnostic regex, a compile-db filter, a ninja target and a CI path
//      filter, and that last one fails by making CI *skip* jobs rather than
//      fail them.
//
// The needles are built with `concat!` so this file does not itself contain
// what it forbids. That is not fussiness: the tree-wide check reads tracked
// files, so a literal here would match itself, and the obvious repair --
// exempting this path -- makes the guard unable to see the one file most
// likely to be edited when someone reintroduces the name. A guard with an
// exemption for itself is not a guard.

mod common;
use common::ozobject_src;

/// Built by concatenation; see the header comment.
const RETIRED_LOWER: &str = concat!("oz_", "static");
const RETIRED_UPPER: &str = concat!("OZ_", "STATIC");

/// Every generated artifact, so a survivor cannot hide in the half this
/// test does not look at.
fn generated(src: &str) -> String {
	let out = oz2c::transpile(src).expect("should transpile");
	format!("{}\n{}\n{}", out.companion_h, out.companion_c, out.source_c)
}

#[test]
fn generated_c_carries_no_trace_of_the_retired_name() {
	let src = format!("{}\n{}", ozobject_src(), "int main(void) { return 0; }\n");
	let all = generated(&src);
	for needle in [RETIRED_LOWER, RETIRED_UPPER] {
		assert!(
			!all.contains(needle),
			"generated C still carries the transpiler's retired name ('{}'); \
			 it is `oz2c` for the tool and `oz_`/`OZ_` for emitted code (#462):\n{}",
			needle,
			all
		);
	}
}

/// The positive half. Absence alone is satisfied by an emitter that stopped
/// emitting the bridge at all, which is the vacuous pass #413 and #417 both
/// hit -- so name the four ABI functions and the class-id macro explicitly.
#[test]
fn the_abi_is_spelled_on_the_c_side_prefix() {
	let src = format!("{}\n{}", ozobject_src(), "int main(void) { return 0; }\n");
	let all = generated(&src);
	for expected in [
		"int oz_retain_count(id obj)",
		"void oz_release(struct OZObject *self)",
		"struct OZObject *oz_retain(struct OZObject *self)",
		"const char *oz_class_name(struct OZObject *self)",
		"#define OZ_CLASS_",
		"oz2c_dispatch.h",
	] {
		assert!(
			all.contains(expected),
			"expected `{}` in generated C; if the ABI moved again, this test \
			 and `docs/STATUS.md` move with it:\n{}",
			expected,
			all
		);
	}
}
