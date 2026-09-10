#!/usr/bin/env bash
# Does this change need the ARM/Zephyr jobs to run?
#
# `pedantic-gate`, `spin-validate`, `hw-build-check` and
# `zephyr-integration` each install the Zephyr SDK, populate a west
# workspace and build samples on target -- 805s of runner time on the
# measurements in #387. None of it reads a documentation change, and
# `docs/STATUS.md` travels with every code change in this repo, so
# docs-only pull requests are common rather than rare.
#
# Prints `affects=true` or `affects=false` for $GITHUB_OUTPUT. Fails
# *open*: anything it cannot work out prints true, because running a job
# that was not needed wastes minutes while skipping one that was needed
# lets a regression through.
#
# Why these four jobs and not the other twelve: the `main` ruleset
# requires eight contexts and all eight are host jobs (`rust-tests`,
# `sanitizers`, `c-coverage`, `generated-freshness` and the four
# `behavior-tests` legs). A skipped job reports neither success nor
# failure, so filtering a *required* one would leave every merge waiting
# on a check that will never arrive. These four are not required.
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

if [ "${GITHUB_EVENT_NAME:-}" != "pull_request" ]; then
	say true "not a pull request, so there is no base to diff against"
	exit 0
fi

base="${GITHUB_BASE_REF:-}"
if [ -z "$base" ]; then
	say true "no base ref in the environment"
	exit 0
fi

git fetch --quiet --depth=1 origin "$base" 2>/dev/null || {
	say true "could not fetch the base ref"
	exit 0
}

# `...` compares against the merge base, so commits that landed on main
# after this branch started are not counted as this change's.
files=$(git diff --name-only "origin/${base}...HEAD" 2>/dev/null) || {
	say true "could not diff against the base ref"
	exit 0
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
