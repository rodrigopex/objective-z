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
use std::process::Command;

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

/// `docs/STATUS.md` is the archive, and is excluded for that reason and no
/// other.
///
/// It is where retired names are explained rather than erased -- it already
/// names `__objc_refcount_get`, `oz_heap_obj_alloc` and the retired Python
/// pipeline throughout -- so a rename narrative that could not spell what it
/// retired would be useless to the reader who greps the old name. It also
/// cites `<retired>/PARITY.md` three times, twice as runnable `git show` /
/// `git log` commands against the `python-backend-final` branch, where that
/// path still exists; rewriting those would turn two working commands into
/// broken ones.
///
/// Every other tracked file is strict.
fn is_the_archive(line: &str) -> bool {
	line.starts_with("docs/STATUS.md:")
}

fn repo_root() -> std::path::PathBuf {
	std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("../..")
		.canonicalize()
		.expect("the crate sits two levels below the repo root")
}

/// The tree-wide half.
///
/// `git grep` rather than a directory walk, for three reasons: it *is* the
/// end-state predicate ("no tracked file"), `-I` skips binaries, and it never
/// descends into `tools/oz2c/target/`, which is gitignored and full of old
/// binaries with the string compiled into them. A walk would have to
/// blocklist that by hand and would go stale.
#[test]
fn no_tracked_file_outside_the_archive_carries_the_retired_name() {
	let out = Command::new("git")
		.args(["grep", "-I", "-n", "-e", RETIRED_LOWER, "-e", RETIRED_UPPER, "--", "."])
		.current_dir(repo_root())
		.output()
		.expect("git grep must run; a guard that skips is not a guard");

	/* Exit 1 is `git grep` finding nothing, which is success here. Anything
	 * above that is git failing to search at all, and must not be mistaken
	 * for a clean tree. */
	let code = out.status.code().unwrap_or(-1);
	assert!(
		code == 0 || code == 1,
		"git grep failed (exit {}), so this proved nothing:\n{}",
		code,
		String::from_utf8_lossy(&out.stderr)
	);

	let stdout = String::from_utf8_lossy(&out.stdout);
	let offenders: Vec<&str> = stdout.lines().filter(|l| !is_the_archive(l)).collect();
	assert!(
		offenders.is_empty(),
		"{} line(s) carry the transpiler's retired name. `oz2c` names the tool, \
		 `oz_`/`OZ_` names the code, and only `docs/STATUS.md` may spell what was \
		 retired:\n{}",
		offenders.len(),
		offenders.join("\n")
	);
}
