# Working in this repo when several changes are in flight

Every rule here was paid for on **2026-09-13**, running four to six concurrent
lanes through fourteen merges. Each one is stated with the evidence that earned
it, because a rule without its incident gets argued away.

Read this alongside `CLAUDE.md`, which has the conventions. This file has the
failure modes.

---

## 1. The failure mode here is a green check that stopped testing anything

Not a red gate. **Six times in one day a guard passed while the thing it
guarded was gone**, and every one was found by reading output rather than by a
gate:

| what passed | what was actually gone |
|---|---|
| `CARGO EXIT: 0` | the test count — the log was deleted while the job still wrote to it |
| a clean rebase | a version bump, dropped as already-applied because two branches picked the same number |
| an absence assertion naming a symbol by its pre-rename spelling | any meaning at all, once the rename landed and the string could no longer occur |
| a `tomllib` read-back | `name = "oz2c"`, deleted by a regex that spanned conflict markers; a `[package]` table with only `version` is valid TOML |
| `just test-boards \| tail -20` then `$?` | the entire ARM leg — `tail`'s exit status cannot fail |
| a grep for the other side of a merge | nothing; the grep was for a *paraphrase* of their wording |

**The rule:** when a gate is green, ask what it would do if the property were
absent. If the answer is "pass", the gate is decoration.

**A guard that works looks like this.** The row above is phrased without the
retired symbol's literal spelling because `naming_tool_identity.rs` refused an
earlier draft of *this file* — only `docs/STATUS.md` may spell what #462
retired, and CI failed the commit that introduced the quote. A document about
guards that stopped testing anything was caught by a guard that had not. That
is the distinction worth internalising: this one names the file, the line and
the rule it enforces, and it fires on prose as readily as on code.

**Pair every absence check with a presence check**, so the fixture is proven to
contain the thing whose absence you assert. Asserting absence alone cannot
distinguish "correctly emitted nothing" from "looked at the wrong code".

**Prefer a runtime oracle to a grep for an absent call.** For an ownership
sink, retain and release counts often *balance* inside the function that builds
the value — only the object failing to die reveals the leak.

**Verify the shape of your own diff, not the presence of the other side's
content.** You know exactly what you added; you may not know their exact words.
`git diff origin/main -- <file>` showing `1 insertion, 0 deletions` proves you
removed nothing, whatever their final text says.

---

## 2. Instruments that answer a different question than you asked

- **`git diff origin/main..HEAD` after a fetch that moved main** reports main's
  new work *inverted* alongside yours. An 8-file branch showed 33 files, then
  14. Use `git diff $(git merge-base HEAD origin/main)..HEAD`, or the three-dot
  `git diff origin/main...HEAD`.
- **`git show --stat HEAD~N -- <old path>`** after a rename reports nothing and
  looks exactly like a dropped commit. Verify against the **new** path, and
  prefer `git log -S'<string>' -- <new path>`.
- **`--is-ancestor` says "no" for work that did land.** This repo
  rebase-merges, so your SHAs are replayed. Compare **tree hashes** instead.
- **`pgrep -f '<worktree-name>'`** matches your own shell wrappers — checking
  before deleting one worktree returned 20 hits, all of them the session's own
  commands. Read the matches (`pgrep -fl`) and look for `twister`,
  `qemu-system`, `cargo`, `rustc`, `west build` specifically. A check is only
  worth running if its result *gates* the action.
- **A pipeline's exit code is the last stage's.** Count target results out of
  `twister.json`, and check them against the documented split: **15 suites on
  ARM `mps2/an385`, 13 on RISC-V `qemu_riscv32`** — the two ARM-only entries
  are `sample.objz.gpio_demo` and `sample.hello_category.debug_lines`. Fewer
  means something was filtered silently.

---

## 3. The version line

`tools/oz2c/Cargo.toml`'s version travels in the same commit as the change it
describes. With more than one branch open, that line is the single most
fragile thing in the repo — it was dropped or collided **ten times** in one day.

