# Issue body template

The body shape for `gh issue create` (or the web form). Issues live in this
repository's GitHub tracker on [Project #4](https://github.com/users/rodrigopex/projects/4)
— see CLAUDE.md's "Issue tracking". The title is a plain descriptive sentence;
there is no id prefix to assign.

---

## Context

{What you were trying to do — the source file, class, or feature area.}

## Input

```objc
{The smallest snippet that reproduces it. Worth more than any amount of prose.}
```

## Observed

{What actually happened. Paste the exact error text when there is one — the
located diagnostic, or the compiler's own message on the generated C.}

## Expected

{What should have happened instead.}

## Workaround

{What unblocks it in the meantime, or "None".}

## Metadata

| Field | Value |
|-------|-------|
| Filed by | MAINTAINER \| DEV \| QA |
| Blocking | {YES \| NO} |

Only fields GitHub does not already track belong here. Assignee, labels, state
and dates are native fields, and a second copy in the body just drifts out of
step with the real one. `Filed by` earns its place because the workspace runs
several agents and GitHub attributes them all to one account.
