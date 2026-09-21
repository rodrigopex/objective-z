# Apple objc4-derived behavioural specifications

Tests here are **not** copies or adaptations of Apple code. Apple's objc4 is
APSL 2.0 and is treated as a *behavioural specification source*: the behaviour
is studied, described in original language, and the fixture written
independently. No APSL-licensed code has been copied, modified or adapted into
these files.

Upstream references:

- <https://opensource.apple.com/source/objc4/>
- Apple's Objective-C runtime and ARC documentation
- Clang's [ARC specification](https://clang.llvm.org/docs/AutomaticReferenceCounting.html),
  which is the normative text `docs/ARC.md` gives a verdict per rule

## What every case must state

A header comment carrying these fields, in this order:

| Field | Content |
|---|---|
| `Proves:` | the exact behaviour the `_test.c` asserts. Not the feature area it belongs to |
| `Does not prove:` | what the evidence does not reach, and where that is covered instead |
| `Inspiration:` | the Apple document or objc4 test the behaviour was read from |
| `Upstream revision:` | the pinned release or commit it was read at |
| `Omitted from upstream:` | the runtime-oriented behaviour dropped because it does not apply here |
| `Authorship:` | that the implementation is independently written, no APSL code copied |

`Proves:` is doing double duty for the plan that introduced this list, which
asked separately for "the exact behavior being tested" and "what the case
proves". In practice those are one sentence, and splitting them produced two
fields that always agreed. Six fields, not seven, and this note is why.

**Both surviving cases predate the rule and are marked, not exempted.** Each
records `Upstream revision: not pinned at authoring time (pre-#596)`. A *new*
case with that line is a review failure; an old one carrying it is a debt that
is visible rather than invented.

## Naming rule

> A case must be named after the behaviour it asserts, not after a broader
> upstream feature for which it only serves as a proxy.

This was not free advice. Until #596 this directory held five cases, of which
`struct_return` declared no struct, `weak_zeroing_dealloc` contained no `__weak`
and no `-dealloc`, and `nil_return_types` sent no message at all. Three of the
five also duplicated stronger evidence elsewhere in the tree and were deleted;
one was renamed. The reason all four survived review is mechanical:
`tests/adapted/test_apple_spec.py` asserts only `result.returncode == 0`, so a
case whose assertions never reach its named construct is indistinguishable from
one that does.

## Choosing an oracle

**Prefer `oz_retain_count` to slab reuse.** Reading a refcount is a plain C call
and the one refcount entry point this project sanctions (`docs/ARC.md` s 1.2,
and CLAUDE.md's "ARC is the only ownership model"). It is **not** runtime
introspection, and several adaptations across this corpus removed it on the
belief that it was -- trading an exact oracle for a weaker one. A refcount read
distinguishes "retained" from "still alive because someone else holds it"; slab
reuse cannot.

**If a claim does rest on slab reuse, pair it with a control.** A one-slot pool
proves nothing unless the test also asserts that the pool is really one slot --
otherwise the claim passes vacuously on a wider pool with nothing ever released.
`retain_cycle_break_test.c` asserts the control first, then the claim, and is
the template to copy (#455).

## Adding a case

Only when it contributes at least one of: a new source-level construct, a new
ownership/lifetime/ABI edge, a new author-visible guarantee, or a clearer
end-to-end oracle. Search `tests/behavior/`, the other `tests/adapted/`
directories and `tools/oz2c/tests/` first -- three of the five original cases
here were duplicates, and the behaviour corpus and Rust suite are usually the
stronger evidence. Prefer a cross-reference from
[docs/OBJECTIVE_C_DIALECT.md](../../../docs/OBJECTIVE_C_DIALECT.md) over a
second test of the same property.

Expected failures do not belong here. This corpus is positive behavioural
evidence; a refusal belongs in `tools/oz2c/tests/`.

## Current cases

| Case | Proves | Apple provenance |
|---|---|---|
| `retain_cycle_break` | a non-owning `__unsafe_unretained` back-reference breaks a cycle; both slots recycle, with a one-slot control | Apple ARC documentation |
| `multipart_selector_returns` | a two-part selector delivers each argument to its own ivar; computed returns compute | **None.** Inspired by the language specification, not by any objc4 test. It sits here for historical reasons and is a candidate for relocation |
