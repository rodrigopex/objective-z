#!/usr/bin/env bash
# Regression test for .github/version-omission.sh (#512).
#
# Two halves, because the gate has two ways to be silently useless.
#
# The regex half drives `--match` against subjects whose answer is
# already known: the six omissions #512 recorded (#504, #503, #510,
# #509, #508 and #523's window), which must all match, and the types
# that bump nothing, which must not. A gate whose pattern quietly
# matches nothing reports success forever -- the first draft of this
# script anchored the pattern at `^` and applied it to `%h %s` log
# lines, which is an always-false that looks exactly like a clean
# repository.
#
# The end-to-end half builds throwaway repositories in a temporary
# directory -- no network, no dependency on this checkout's history --
# so the red cases are constructed rather than waited for, and the
# green cases prove the gate is not simply always red.
#
# Run it directly: .github/version-omission.test.sh
set -uo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/version-omission.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

failures=0
cases=0

git_quiet() { git -c init.defaultBranch=main -c user.email=t@example.com -c user.name=t "$@"; }

# --- the regex half -------------------------------------------------

# $1 = expected ("yes"/"no"), $2 = subject.
match() {
	local want="$1" subject="$2" rc
	cases=$((cases + 1))
	bash "$script" --match "$subject" >/dev/null 2>&1
	rc=$?
	if [ "$rc" -gt 1 ]; then
		echo "FAIL  --match errored ($rc) on: $subject"
		failures=$((failures + 1))
		return
	fi
	local got=no
	[ "$rc" -eq 0 ] && got=yes
	if [ "$got" != "$want" ]; then
		echo "FAIL  --match said $got, expected $want: $subject"
		failures=$((failures + 1))
		return
	fi
	echo "ok    $want  $subject"
}

echo "-- the six omissions #512 recorded: every one must be bumpworthy"
match yes "feat(oz2c): read the ARC marks and ownership qualifiers Clang already wrote (#453)"
match yes "fix(oz2c): resolve a for-in loop variable used as a receiver (#502)"
match yes "fix(oz2c): poison the one word a slab free cannot reach (#490)"
match yes "fix(oz2c)!: refuse a category on a class declared nowhere (#501)"
match yes "fix(oz2c)!: refuse a static send whose ownership came from an ambiguous poll (#483)"
match yes "fix(oz2c): refuse a for-in header unrelated to the element type (#505)"

echo
echo "-- a break on the crate, whatever its type; and the crate's former name"
match yes "refactor(oz2c)!: a break in the crate spelled as a refactor"
match yes "fix(oz_static)!: the crate's name before #462"
match yes "feat(oz_static): the crate's name before #462"

echo
echo "-- types that bump nothing by the convention"
match no "build: oz2c 0.100.3 (#515)"
match no "docs: the AST is fully read, and the audit's first finding was about itself (#453)"
match no "samples: include <zephyr/kernel.h>, and drop the workarounds it made unnecessary"
match no "test(oz2c): pin the one nested store where both refusals fire (#511)"
match no "ci: floor pykwalify so west init stops flaking under Python 3.12 (#517)"
match no "docs(oz2c): a documentation change scoped to the crate"
match no "build(oz2c): a build change scoped to the crate"
match no "chore(oz2c): a chore scoped to the crate"

echo
echo "-- the false positives a scope-agnostic '^(feat|fix)' would produce"
# All real subjects or real prefixes from `main`: fix(ci) x17,
# feat(samples) x6, fix(build) x2, feat(runtime) x23. Each of these
# going red would put the gate's first failure on an unrelated CI fix.
match no "fix(ci): drop --narrow from hw-build west update for CMSIS resolution"
match no "feat(samples): a sample that exercises a new construct"
match no "fix(build): use spaces instead of tabs in generated .clangd file"
match no "feat(runtime): something under src/, which is not this crate"
match no "refactor(oz_sdk)!: a break in the SDK headers, not the crate"

# --- the end-to-end half --------------------------------------------

# A repo with tools/oz2c/Cargo.toml at $2, then the commits in $3 (one
# `subject` per line; a subject of the form `@version X.Y.Z` sets the
# version instead of adding a file).
make_repo() {
	local dir="$tmp/$1" version="$2" commits="$3" n=0
	mkdir -p "$dir/tools/oz2c/src" && (
		cd "$dir" || exit 1
		git_quiet init -q .
		printf '[package]\nname = "oz2c"\nversion = "%s"\nedition = "2021"\n\n[dependencies]\ntree-sitter = "0.24"\n' \
			"$version" >tools/oz2c/Cargo.toml
		echo 'fn main() {}' >tools/oz2c/src/main.rs
		git_quiet add -A && git_quiet commit -qm "feat(oz2c): the beginning"
		while IFS= read -r subject; do
			[ -n "$subject" ] || continue
			n=$((n + 1))
			case "$subject" in
			"@version "*)
				local v="${subject#@version }"
				sed -i.bak "s/^version = .*/version = \"$v\"/" tools/oz2c/Cargo.toml
				rm -f tools/oz2c/Cargo.toml.bak
				git_quiet add -A
				# Loudly, because a fixture whose commit silently
				# did nothing -- a sed that matched nothing, say --
				# builds the repository the test is not describing,
				# and the case still passes for the wrong reason.
				if ! git_quiet commit -qm "build: oz2c $v"; then
					echo "FIXTURE BROKEN: '@version $v' committed nothing" >&2
					exit 1
				fi
				;;
			"@dependency")
				printf 'serde = "1.0"\n' >>tools/oz2c/Cargo.toml
				git_quiet add -A
				git_quiet commit -qm "build(oz2c): add a dependency"
				;;
			*)
				echo "$n" >>"tools/oz2c/src/f$n.rs"
				git_quiet add -A
				git_quiet commit -qm "$subject"
				;;
			esac
		done <<<"$commits"
	)
}

