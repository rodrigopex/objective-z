#!/usr/bin/env bash
# Fail when `main` owes a version bump that never got applied (#512).
#
# #499 moved the crate version off the PR and onto the merge: a branch
# declares the *kind* of bump in its conventional-commit subject and
# carries no number, and the number goes on as the final commit of
# whichever PR is next to land. That removed the conflict class it was
# aimed at -- 8 of the 9 recorded version incidents were *created* by a
# branch carrying a number it could not know -- and opened a new one:
# the number became a separate, final, skippable commit. It was skipped
# six times in about a day (#504, #503, #510, #509, #508, and #523's
# window).
#
# This is the gate for exactly that, and for nothing else.
#
# WHAT IT DOES NOT DO, deliberately: it does not compute the number.
# The arithmetic version of this check was implemented and replayed over
# 17 historical transitions, and 9 reproduced while 8 did not, for three
# independent reasons -- the bump can ride a commit that *precedes* the
# one setting the kind (16 of 18 windows hold more than one commit, so
# that is the normal case), deliberate skips are indistinguishable from
# omissions because allocation lives in coordination messages rather
# than the repo, and the semver *kind* is a PR-level judgment rather
# than the conventional-commit *type*. A gate that fails on correct
# history teaches people to ignore it, so this one answers the only
# question that has a reliable answer:
#
#   Is there a bumpworthy commit since the last version change, with
#   the version unchanged?
#
# Omission is the only failure mode #499 introduced, so it is the only
# one that has to be covered.
#
# It also does not print a suggested number. The total is
# order-dependent -- base 0.98.0 with one patch and two breaks folds to
# 0.100.0 as `patch, break, break` but 0.100.1 as `break, break, patch`
# -- so any number this script printed would carry the same
# unreliability the arithmetic gate was rejected for, in a place where a
# human would trust it. It lists the commits in landing order instead,
# which is what folding them by hand needs.
#
# Run it anywhere: .github/version-omission.sh
set -uo pipefail

manifest="tools/oz2c/Cargo.toml"

# The scope that means "the crate this version belongs to".
#
# The scope is load-bearing, not decoration. A scope-agnostic
# `^(feat|fix)` -- the shape the issue sketched -- also matches
# `fix(ci):` (17 on main), `feat(samples):` (6), `fix(build):` (2) and
# `feat(runtime):` (23), none of which bump this crate by the
# convention. That is the "fails on correct history" failure mode
# restated, and it would fire on the very next CI fix.
#
# The crate's former name (#462 renamed it) is deliberately absent, and
# the reason is measured rather than stylistic: this gate only ever
# walks `<last version change>..HEAD`, and that window's start moves
# forward with every bump and never backwards. The newest commit
# carrying the old scope is 256e8c0, 2026-09-13; the version has changed
# many times since, most recently d37a0b7 on 2026-09-15. So the old
# scope is unreachable here -- 0 occurrences in the current window
# against 186 across all of `main` -- and matching it would be dead
# coverage that also trips `naming_tool_identity.rs`, which reserves the
# retired spelling to `docs/STATUS.md`. `transpiler` is absent for a
# different reason: it scoped the retired Python pipeline, not this
# crate, and its last use was 2026-08-29.
crate_scopes="oz2c"

# Bumpworthy: `feat` or `fix` on the crate (patch or minor -- the gate
# does not care which), or any type on the crate marked `!` (a break).
# `test(oz2c):`, `docs(oz2c):`, `build(oz2c):` and `chore(oz2c):` bump
# nothing and are excluded.
#
# Known under-detection, accepted: a break spelled on another scope
# (`refactor(oz_sdk)!:`) that the maintainer judges a crate minor, and a
# scopeless `fix!:` (zero occurrences on main). Under-detection is the
# safe direction for a net; a false positive is the one that kills it.
#
# Held without a `^` so it can be anchored against either a bare subject
# or a `%h %s` log line. Anchoring it at `^` and then matching it
# against `%h %s` lines is a silent always-false, which is how the first
# draft of this script passed on `main`'s real omissions.
bumpworthy_re="((feat|fix)\((${crate_scopes})\)!?:|[a-zA-Z]+\((${crate_scopes})\)!:)"

# `--match <subject>`: is this one subject bumpworthy? Exit 0 yes, 1 no.
# The gate's verdict rests entirely on this regex, so it is reachable on
# its own and the self-test drives it against subjects whose answer is
# already known -- the six omissions #512 recorded, and the types that
# must *not* match.
if [ "${1:-}" = "--match" ]; then
	if [ "$#" -ne 2 ]; then
		echo "usage: $0 --match <commit-subject>" >&2
		exit 2
	fi
	printf '%s\n' "$2" | grep -qE "^${bumpworthy_re}"
	rc=$?
	exit "$rc"
