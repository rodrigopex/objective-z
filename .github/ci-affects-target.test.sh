#!/usr/bin/env bash
# Regression test for .github/ci-affects-target.sh.
#
# The filter it tests ran on every pull request for three weeks and
# answered `affects=true` every time, whatever the diff (#409). Nothing
# caught that, because a broken filter and a filter that correctly says
# "yes, this touches the transpiler" print the same line. So the cases
# below are about the *false* answers as much as the true ones: a filter
# that cannot say false is the bug, restated.
#
# Builds throwaway repositories in a temporary directory -- no network,
# no dependency on this checkout's own history -- and asserts the
# printed verdict, and for the fail-open cases the reason too, since
# failing open for the wrong reason is how #409 looked correct.
#
# Run it directly: .github/ci-affects-target.test.sh
set -uo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ci-affects-target.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

failures=0
cases=0

git_quiet() { git -c init.defaultBranch=main -c user.email=t@example.com -c user.name=t "$@"; }

# An upstream with `main`, plus a feature branch containing $2 (a
# newline-separated list of paths to create and commit).
make_upstream() {
	local dir="$tmp/$1" branch="$2" paths="$3"
	mkdir -p "$dir" && (
		cd "$dir" || exit 1
		git_quiet init -q .
		mkdir -p docs && echo base >docs/README.md
		git_quiet add -A && git_quiet commit -qm base
		git_quiet checkout -qb "$branch"
		while IFS= read -r p; do
			[ -n "$p" ] || continue
			mkdir -p "$(dirname "$p")" && echo touched >>"$p"
		done <<<"$paths"
		git_quiet add -A && git_quiet commit -qm change
		git_quiet checkout -q main
	)
}

# Run the script in $1 with a pull_request environment, and compare
# `affects=` against $2 and -- when $3 is given -- the reason against a
# substring of it. $BASE_SHA, when non-empty, is handed to the script as
# OZ_BASE_SHA; it is a global rather than an assignment prefix on the
# call because bash keeps such a prefix in effect after a *function*
# returns, which would leak one case's base into the next one's.
BASE_SHA=""
expect() {
	local dir="$1" want="$2" want_reason="${3:-}" name="$4"
	local out err rc verdict reason
	cases=$((cases + 1))
	err="$tmp/stderr.$cases"
	out=$(cd "$dir" && CI=true GITHUB_ACTIONS="" GITHUB_EVENT_NAME=pull_request \
		GITHUB_BASE_REF=main OZ_BASE_SHA="$BASE_SHA" \
		bash "$script" 2>"$err")
	rc=$?
	verdict="${out#affects=}"
	reason=$(sed -n 's/^reason: //p' "$err")

	if [ "$rc" -ne 0 ]; then
		echo "FAIL  $name: exited $rc"
		failures=$((failures + 1))
		return
	fi
	if [ "$verdict" != "$want" ]; then
		echo "FAIL  $name: affects=$verdict, expected $want (reason: $reason)"
		failures=$((failures + 1))
		return
	fi
	if [ -n "$want_reason" ] && [[ "$reason" != *"$want_reason"* ]]; then
		echo "FAIL  $name: reason '$reason' does not mention '$want_reason'"
		failures=$((failures + 1))
		return
	fi
	echo "ok    $name (affects=$verdict)"
}

# The case #409 is about: a documentation-only pull request, in the
# checkout shape `fetch-depth: 0` produces.
make_upstream docs-up docs-only 'docs/STATUS.md
CLAUDE.md'
git_quiet clone -q "$tmp/docs-up" "$tmp/docs-full"
(cd "$tmp/docs-full" && git_quiet checkout -q docs-only)
expect "$tmp/docs-full" false "none under a path that reaches a target build" \
	"docs-only change does not reach a target build"

# The same change with the base named by SHA rather than by ref, which
# is the workflow's OZ_BASE_SHA path.
BASE_SHA=$(git -C "$tmp/docs-full" rev-parse origin/main)
expect "$tmp/docs-full" false "none under a path that reaches a target build" \
	"docs-only change, base given as a SHA"