- **Two branches picking the same next number do not conflict.** The second
  one's edit is byte-identical and already applied, so git drops it silently:
  "Successfully rebased", empty `git status`, and `Cargo.toml` absent from the
  commit. **A conflict stops you; this does not.**
- **Take a number that main does not have, and apply it immediately** — not at
  rebase time. A branch holding a number main is about to take is the silent
  case; a branch holding a number main does not have is *forced* to conflict
  loudly.
- **Read main's actual version at the moment you choose**, never a remembered
  ladder. A version is a claim about main at an instant.
- **Verify three ways**: field completeness (not parseability — `name`,
  `version`, `edition`, `description`, `publish` all present, `name == "oz2c"`),
  `git log -S'<version>' -- tools/oz2c/Cargo.toml` naming exactly one commit,
  and `git diff origin/main --numstat -- tools/oz2c/Cargo.toml` showing `1 1`.
- **Resolve a version conflict by rebuilding, not by patching markers.**
  `git show origin/main:<path> > <path>`, then a column-anchored
  `sed 's/^version = "X"$/version = "Y"/'`. A regex that spans conflict markers
  is the wrong instrument when the markers themselves carry content — after a
  path rename they read `<<<<<<< HEAD:tools/oz2c/Cargo.toml`.
- **Putting the bump in its own tip commit** makes every later rebase a
  one-line re-resolve.
- **A rebase performed on the GitHub page is invisible locally until you
  fetch**, and drops the bump the same way. After any merge, check whether
  main's version actually moved; if it did not, that change shipped unbumped.

---

## 4. Rebasing across other people's work

- **A rename can arrive with no conflict at all.** `oz_platform_print` became
  `OZ_PLATFORM_PRINT`; its only callers were in one lane's file, so nothing
  conflicted and the emitter went on spelling a name the header no longer
  defined. **`git grep` every identifier your commits introduce against the
  post-rebase tree, then build.** A clean replay proves nothing about whether
  the tree compiles.
- **A mechanical sweep's rules cannot reach a file main added *after* your
  commits were written** — that file appears in no diff of yours. Re-run the
  rules against the **post-rebase** tree. One rename branch had 4 compile
  errors this way and the rebase reported success.
- **Gate a rebase when it crosses a rename or an API change you emit; carry the
  numbers forward when it only moves the base.** A replay onto a moved base
  relocates commits. A replay across a rename changes generated output.
- **Commit before any comparison build.** `git checkout -- <file>` restores
  main, not your edited state, and destroys uncommitted work. **The tell is a
  test failure**, not an error: the confirming run reports FAILED and the first
  instinct is "flaky test" rather than "my files are gone". If a gate goes red
  straight after a `checkout`, `restore` or `stash`, check `git status` before
  debugging the test.
- **Check what base you are on before editing.** One lane spent its opening
  minutes re-implementing merged work because its worktree was on a detached
  HEAD at main rather than on the branch. `git log --oneline -1` costs nothing.
- **The push is part of the rebase, not a step after it.** From outside,
  "rebased locally but unpushed" is indistinguishable from "still rebasing":
  the PR keeps reading CONFLICTING and everyone keeps asking. Rebase, verify,
  **push**, then report. Use `--force-with-lease`.

---

## 5. Prose is load-bearing

**A mechanical sweep must not resolve a semantic conflict.** Three instances in
one day, all in prose, none greppable:

- a rename branch conflicting with a branch that changed `docs/ARC.md`'s
  *verdicts*; taking "ours" would have silently reverted the other side's
  findings, and no test would have caught it, because the rows are prose;
- a test's stated rationale, where the *argument* was the defect rather than
  the code — `__bridge_retained` held back "not because either is known to be
  unsafe", which was wrong twice over;
- three files describing a pending rename in the **future tense**, which a
  substitution rewrote into something grammatical, plausible and wrong on both
  the target spelling and the tense.

