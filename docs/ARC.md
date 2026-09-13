<!-- SPDX-License-Identifier: Apache-2.0 -->

# ARC conformance

**What this file is.** One row per normative rule of Clang's
[Objective-C Automatic Reference Counting specification][spec], in the spec's
own section order, each with exactly one verdict. It exists because
[docs/STATUS.md](STATUS.md) explains *why* this backend's ARC has the shape it
has and *what* the hybrid model is, at length and well — and neither answers
"which of ARC's requirements hold here?" rule by rule. Without that, "does it
leak?" has no mechanical answer, and every ownership defect from #351 to #423
was found by a hand audit prompted by the previous one.

[spec]: https://clang.llvm.org/docs/AutomaticReferenceCounting.html

**How to read a verdict.**

| verdict | meaning |
|---|---|
| `IMPLEMENTED` | oz_static does it. The `evidence` column names the test that proves it. |
| `DELEGATED` | `clang -fobjc-arc` refuses it before oz2c sees it, on every path that dumps an AST. The `evidence` column quotes the diagnostic. |
| `REFUSED` | a located oz_static error. The `evidence` column names the site. |
| `N/A` | structurally impossible on this target. The `evidence` column says why. |
| `GAP` | oz_static neither implements nor refuses it. Every one cites the issue tracking it. |
| `UNEXAMINED` | no construct in the accepted subset reaches it, so it has been neither implemented nor verified. Recorded so it is not mistaken for covered. |