fi

die() {
	echo "version-omission: cannot answer: $*" >&2
	echo >&2
	echo "This is a gate, so it fails closed: an unanswerable check that" >&2
	echo "reported success would be worse than no check at all." >&2
	exit 2
}

# The `[package]` version at a commit. Scoped to the `[package]` table
# rather than grepping the file, because `^version = ` at column zero is
# also what a `[dependencies.foo]` table writes -- and a count over a
# file that mentions the thing being counted is not a count of the
# thing (the same trap that makes `git grep -c CARGO_PKG_VERSION` return
# 2, both of them prose asserting the absence).
pkg_version() {
	git show "$1:$manifest" 2>/dev/null | awk '
		/^\[/ { in_pkg = ($0 == "[package]"); next }
		in_pkg && /^version[[:space:]]*=/ {
			gsub(/^version[[:space:]]*=[[:space:]]*"|"[[:space:]]*$/, "")
			print
			exit
		}'
}

if [ "$(git rev-parse --is-inside-work-tree 2>/dev/null)" != "true" ]; then
	die "not a git work tree"
fi

# A depth-1 checkout has no history to walk, so `git log -- $manifest`
# answers "nothing ever changed it" and the gate would go green on a
# repo it never looked at. The workflow sets `fetch-depth: 0`; this is
# what says so when someone removes it.
if [ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = "true" ]; then
	die "shallow checkout -- this check needs the full history (fetch-depth: 0)"
fi

head_version=$(pkg_version HEAD)
if [ -z "$head_version" ]; then
	die "no [package] version in $manifest at HEAD"
fi

# The newest commit that changed the version *value*, walking newest
# first and stopping at the first one. Not "the newest commit touching
# Cargo.toml", which the issue's sketch used: a commit that adds a
# dependency touches the manifest without changing the version, and
# would reset the window and hide an omission behind it.
last=""
for sha in $(git log --format=%H -- "$manifest"); do
	parent=$(git rev-parse --verify --quiet "${sha}^" 2>/dev/null)
	if [ -z "$parent" ]; then
		# Root commit: it introduced the manifest, so it set the version.
		last="$sha"
		break
	fi
	if [ "$(pkg_version "$sha")" != "$(pkg_version "$parent")" ]; then
		last="$sha"
		break
	fi
done

if [ -z "$last" ]; then
	die "found no commit that set the version in $manifest"
fi

last_subject=$(git log -1 --format='%h %s' "$last")

# Landing order, because that is the order a human folds the kinds in.
# The gate's own verdict does not depend on it -- it does not compute a
# number -- but the list it prints is read by someone who has to.
subjects=$(git log --reverse --format='%h %s' "${last}..HEAD")
rc=$?
if [ "$rc" -ne 0 ]; then
	die "could not walk ${last}..HEAD (git log exited $rc)"
fi

# `grep -c` on a variable rather than on a pipeline off `git log`: a
# pipeline's exit code is its last stage's, so a `git log` that died
# would have left a count of 0 and a green gate. The walk above already
# succeeded by the time this runs.
# Anchored past the `%h ` prefix the walk above prints, so the pattern
# sees the subject's first character where it expects it.
bumpworthy=$(printf '%s\n' "$subjects" | grep -E "^[0-9a-f]+ ${bumpworthy_re}")
count=$(printf '%s\n' "$bumpworthy" | grep -c .)

echo "version-omission: $manifest reads $head_version"
echo "version-omission: last version change: $last_subject"

if [ "$count" -eq 0 ]; then
	total=$(printf '%s' "$subjects" | grep -c . )
	echo "version-omission: $total commit(s) since, none bumpworthy -- OK"
	exit 0
fi

echo >&2
echo "version-omission: FAIL -- $count bumpworthy commit(s) since the last" >&2
echo "version change, and $manifest still reads $head_version." >&2
echo >&2
echo "In landing order:" >&2
printf '%s\n' "$bumpworthy" | sed 's/^/  /' >&2
echo >&2
echo "Per #499 the number goes on as the final commit of the PR that is" >&2
echo "next to land, and one of these landed without it. Fold the kinds in" >&2
echo "the order above -- \`fix\` is a patch, \`feat\` or \`!\` is a minor while" >&2
echo "pre-1.0 -- and land the result as \`build: oz2c X.Y.Z\`, noting the" >&2
echo "commits it absorbs so the jump does not read as a typo." >&2
echo >&2
echo "The order matters and a count will not do: one patch and two breaks" >&2
echo "off 0.98.0 fold to 0.100.0 as patch-break-break, but 0.100.1 as" >&2
echo "break-break-patch." >&2
exit 1