# Run the gate in $1 and compare its exit code to $2 (0 green, 1 red,
# 2 unanswerable). $4, when given, must appear in the combined output.
expect() {
	local dir="$1" want="$2" name="$3" needle="${4:-}" out rc
	cases=$((cases + 1))
	out=$(cd "$dir" && bash "$script" 2>&1)
	rc=$?
	if [ "$rc" -ne "$want" ]; then
		echo "FAIL  $name: exited $rc, expected $want"
		printf '%s\n' "$out" | sed 's/^/        /'
		failures=$((failures + 1))
		return
	fi
	if [ -n "$needle" ] && [[ "$out" != *"$needle"* ]]; then
		echo "FAIL  $name: output does not mention '$needle'"
		printf '%s\n' "$out" | sed 's/^/        /'
		failures=$((failures + 1))
		return
	fi
	echo "ok    $name (exit $rc)"
}

echo
echo "-- end to end"

# The shape a correct merge leaves: work, then the number, last.
make_repo green-bumped 0.98.0 'fix(oz2c): a patch
@version 0.98.1'
expect "$tmp/green-bumped" 0 "a PR that landed with its number is green"

# The shape all six omissions left: bumpworthy work on top, no number.
make_repo red-omitted 0.98.0 'fix(oz2c): a patch'
expect "$tmp/red-omitted" 1 "a bumpworthy commit with no number is red" \
	"still reads 0.98.0"

# #512's measured state of `main`: one patch and two breaks since
# b0a67ae, version unchanged. Three, not one -- the count is reported.
make_repo red-three 0.98.0 'fix(oz2c): poison the one word a slab free cannot reach (#490)
fix(oz2c)!: refuse a category on a class declared nowhere (#501)
fix(oz2c)!: refuse a static send whose ownership came from an ambiguous poll (#483)'
expect "$tmp/red-three" 1 "#512's three-omission state of main is red" \
	"3 bumpworthy commit(s)"

# The #508 shape: the number was written and gated locally, and the PR
# was merged from a different head before it could be pushed. From the
# repository's side that is indistinguishable from never writing it --
# which is the point, and why no author-side habit closes it.
make_repo red-508 0.99.0 'fix(oz2c)!: refuse a static send whose ownership came from an ambiguous poll (#483)
test(oz2c): pin that #507s store refusal and this one cannot shadow each other (#483)'
expect "$tmp/red-508" 1 "#508's merged-before-the-number-landed shape is red" \
	"1 bumpworthy commit(s)"

# Non-bumpworthy commits on top of a number are the normal resting
# state of `main`, and must not be red -- this is the case that decides
# whether the gate is usable at all.
make_repo green-non-bumpworthy 0.100.2 '@version 0.100.3
docs: record what landed
test(oz2c): pin a refusal
samples: include <zephyr/kernel.h>
ci: floor pykwalify
build: reach the artifacts that accumulate
fix(ci): a CI fix
feat(samples): a new sample'
expect "$tmp/green-non-bumpworthy" 0 "docs/test/ci/samples on top of a number stay green" \
	"none bumpworthy"

# A commit that touches Cargo.toml without changing the version must
# not reset the window. `git log -- tools/oz2c/Cargo.toml | head -1`,
# the issue's sketch, stops here and reports green with the omission
# still standing.
make_repo red-dependency 0.98.0 'fix(oz2c): a patch
@dependency'
expect "$tmp/red-dependency" 1 "a dependency edit does not hide the omission behind it" \
	"1 bumpworthy commit(s)"

# ...and the same repository is what the sketch would have called green,
# recorded so the difference is visible rather than asserted.
cases=$((cases + 1))
sketch_last=$(cd "$tmp/red-dependency" && git log --format=%H -- tools/oz2c/Cargo.toml | head -1)
sketch_count=$(cd "$tmp/red-dependency" && git log --format='%s' "$sketch_last..HEAD" |
	grep -cE '^(feat|fix)(\([^)]*\))?!?:')
if [ "$sketch_count" -eq 0 ]; then
	echo "ok    the sketch's last-touch window would have called that case green (0)"
else
	echo "FAIL  expected the sketch to miss it, but it counted $sketch_count"
	failures=$((failures + 1))
fi

# A shallow checkout has no history to walk, so `git log -- Cargo.toml`
# answers "never changed" and a fail-open gate goes green on a repo it
# never read. This one must fail *closed*: exit 2, saying which.
make_repo shallow-src 0.98.0 'fix(oz2c): a patch'
git_quiet clone -q --depth=1 "file://$tmp/shallow-src" "$tmp/shallow" 2>/dev/null
if [ "$(git -C "$tmp/shallow" rev-parse --is-shallow-repository)" = "true" ]; then
	expect "$tmp/shallow" 2 "a shallow checkout fails closed, naming the depth" \
		"fetch-depth: 0"
else
	echo "skip  shallow case: this git did not produce a shallow clone"
fi

echo
if [ "$failures" -eq 0 ]; then
	echo "$cases case(s), all passed"
	exit 0
fi
echo "$cases case(s), $failures failed"
exit 1