**`DELEGATED` is load-bearing and is the reason oz_static does not reimplement
ARC's front end.** Every Clang path in this project passes `-fobjc-arc` —
asserted, not assumed, by `arc_flag_is_universal.rs` (#443) — and the AST dump
fails the build on *any* error, with two matched exceptions
(`cmake/oz_static.cmake:345`). So a rule Clang refuses is a rule this project
refuses, and writing that down is what turns an inherited property into a claim
someone checked.

Two limits on `DELEGATED`, both of which have already produced defects:

- **tree-sitter is the primary frontend, and it is more permissive than Clang.**
  `oz_static::transpile(source)` — the pure-string form that ~130 tests drive,
  with `Options::default()`'s `require_ast: false` — sees no Clang at all. So
  does `--allow-missing-ast`. A `DELEGATED` verdict holds on the *build* paths
  and not on those two. #428 is what happens when that gap is treated as
  coverage.
- **Apple Clang accepts ARC spellings in plain C that Linux GCC rejects**
  (`docs/STATUS.md:711-752`). So "the C compiler will catch it" is not a
  verdict, and is not written as one here.

**Every verdict below was measured, not recalled.** `docs/STATUS.md:1400-1404`
records that two claims about ARC made from memory during the #359 audit were
wrong and were corrected by dumping the AST for the shape. The probes behind
this file are `clang -fobjc-arc -Weverything` for the Clang half and a
transpile-compile-run under ASan for the oz_static half.

---

## § 1 — Retainable object pointers

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 1.2 | `retain`/`release` are balanced and a null operand is a no-op | `IMPLEMENTED` | `oz_static_retain`/`oz_static_release` both return early on nil (`companion.rs:1603`, `1637`); `behavior/cases/error/release_nil_safe.m` |
| 1.3 | No automatic retain/release merely for using a pointer as an operand | `IMPLEMENTED` | This *is* the model — see STATUS.md "The hybrid model". `arc.rs` emits only necessary traffic |
| 1.3.1 | `ns_consumed` / `ns_consumes_self`: caller retains, callee releases | `GAP` | Clang **accepts** the attribute; oz_static reads it nowhere. #461 |
| 1.3.2 | `ns_returns_retained`: the caller owns the result | `GAP` | Clang accepts; ignored. Harmless alone, corrupting with its negation — #458 |
| 1.3.2 | `ns_returns_not_retained`: overrides a family's implicit +1 | `GAP` | **Use-after-free.** oz_static still calls a `copy`-family method +1 and releases. #458 |
| 1.3.3 | `objc_autoreleaseReturnValue` / `objc_retainAutoreleasedReturnValue` return convention | `N/A` | No autorelease pool exists, so the convention has nowhere to stand. A function must pick +1 or +0 and declare it consistently (STATUS.md:1077-1082) |
| 1.3.4 | `(__bridge T)` transfers nothing | `IMPLEMENTED` | `is_bridging_cast` holds it back from all three ownership questions (`arc.rs:1417`) |
| 1.3.4 | `(__bridge_retained T)` retains, handing +1 to the recipient | `GAP` | **Use-after-free.** No retain emitted; the local is still released at scope exit. #460 |
| 1.3.4 | `(__bridge_transfer T)` releases at the end of the full expression | `GAP` | Leak. No release emitted. #460 |
| 1.4 | Object ↔ non-object conversion is ill-formed without a bridge | `DELEGATED` | `cast of Objective-C pointer type 'Thing *' to C pointer type 'void *' requires a bridged cast` |
| 1.4 | …except a cast to an integer type, which is allowed | `IMPLEMENTED` | Clang accepts it; oz_static deliberately leaks it rather than releasing through an integer slot (#380, `integer_slot_ownership.rs`) |
| 1.5 | Known-semantics conversions (CF audited functions, `cf_returns_*`) | `N/A` | No Core Foundation and no C retainable pointer types on this target |

## § 2 — Ownership qualification

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 2.2 | `__strong` is the default and holds a reference | `IMPLEMENTED` | For ivars from the Clang AST (`astinfo::is_owned_object`, `astinfo.rs:288`); for locals by analysis. Stripped from output (`STRIPPED_ARC_QUALIFIERS`) |
| 2.2 | `__unsafe_unretained` takes no ownership | `IMPLEMENTED` | Honoured at every site — `owned_locals_of:5004`, `retained_bindings:5126`, `managed_object_locals:760`, `collect.rs:438`, `model.rs:259` |
| 2.2 | `__autoreleasing` | `GAP` | **Silently stripped** (`emit.rs:6453`). The one qualifier with neither support nor a diagnostic; no pool exists, so it can mean nothing. #430 settles the precedent in the other direction — `@autoreleasepool` is now refused because a keyword whose mechanism does not exist must not be quietly accepted — and this qualifier is the same case one word smaller. #448 |
| 2.2 | `__weak` is a zeroing weak reference | `N/A` by decision | No runtime can zero one. `__unsafe_unretained` is the supported opt-out and the cycle-breaker |
| 2.2 | …and `__weak` must therefore be refused, not ignored | `REFUSED` (ivars, properties) / `GAP` (everywhere else) | Ivar `emit.rs:6692`, property `collect.rs:558`. On a local, parameter, `static`, file-scope decl, `for`-header decl or C struct field it reaches the generated C verbatim. Clang backstops the build paths with `cannot create __weak reference because the current deployment target does not support weak references`. #448 |
| 2.4 | Property ownership from a modifier (`strong`, `copy`, `assign`, `unsafe_unretained`) | `IMPLEMENTED` | `collect.rs:535`; synthesized strong setter is retain-new / assign / release-old (`emit.rs:7047`) |
| 2.4 | `weak` property | `REFUSED` | `collect.rs:558`. oz_static's own rule, **not** delegated: Clang *accepts* a `weak` property declaration |
| 2.4 | `__autoreleasing` is forbidden on a property | `DELEGATED` | Clang refuses it |
| 2.5.1 | Reading a `__weak` lvalue retains-and-autoreleases | `N/A` | No `__weak` |
| 2.5.2 | Assigning a `__strong` lvalue: retain new, release old | `IMPLEMENTED` | `classify_store` (`emit.rs:590`) — the one predicate both `staticbar` and the emitter ask (#405). Four destinations: ivar, managed local, `static` local, file-scope object, and since #429 a slot spelled `id` is a strong slot in all four |
| 2.5.2 | …including the self-assignment case `c = c` | `IMPLEMENTED` | `LocalStore::BorrowedIdent` emits retain-before-release for exactly this (`emit.rs:3115`) |
| 2.5.3 | Initialization is a null store followed by an assignment | `IMPLEMENTED` | `+alloc` memsets the whole instance (`companion.rs:576`), so every slot starts null |
| 2.5.4 | Destruction is equivalent to assigning null | `IMPLEMENTED` | Scope-exit release (`arc_exit`); ivars via `_oz_release_ivars` (`companion.rs:391`). Since #459 the strong-local set is **body-scoped**: it holds names, and a name left behind by an earlier body used to answer for a later body's local of the same name — releasing a borrowed reference. `managed_locals_are_body_scoped.rs` pins both orderings |
| 2.5.5 | **Moving** a `__strong` lvalue: load, write null, release at end of full-expression | `UNEXAMINED` | Untested and unimplemented; no construct in the accepted subset moves a slot. Recorded so it is not mistaken for covered |
| 2.6.1 | Weak-unavailable types | `N/A` | No `__weak` |
| 2.6.2 | `__autoreleasing` must have automatic storage duration | `N/A` | No pool |
| 2.6.3 | Conversion between differently-qualified pointers is ill-formed | `DELEGATED` | `casting 'Thing *__strong *' to type 'Thing *__weak *' changes retain/release properties of pointer` |
| 2.6.5 | Pass-by-writeback through a `T __autoreleasing *` out-parameter | `GAP` | Clang accepts; `*out = <+1>` asks no ownership question and the caller's variable joins no scope. **Leak, and a fifth untracked strong destination** — the #359 shape at a site nobody walked. #461 |
| 2.6.6 | `__strong` fields of a C struct are managed | `IMPLEMENTED` | Walked and fixed in #359; `ownership_matrix.rs` has the row |
| 2.6.6 | `__strong` in a **union** is ill-formed | `UNEXAMINED` | Clang *accepted* the probe. Unverified whether oz_static would mismanage it; no construct in the tree uses one |
| 2.7.1 | An unqualified retainable pointer is inferred `__strong` | `IMPLEMENTED` | The AST states each ivar's ownership outright under `-fobjc-arc` (`model.rs:340`) |
| 2.7.1 | …except `Class`, inferred `__unsafe_unretained` | `IMPLEMENTED` | Class objects are static and immortal (`companion.rs`, `class_objects.rs`) |
| 2.7.2 | A `T *` parameter infers `__autoreleasing` | `GAP` | Same site as 2.6.5. #461 |

## § 3 — Method families

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 3.1 | A selector is in a family if its first component **is** the family name, or begins with it followed by a non-lowercase character | `GAP` | `CREATE_RULE_SELECTORS.contains(&selector)` is an **exact** match (`arc.rs:219-226`). `-newThing` and `-copyThing` are in a family and are not recognised. Leak where no implementation is visible; subsumed by return-path analysis where one is. #458 |
| 3.2 | Family signature requirements (`alloc`/`copy`/`mutableCopy`/`new` return a retainable pointer) | `IMPLEMENTED` | `returns_object_pointer` (`arc.rs:137`) |
| 3.2 | An `init` method must return an ObjC pointer | `IMPLEMENTED`, and sharper than Clang | `is_initialiser` asks what the method *returns*, not how it is spelled — so `-initialValue` is not an initialiser (#398). Clang accepted a declared-only `- (int)initBadly;` in the probe |
| 3.3 | `objc_method_family(none)` removes a selector from its family | `GAP` | Clang accepts; ignored. #458 |
| 3.4.1 | `alloc`/`copy`/`mutableCopy`/`new` implicitly return retained | `IMPLEMENTED` for the exact spellings | `CREATE_RULE_SELECTORS` (`arc.rs:219`); `selector_ownership_matrix.rs` |
| 3.4.2 | `init` consumes `self` and returns retained | `IMPLEMENTED` | `is_initialiser`; `accounts_for_its_receiver` (`arc.rs:1293`) stops the receiver being released twice (#340) |
| 3.4.2 | A delegate init (`self = [super init]`) is legal only inside an `init` method | `DELEGATED` | Clang: `cannot assign to 'self' outside of a method in the init family` |
| 3.4.2 | `-init` may run more than once; idempotence is the author's contract | `IMPLEMENTED` (memory) / stated contract (semantics) | Release-before-store makes the peak `N` slots however many times it runs (#405, `ivar_store_ordering.rs`). STATUS.md:1165-1207 names the three shapes outside it. Clang has no opinion here |
| 3.4.3 | Related result types (`instancetype`, `+alloc` on class `T` returns `T*`) | `IMPLEMENTED` (partial) | `instancetype` resolved to the declaring class in `collect::extract_method_sig`; `regression_instancetype_covariance.rs`. The spec's *class-send* rule is not separately asserted |

## § 4 — Miscellaneous

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 4.1 | A **send** of `retain`/`release`/`autorelease`/`retainCount` is ill-formed | `DELEGATED` + `REFUSED` | Clang: `ARC forbids explicit message send of 'retain'` (verified for all four, with the selectors declared). Also `staticbar::check_manual_memory_sends` with `ARC_FORBIDDEN_SELECTORS` (#428, #436), covering every receiver spelling |
| 4.1 | `@selector(retain)` is ill-formed | `DELEGATED` | Clang: `ARC forbids use of 'retain' in a @selector` |
| 4.1 | An **implementation** of `retain`/`release`/`autorelease` is ill-formed | `DELEGATED` | Clang: `ARC forbids implementation of 'retain'` |
| 4.1 | A bare **declaration** of one of them | `REFUSED`, wider than Clang | `staticbar.rs:1215-1229`. Clang **accepts** `- (void)release;` in an `@interface` — it refuses the *send* and the *implementation*, not the declaration (measured both ways). CLAUDE.md's "exactly what Clang refuses" is a claim about the **selector set**, and on that it is exact; oz_static's *operation* set is one wider, deliberately, because a declaration whose every call site is an error is dead weight |
| 4.2 | A send of `dealloc` is ill-formed | `DELEGATED` + `REFUSED` | Clang: `ARC forbids explicit message send of 'dealloc'`, including `[super dealloc]` |
| 4.2 | A class may define `-dealloc`; the superclass chain runs automatically | `IMPLEMENTED` | `companion::dealloc_chain` (`companion.rs:71`), most-derived first. Synthesizing it is what made rejecting `[super dealloc]` possible (#428) |
| 4.2 | Instance variables are destroyed after the root `-dealloc` entry | `IMPLEMENTED`, order deliberate | `_oz_release_ivars` runs **after** the `-dealloc` bodies so they can still read the ivars (`companion.rs:1649-1678`) |
| 4.3 | `@autoreleasepool { }` captures and restores the pool | `REFUSED` | Since #430 a located error: *"'@autoreleasepool' has no meaning in the static subset: there is no '-autorelease' … Delete the keyword and keep the braces"*. It used to lower to a plain compound statement, which is the `N/A` this row said before #430 landed — silently accepting a keyword whose mechanism does not exist is the degradation the standing rule forbids |
| 4.3 | Referring to `NSAutoreleasePool` is ill-formed | `N/A` | No such class; Clang's rule is about a Foundation this SDK does not have |
| 4.4 | `self` is externally retained in a non-`init` method | `DELEGATED` | Clang refuses assigning `self` outside the init family; oz_static never releases `self` |
| 4.4 | The for-in loop variable is externally retained | `IMPLEMENTED`, unpinned | Verified by probe: no release is emitted for the loop variable. `behavior_forin.rs` and the four `behavior/cases/forin/` cases exercise the construct but assert nothing about refcount traffic, so nothing would notice if this changed |
| 4.4 | `objc_externally_retained` on a variable | `GAP` | Ignored, and reaches the generated C as an `__attribute__`. #461 |

## § 5 — Optimization

The section that governs the *whole* of `arc.rs`, because the elision is the
value — see STATUS.md, "The hybrid model". Clang is permitted to remove
retain/release pairs; `ObjCARCOpt` does it in an LLVM pass, and here it has to
be done at the source level because GCC will not
(measured: STATUS.md:1084-1102).

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 5.2 | ARC may assume non-ARC code balances sensibly | `IMPLEMENTED` by decision | The C API (`oz_static_retain`/`oz_static_release`) is a deliberate escape hatch, not an enforced invariant (#437) |
| 5.3 | Object liveness: an object in a `__strong` slot is live while a later computation depends on it | `IMPLEMENTED` | This is the **escape** half of every release decision — `alias_chain`, `return_needs_retain` (#351). Asking only provenance produced #351, #352, #359, #360 |
| 5.4 | No object lifetime extension | `IMPLEMENTED` | Scope-exit release rather than a pool; `objc_precise_lifetime` is the default here because there is no imprecise case |
| 5.5 | `objc_precise_lifetime` forces a precise release | `GAP` (benign) | Ignored, and reaches the generated C as an `__attribute__`. Semantically a no-op — every release here is already precise — so the defect is the unlowered spelling, not the behaviour. #461 |

## § 6 — Blocks

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 6 | A block captures a `__strong` variable by retaining it | `REFUSED` | Any capture of an enclosing local, ivar or `self` is a located error (`staticbar::find_capture`, `staticbar.rs:1354`). So there is no `objc_retainBlock`, no `Block_copy`/`Block_release`, and no block-capture ARC to get wrong |
| 6 | `__block` variables are moved | `IMPLEMENTED` (differently) | A `__block` local is promoted to a file-scope `static` (`emit.rs:840`) and excluded from the managed set, so it is a strong slot rather than a moved local |
| 6 | A block pointer is a retainable object pointer | `N/A` | Blocks lower to plain C function pointers; `astinfo::is_owned_object` reports `void (^__strong)(id)` as not owned |
| — | A `return` inside a block literal releases only the block's own scopes | `IMPLEMENTED` | #342; `block_return_scopes.rs` |

## § 7 — Exceptions

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| — | ARC cleanup is exception-safe | `N/A` | `@try`/`@catch` are refused (`staticbar.rs:729`) — no unwinding info on this target. There is no `__attribute__((cleanup))` either; every release is emitted explicitly at each exit |
| — | …and every *other* exit must therefore have its own release arm | `IMPLEMENTED` (3 of 4) | Normal scope end, `return`, `break`/`continue` each have one. **`goto` does not** — `is_jump_statement` suppresses the trailing releases and nothing emits them. #454 |

---

## Gaps, as a work queue

Ordered by direction, because a leak and a double free are not the same bug
(STATUS.md:2216-2219).

**Corrupting — these free an object that is still referenced:**

| issue | § | what |
|---|---|---|
| #458 | 1.3.2, 3.1, 3.3 | `ns_returns_not_retained` on a family selector: oz_static releases what ARC says it does not own |
| #460 | 1.3.4 | `__bridge_retained` emits no retain, so the C side is handed a freed slot |

**Leaking:**

| issue | § | what |
|---|---|---|
| #460 | 1.3.4 | `__bridge_transfer` emits no release for the +1 it took over |
| #461 | 2.6.5, 2.7.2 | an out-parameter store is an untracked strong destination |
| #458 | 3.1 | a family-named selector with no visible implementation is treated as +0 |
| #454 | — | `goto` emits no scope releases |
| #450 | — | `arc::analyze`'s fixed point terminates early on a C-factory chain |

**Silent degrade — accepted and then ignored, which this project's standing
rule forbids:**

| issue | § | what |
|---|---|---|
| #448 | 2.2 | `__weak` outside an ivar or property; `__autoreleasing` anywhere |
| #461 | 1.3.1, 4.4, 5.5 | the five ARC `__attribute__`s reach the generated C unlowered |

**`UNEXAMINED` — recorded, not scheduled:**

| § | what |
|---|---|
| 2.5.5 | moving a `__strong` lvalue — no construct in the subset does it |
| 2.6.6 | `__strong` in a union — Clang accepted the probe; unverified here |
| 3.4.3 | the class-send half of related result types is not separately asserted |

## What this file does not cover

The elision itself. Whether a *particular* release is correctly placed is
`ownership_matrix.rs` (every sink a `+1` reaches, by refcount count) and
`selector_ownership_matrix.rs` (every selector and construct, by observed
output). This file says which *rules* apply; those say whether the emitter
honours them at each site. A new sink, selector or construct needs a row
there; a new *rule* needs a row here.

Three known blind spots in those two, worth knowing before trusting a green
run:

- counting cannot see **which** pointer a release names, which is what #398 got
  wrong;
- eager allocation balances, so only observing a side effect catches it (#376);
- `ownership_matrix.rs` drives `oz_static::transpile` with no AST, i.e. the
  fall-back rule that leaks every `id`-typed ivar — a configuration no shipped
  path uses.

And #459 was expressible in neither, because both assert on a single emitted
function while that defect needs two methods *and* their order — which is why
it has its own file, `managed_locals_are_body_scoped.rs`, rather than a row.