On a rename or sweep branch, **"ours" is almost always wrong when the other
side changed meaning.** Read the arguments; do not only grep the identifiers.

**Renaming a symbol also invalidates** the diagnostic text that quotes it, the
test that asserts that text, and the doc comment that argues for the old
spelling. Rename symbol, help text and assertion **in one commit**, or you ship
a diagnostic telling authors to call a function that no longer exists.

**Reversing a merged assertion is recorded as an argument overturned**, not a
typo corrected.

---

## 6. A document is a relay, and an instruction file is the most dangerous kind

The workspace `CLAUDE.md` asserted that `just test-smp` was not in CI and that
`samples/smp_shared` had been unbuildable for some time. Both were false, and
had been for five merged PRs — `smp-tests` is `.github/workflows/ci.yml:601`,
compiling *and running* `smp_shared` on two cores on every PR. Two sessions
read that line, believed it, and repeated it as fact; it reached the maintainer
as grounds to block a PR, and the workflow file that disproved it was forty
lines away.

An instruction file arrives as standing direction, so nothing in it reads as an
assertion someone made on a date. It is therefore trusted *more* and verified
*less* than a colleague's message.

**Treat any claim about CI, gates, or what is broken as a relay with an unknown
date.** Check the workflow, the recipe or the tracker before acting. **Then fix
the document** — and **date the claim**, because the correction goes stale too:
the rewritten `smp-tests` bullet asserted the opposite within minutes when the
context was made required.

---

## 7. Coordinating with other lanes

- **Split work by mechanism, not by symptom.** One owner per file that several
  issues rewrite. Serialise them; do not run two writers in `arc.rs`.
- **A peer's relay is not a principal's instruction.** A lane correctly
  declined both an assignment and a reversal of its own merged commit on
  second-hand word. The rule holds *even when the relay turns out to be
  accurate* — a rule that only holds when the relay is wrong is not a rule.
- **A relayed fact is not a verified one, and the coordinator is the single
  point of unverified assertion.** Five claims relayed by the coordinator were
  corrected by lanes, each by measurement: a file's footprint read off a
  pre-rebase worktree, a CI claim taken from a stale document, a doc fix
  assigned for a file that did not contain the text, a defect's location
  inferred from an issue title, and a repro that did not reproduce.
- **Attribution is what makes a relay auditable, not accuracy.** A detail that
  arrives attributed gets checked; the same detail unattributed gets absorbed.
- **Only one session runs twister at a time.** Never `just clean-twister` — it
  deletes every other lane's output. Remove only your own
  `/tmp/twister-out-<lane>*` by name, and confirm with `df -h`, since `rip`
  only moves bytes to the graveyard.
- **Check nothing is live before deleting a shared output dir**, and read the
  matches rather than counting them.
- **Tell the owner before pushing to a branch you do not own** — and as the
  owner, **diff before resetting**: `git diff <your-old-head> <remote>` coming
  back empty proves the rebase reproduced your content byte for byte, which you
  can know *before* running `--hard`.
- **Leave a handoff a successor can act on**: why each choice, what you
  rejected and why, which remaining steps are mechanical and which need
  judgement. **The best handoff is a test** — a `KNOWN_DEFECTS`-style row whose
  failure message says what landed and what the expectation should become. It
  cannot rot, and it finds its own reader.

---

## 8. Two tracker mechanics worth knowing

- **A trailing `(#NNN)` citation is not a closing keyword.** Three issues
  stayed open after their work merged and needed closing by hand. After a
  merge, check the issue actually closed.
- **`main` is governed by a ruleset, not classic branch protection**, so
  `gh api .../branches/main/protection` answers 404 and tells you nothing. Use
  `gh api repos/rodrigopex/objective-z/rulesets`. It requires the branch to be
  current, so a PR reads `BEHIND` — and `gh pr merge` then refuses with
  "required status checks are expected" even when every check has passed. That
  is a rebase, not a retry.