BASE_SHA=""

# A source change must still run everything.
make_upstream code-up code 'tools/oz_static/src/emit.rs'
git_quiet clone -q "$tmp/code-up" "$tmp/code-full"
(cd "$tmp/code-full" && git_quiet checkout -q code)
expect "$tmp/code-full" true "matches tools/oz_static/" \
	"a transpiler source change reaches a target build"

# So must a change to the filter itself, or to the workflow that runs it.
make_upstream self-up self '.github/ci-affects-target.sh'
git_quiet clone -q "$tmp/self-up" "$tmp/self-full"
(cd "$tmp/self-full" && git_quiet checkout -q self)
expect "$tmp/self-full" true "matches .github/" \
	"a change to the filter re-runs what it gates"

# `...` semantics: a source file landing on the base branch after this
# branch started is not this change's, so a docs-only branch stays
# docs-only.
git_quiet clone -q "$tmp/docs-up" "$tmp/docs-moved"
(
	cd "$tmp/docs-moved" || exit 1
	git_quiet checkout -q main
	mkdir -p src && echo later >>src/OZLater.m
	git_quiet add -A && git_quiet commit -qm "landed on main after the branch"
	git_quiet update-ref refs/remotes/origin/main HEAD
	git_quiet checkout -q docs-only
)
expect "$tmp/docs-moved" false "none under a path that reaches a target build" \
	"base commits after the branch point are not this change's"

# The failure #409 was made of: a depth-1 single-branch clone has
# neither origin/main nor a merge base. It must fail *open*, and say
# which of the two it was rather than blaming the diff.
git_quiet clone -q --depth=1 --branch docs-only "file://$tmp/docs-up" "$tmp/docs-shallow"
expect "$tmp/docs-shallow" true "no base commit in this checkout" \
	"a shallow checkout fails open, naming the missing base"

# A shallow clone that does have origin/main -- so the ref check passes
# and only the merge base is missing. This is the shape the old script
# thought it was in.
git_quiet clone -q --no-single-branch --depth=1 --branch docs-only \
	"file://$tmp/docs-up" "$tmp/docs-shallow-ref"
if (cd "$tmp/docs-shallow-ref" && git rev-parse --verify --quiet origin/main >/dev/null); then
	expect "$tmp/docs-shallow-ref" true "no merge base" \
		"a shallow checkout with the ref present fails open on the merge base"
else
	echo "skip  shallow-with-ref case: this git does not keep origin/main"
fi

# The damage the old script did, which no verdict reveals: under CI it
# fetched `--depth=1`, and `git` applies that to the *repository*. So a
# full checkout came out shallow, and every later step -- including the
# merge base this filter needs -- lost the history it depends on. The
# filter must answer without touching the object store.
cases=$((cases + 1))
git_quiet clone -q "$tmp/docs-up" "$tmp/docs-intact"
(cd "$tmp/docs-intact" && git_quiet checkout -q docs-only)
(cd "$tmp/docs-intact" && CI=true GITHUB_ACTIONS="" GITHUB_EVENT_NAME=pull_request \
	GITHUB_BASE_REF=main bash "$script" >/dev/null 2>&1)
if [ "$(git -C "$tmp/docs-intact" rev-parse --is-shallow-repository)" = "false" ]; then
	echo "ok    the filter leaves a full checkout full"
else
	echo "FAIL  the filter shallowed a full checkout"
	failures=$((failures + 1))
fi

# Not a pull request at all: nothing to diff, so run everything.
cases=$((cases + 1))
out=$(cd "$tmp/docs-full" && GITHUB_EVENT_NAME=push bash "$script" 2>/dev/null)
if [ "$out" = "affects=true" ]; then
	echo "ok    a push is not filtered (affects=true)"
else
	echo "FAIL  a push is not filtered: got '$out'"
	failures=$((failures + 1))
fi

echo
if [ "$failures" -eq 0 ]; then
	echo "$cases case(s), all passed"
	exit 0
fi
echo "$cases case(s), $failures failed"
exit 1
