#!/usr/bin/env bash
# Does this change need the ARM/Zephyr jobs to run?
#
# `pedantic-gate`, `spin-validate`, `hw-build-check` (two legs) and
# `zephyr-integration` each install the Zephyr SDK, populate a west
# workspace and build samples on target. Measured on run 34595210212, a
# docs-only pull request that ran all five: 172s + 165s + 253s + 350s +
# 947s = 1887s of runner time. None of it reads a documentation change,
# and `docs/STATUS.md` travels with every code change in this repo, so
# docs-only pull requests are common rather than rare.
#
# Prints `affects=true` or `affects=false` for $GITHUB_OUTPUT. Fails
# *open*: anything it cannot work out prints true, because running a job
# that was not needed wastes minutes while skipping one that was needed
# lets a regression through.
#
# Why these jobs and not the other twelve: the `main` ruleset requires
# eight contexts and all eight are host jobs (`rust-tests`,
# `sanitizers`, `c-coverage`, `generated-freshness` and the four
# `behavior-tests` legs). A skipped job reports neither success nor
# failure, so filtering a *required* one would leave every merge waiting
# on a check that will never arrive. These five are not required.
#
# `.github/ci-affects-target.test.sh` is the regression test, and it
# exists because the first version of this script never once answered
# false (#409).
set -euo pipefail

# Paths that can change what the transpiler emits, how it is built, what
# is built against it, or how CI does any of that. `.github/**` is here
# so a change to this script or the workflow re-runs everything it gates.
patterns=(
	'tools/oz_static/'
	'include/'
	'src/'
	'samples/'
	'benchmarks/'
	'cmake/'
	'scripts/'
	'tests/'
	'Kconfig'
	'CMakeLists.txt'
	'west.yml'
	'zephyr/'
	'justfile'
	'.github/'
)

say() {
	echo "affects=$1"
	echo "reason: $2" >&2
}

# A fail-open is not an answer, and #409 shipped and stayed shipped
# because it looks exactly like one: `affects=true` from a diff that
# died is indistinguishable from `affects=true` because the diff found
# a source file, and the reason line goes to stderr, which nothing
# reads. So annotate the unexplained ones -- they reach the run summary,
# where a filter that has stopped filtering is visible without anyone
# going looking. The two deliberate `true` answers (a code path changed,
# or this is not a pull request) stay quiet.
bail() {
	if [ -n "${GITHUB_ACTIONS:-}" ]; then
		echo "::warning title=CI paths filter failed open::$1"
	fi
	say true "$1"
	exit 0
}

if [ "${GITHUB_EVENT_NAME:-}" != "pull_request" ]; then
	say true "not a pull request, so there is no base to diff against"
	exit 0
fi

base="${GITHUB_BASE_REF:-}"
if [ -z "$base" ]; then
	bail "no base ref in the environment"
fi

# Resolve the base commit, and *never fetch* to get it.
#
# The original script fetched `--depth=1` here, which failed two ways at
# once and both silently (#409). `git` applies `--depth=1` to the
# repository rather than to the transfer, so against a full clone it
# truncates history and takes away the very merge base `...` needs --
# the CI checkout became shallow, exactly the damage #389 stopped this
# script doing to a developer's clone. And `actions/checkout` makes a
# *single-branch* clone, whose refspec covers only the PR's own ref, so
# the fetch wrote `FETCH_HEAD` and no `refs/remotes/origin/<base>` at
# all: `origin/main` was not even a valid object name.
#
# Both disappear if the commit is simply present. The workflow checks
# out with `fetch-depth: 0` and passes `OZ_BASE_SHA` from
# `github.event.pull_request.base.sha`, so there are two independent
# ways to name the base and neither needs the network. Run by hand, a
# full clone already has `origin/<base>`; a shallow one is told so
# rather than deepened behind the developer's back.
base_rev=""
for candidate in "${OZ_BASE_SHA:-}" "origin/${base}"; do
	[ -n "$candidate" ] || continue
	if git rev-parse --verify --quiet "${candidate}^{commit}" >/dev/null; then
		base_rev="$candidate"
		break
	fi
done

if [ -z "$base_rev" ]; then
	bail "no base commit in this checkout (shallow clone, or OZ_BASE_SHA unset and no origin/${base})"
fi

# `...` compares against the merge base, so commits that landed on the
# base branch after this branch started are not counted as this
# change's. Ask for that merge base explicitly: a shallow checkout has
# none, and finding that out here names the cause instead of leaving it
# to a `git diff` that reports it as an ambiguous argument.
if ! git merge-base "$base_rev" HEAD >/dev/null 2>&1; then
	bail "no merge base between ${base_rev} and HEAD (shallow checkout?)"
fi

files=$(git diff --name-only "${base_rev}...HEAD" 2>/dev/null) || {
	bail "could not diff against ${base_rev}"
}

if [ -z "$files" ]; then
	say false "no files differ from the base"
	exit 0
fi

while IFS= read -r f; do
	[ -n "$f" ] || continue
	for p in "${patterns[@]}"; do
		case "$f" in
		"$p"*)
			say true "$f matches $p"
			exit 0
			;;
		esac
	done
done <<<"$files"

say false "$(echo "$files" | wc -l | tr -d ' ') changed file(s), none under a path that reaches a target build"
