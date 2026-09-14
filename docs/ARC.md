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
| `IMPLEMENTED` | oz2c does it. The `evidence` column names the test that proves it. |
| `DELEGATED` | `clang -fobjc-arc` refuses it before oz2c sees it, on every path that dumps an AST. The `evidence` column quotes the diagnostic. |
| `REFUSED` | a located oz2c error. The `evidence` column names the site. |
| `N/A` | structurally impossible on this target. The `evidence` column says why. |
| `GAP` | oz2c neither implements nor refuses it. Every one cites the issue tracking it. |
| `UNEXAMINED` | no construct in the accepted subset reaches it, so it has been neither implemented nor verified. Recorded so it is not mistaken for covered. |

**`DELEGATED` is load-bearing and is the reason oz2c does not reimplement
ARC's front end.** Every Clang path in this project passes `-fobjc-arc` —
asserted, not assumed, by `arc_flag_is_universal.rs` (#443) — and the AST dump
fails the build on *any* error, with two matched exceptions
(`cmake/oz2c.cmake:345`). So a rule Clang refuses is a rule this project
refuses, and writing that down is what turns an inherited property into a claim
someone checked.

Two limits on `DELEGATED`, both of which have already produced defects:

- **tree-sitter is the primary frontend, and it is more permissive than Clang.**
  `oz2c::transpile(source)` — the pure-string form that ~130 tests drive,
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
transpile-compile-run under ASan for the oz2c half.

---

## § 1 — Retainable object pointers

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 1.2 | `retain`/`release` are balanced and a null operand is a no-op | `IMPLEMENTED` | `oz_retain`/`oz_release` both return early on nil (`companion.rs:1603`, `1637`); `behavior/cases/error/release_nil_safe.m` |
| 1.3 | No automatic retain/release merely for using a pointer as an operand | `IMPLEMENTED` | This *is* the model — see STATUS.md "The hybrid model". `arc.rs` emits only necessary traffic |
| 1.3.1 | `ns_consumed` / `ns_consumes_self`: caller retains, callee releases | `REFUSED` | Same walk (#458). They move an argument's release to the callee, which oz2c does not model — a caller and callee that disagree about who releases is the corrupting direction |
| 1.3.2 | `ns_returns_retained`: the caller owns the result | `REFUSED` | A located error since #458 (`staticbar::check_ownership_attributes`). Refused rather than implemented on #430's precedent: a spelling whose meaning is a mechanism the backend does not have must not be quietly accepted. Reading it needs #453 |
| 1.3.2 | `ns_returns_not_retained`: overrides a family's implicit +1 | `REFUSED` | Same walk. This is the one that reached a **use-after-free**: ARC read it and said +0, oz2c called `-copy` +1 and released the caller's object. Refusing it is also what makes § 3.1's family rule safe to widen — no attribute can contradict a family (#458) |
| 1.3.3 | `objc_autoreleaseReturnValue` / `objc_retainAutoreleasedReturnValue` return convention | `N/A` | No autorelease pool exists, so the convention has nowhere to stand. A function must pick +1 or +0 and declare it consistently (STATUS.md:1077-1082) |
| 1.3.4 | `(__bridge T)` transfers nothing | `IMPLEMENTED` | `is_bridging_cast` holds it back from all three ownership questions (`arc.rs:1417`) |
| 1.3.4 | `(__bridge_retained T)` retains, handing +1 to the recipient | `REFUSED` | A located error since #460 (`staticbar::check_bridging_casts`). It emitted no retain, so the local was still released at scope exit and C was handed a freed slot — and the stale read *succeeded* first, which is why it was silent. Refused rather than implemented: the tree has zero uses, and emission is sequenced after #462's respelling of the emitted ABI |
| 1.3.4 | `(__bridge_transfer T)` releases at the end of the full expression | `REFUSED` | Same walk (#460). It emitted no release, stranding the +1 it took over from C |
| 1.4 | Object ↔ non-object conversion is ill-formed without a bridge | `DELEGATED` | `cast of Objective-C pointer type 'Thing *' to C pointer type 'void *' requires a bridged cast` |
| 1.4 | …except a cast to an integer type, which is allowed | `IMPLEMENTED` | Clang accepts it; oz2c deliberately leaks it rather than releasing through an integer slot (#380, `integer_slot_ownership.rs`) |
| 1.5 | Known-semantics conversions (CF audited functions, `cf_returns_*`) | `N/A` | No Core Foundation and no C retainable pointer types on this target |

## § 2 — Ownership qualification

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 2.2 | `__strong` is the default and holds a reference | `IMPLEMENTED` | For ivars from the Clang AST (`astinfo::is_owned_object`, `astinfo.rs:288`); for locals by analysis. Stripped from output (`STRIPPED_ARC_QUALIFIERS`) |
| 2.2 | `__unsafe_unretained` takes no ownership | `IMPLEMENTED` | Honoured at every site — `owned_locals_of:5004`, `retained_bindings:5126`, `managed_object_locals:760`, `collect.rs:438`, `model.rs:259` |
| 2.2 | `__autoreleasing` | `GAP` | **Silently stripped** (`emit.rs:6453`). The one qualifier with neither support nor a diagnostic; no pool exists, so it can mean nothing. #430 settles the precedent in the other direction — `@autoreleasepool` is now refused because a keyword whose mechanism does not exist must not be quietly accepted — and this qualifier is the same case one word smaller. #448 |
| 2.2 | `__weak` is a zeroing weak reference | `N/A` by decision | No runtime can zero one. `__unsafe_unretained` is the supported opt-out and the cycle-breaker |
| 2.2 | …and `__weak` must therefore be refused, not ignored | `REFUSED` | `staticbar::check_weak_qualifier`, one whole-tree walk over `type_qualifier` nodes, so every position is one function's answer (#448). Ten positions measured: nine reached the generated C before that walk and only an ivar was refused. **The property is two spellings, not one** — `collect.rs`'s attribute parse refuses `@property (weak)`, and `@property () __weak T *w` is the qualifier, which was *not* covered; this row previously recorded the position as done on the strength of the attribute rule. Pinned in `weak_every_position.rs`, including that `__unsafe_unretained` is accepted in all ten, so the remedy is one the checker honours (#425's bar) |
| 2.4 | Property ownership from a modifier (`strong`, `copy`, `assign`, `unsafe_unretained`) | `IMPLEMENTED` | `collect.rs:535`; synthesized strong setter is retain-new / assign / release-old (`emit.rs:7047`) |
| 2.4 | `weak` property | `REFUSED` | `collect.rs:558`. oz2c's own rule, **not** delegated: Clang *accepts* a `weak` property declaration |
| 2.4 | `__autoreleasing` is forbidden on a property | `DELEGATED` | Clang refuses it |
| 2.5.1 | Reading a `__weak` lvalue retains-and-autoreleases | `N/A` | No `__weak` |
| 2.5.2 | Assigning a `__strong` lvalue: retain new, release old | `IMPLEMENTED` | `classify_store` (`emit.rs:590`) — the one predicate both `staticbar` and the emitter ask (#405). Four destinations: ivar, managed local, `static` local, file-scope object, and since #429 a slot spelled `id` is a strong slot in all four |
| 2.5.2 | …including the self-assignment case `c = c` | `IMPLEMENTED` | `LocalStore::BorrowedIdent` emits retain-before-release for exactly this (`emit.rs:3115`) |
| 2.5.2 | …including a collection taking ownership of a boxed literal's elements | `IMPLEMENTED` | Each element of `@[…]` / `@{…}` is passed through when it is already `+1` and retained when borrowed (`emit::is_fresh_alloc`), and `OZArray_oz_free` / `OZDictionary_oz_free` release them. Until #449 that question was answered by a node-kind whitelist, so `@[[Thing alloc]]` was retained *and* already owned — one reference leaked per element, keys and values alike. It now asks `arc::binds_ownership` like every other binding site; the four literal kinds stay local because a nested literal's `+1` is created by this emitter and is invisible to `arc.rs`. Pinned by `literal_element_ownership.rs` |
| 2.5.3 | Initialization is a null store followed by an assignment | `IMPLEMENTED` | `+alloc` memsets the whole instance (`companion.rs:576`), so every slot starts null |
| 2.5.4 | Destruction is equivalent to assigning null | `IMPLEMENTED` | Scope-exit release (`arc_exit`); ivars via `_oz_release_ivars` (`companion.rs:391`). Since #459 the strong-local set is **body-scoped**: it holds names, and a name left behind by an earlier body used to answer for a later body's local of the same name — releasing a borrowed reference. `managed_locals_are_body_scoped.rs` pins both orderings |
| 2.5.5 | **Moving** a `__strong` lvalue: load, write null, release at end of full-expression | `UNEXAMINED` | Untested and unimplemented; no construct in the accepted subset moves a slot. Recorded so it is not mistaken for covered |
| 2.6.1 | Weak-unavailable types | `N/A` | No `__weak` |
| 2.6.2 | `__autoreleasing` must have automatic storage duration | `N/A` | No pool |
| 2.6.3 | Conversion between differently-qualified pointers is ill-formed | `DELEGATED` | `casting 'Thing *__strong *' to type 'Thing *__weak *' changes retain/release properties of pointer` |
| 2.6.5 | Pass-by-writeback through a `T __autoreleasing *` out-parameter | `REFUSED` | A located error since #461 (`staticbar::check_out_parameter_stores`). Refused on #430's precedent: ARC's answer is writeback through an *autoreleased* temporary and there is no pool to autorelease into. Keyed on the `+1` (`arc::binds_ownership`), not on the shape — `OZArray.h:28` declares `objects:(__unsafe_unretained id *)stackbuf`, which is the same shape and correct, so a shape-keyed refusal would refuse fast enumeration |
| 2.6.6 | `__strong` fields of a C struct are managed | `IMPLEMENTED` | Walked and fixed in #359; `ownership_matrix.rs` has the row |
| 2.6.6 | `__strong` in a **union** is ill-formed | `UNEXAMINED` | Clang *accepted* the probe. Unverified whether oz2c would mismanage it; no construct in the tree uses one |
| 2.7.1 | An unqualified retainable pointer is inferred `__strong` | `IMPLEMENTED` | The AST states each ivar's ownership outright under `-fobjc-arc` (`model.rs:340`) |
| 2.7.1 | …except `Class`, inferred `__unsafe_unretained` | `IMPLEMENTED` | Class objects are static and immortal (`companion.rs`, `class_objects.rs`) |
| 2.7.2 | A `T *` parameter infers `__autoreleasing` | `REFUSED` | Same site as 2.6.5, and the reason the refusal cannot be keyed on the qualifier: here Clang *infers* it and the token is never written, so nothing spelling-keyed can see it. The store is what is seen |

## § 3 — Method families

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 3.1 | A selector is in a family if its first component **is** the family name, or begins with it followed by a non-lowercase character | `IMPLEMENTED` | `arc::create_rule_family_of` since #458, replacing an exact match against six spellings. **Guarded by the return type**, because the corpus holds the counterexample: `- (int)allocOk` is in the `alloc` family by spelling and releasing it hands `oz_release` an `int` — #398 exactly. Clang accepts `- (int)allocOk` silently, so the guard is oz2c's own. Mirrors `is_initialiser`; `method_family_ownership.rs` pins nine family boundaries |
| 3.2 | Family signature requirements (`alloc`/`copy`/`mutableCopy`/`new` return a retainable pointer) | `IMPLEMENTED` | `returns_object_pointer` (`arc.rs:137`) |
| 3.2 | An `init` method must return an ObjC pointer | `IMPLEMENTED`, and sharper than Clang | `is_initialiser` asks what the method *returns*, not how it is spelled — so `-initialValue` is not an initialiser (#398). Clang accepted a declared-only `- (int)initBadly;` in the probe |
| 3.3 | `objc_method_family(none)` removes a selector from its family | `REFUSED` | Same walk as § 1.3.2 (#458): it reassigns a family outright, which is precisely what the widened family rule must not have contradicted underneath it |
| 3.4.1 | `alloc`/`copy`/`mutableCopy`/`new` implicitly return retained | `IMPLEMENTED` for the exact spellings | `CREATE_RULE_SELECTORS` (`arc.rs:219`); `selector_ownership_matrix.rs` |
| 3.4.2 | `init` consumes `self` and returns retained | `IMPLEMENTED` | `is_initialiser`; `accounts_for_its_receiver` (`arc.rs:1293`) stops the receiver being released twice (#340) |
| 3.4.2 | A delegate init (`self = [super init]`) is legal only inside an `init` method | `DELEGATED` | Clang: `cannot assign to 'self' outside of a method in the init family` |
| 3.4.2 | `-init` may run more than once; idempotence is the author's contract | `IMPLEMENTED` (memory) / stated contract (semantics) | Release-before-store makes the peak `N` slots however many times it runs (#405, `ivar_store_ordering.rs`). STATUS.md:1165-1207 names the three shapes outside it. Clang has no opinion here |
| — | A receiver's class must resolve the same way for ownership as for dispatch | `IMPLEMENTED` | Since #481 `arc::collect_declared_types` knows `method_parameter`, the Objective-C parameter kind that appeared **zero** times in all of `arc.rs`. Before it, the emitter resolved a parameter receiver from `ctx.scope` and emitted a *static* call while `arc` answered `None`, polled every implementor, and read a disagreement as borrowed — so a statically dispatched send took its ownership from an ambiguous poll over classes it can never reach, and `emit::dynamic_dispatch_call`'s `Ambiguous` refusal could not catch it because that guards *dynamic* sends. Needed three coinciding conditions, so it had never been seen; `parameter_receiver_class.rs` pins the shape and the two that do not reproduce |
| 3.4.3 | Related result types (`instancetype`, `+alloc` on class `T` returns `T*`) | `IMPLEMENTED` (partial) | `instancetype` resolved to the declaring class in `collect::extract_method_sig`; `regression_instancetype_covariance.rs`. The spec's *class-send* rule is not separately asserted |

## § 4 — Miscellaneous

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 4.1 | A **send** of `retain`/`release`/`autorelease`/`retainCount` is ill-formed | `DELEGATED` + `REFUSED` | Clang: `ARC forbids explicit message send of 'retain'` (verified for all four, with the selectors declared). Also `staticbar::check_manual_memory_sends` with `ARC_FORBIDDEN_SELECTORS` (#428, #436), covering every receiver spelling |
| 4.1 | `@selector(retain)` is ill-formed | `DELEGATED` | Clang: `ARC forbids use of 'retain' in a @selector` |
| 4.1 | An **implementation** of `retain`/`release`/`autorelease` is ill-formed | `DELEGATED` | Clang: `ARC forbids implementation of 'retain'` |
| 4.1 | A bare **declaration** of one of them | `REFUSED`, wider than Clang | `staticbar.rs:1215-1229`. Clang **accepts** `- (void)release;` in an `@interface` — it refuses the *send* and the *implementation*, not the declaration (measured both ways). CLAUDE.md's "exactly what Clang refuses" is a claim about the **selector set**, and on that it is exact; oz2c's *operation* set is one wider, deliberately, because a declaration whose every call site is an error is dead weight |
| 4.2 | A send of `dealloc` is ill-formed | `DELEGATED` + `REFUSED` | Clang: `ARC forbids explicit message send of 'dealloc'`, including `[super dealloc]` |
| 4.2 | A class may define `-dealloc`; the superclass chain runs automatically | `IMPLEMENTED` | `companion::dealloc_chain` (`companion.rs:71`), most-derived first. Synthesizing it is what made rejecting `[super dealloc]` possible (#428) |
| 4.2 | Instance variables are destroyed after the root `-dealloc` entry | `IMPLEMENTED`, order deliberate | `_oz_release_ivars` runs **after** the `-dealloc` bodies so they can still read the ivars (`companion.rs:1649-1678`) |
| 4.3 | `@autoreleasepool { }` captures and restores the pool | `REFUSED` | Since #430 a located error: *"'@autoreleasepool' has no meaning in the static subset: there is no '-autorelease' … Delete the keyword and keep the braces"*. It used to lower to a plain compound statement, which is the `N/A` this row said before #430 landed — silently accepting a keyword whose mechanism does not exist is the degradation the standing rule forbids |
| 4.3 | Referring to `NSAutoreleasePool` is ill-formed | `N/A` | No such class; Clang's rule is about a Foundation this SDK does not have |
| 4.4 | `self` is externally retained in a non-`init` method | `DELEGATED` | Clang refuses assigning `self` outside the init family; oz2c never releases `self` |
| 4.4 | The for-in loop variable is externally retained | `IMPLEMENTED`, unpinned | Verified by probe: no release is emitted for the loop variable. `behavior_forin.rs` and the four `behavior/cases/forin/` cases exercise the construct but assert nothing about refcount traffic, so nothing would notice if this changed |
| 4.4 | `objc_externally_retained` on a variable | `IMPLEMENTED` (as a strip) | Ignored, which changes no answer, and no longer reaches the generated C: stripped at all four positions by `emit::is_stripped_arc_spelling` (#461). Not *refused*, unlike the five in 1.3.1/1.3.2 — those carry an ownership answer, and silently dropping one is a use-after-free rather than a leak |

## § 5 — Optimization

The section that governs the *whole* of `arc.rs`, because the elision is the
value — see STATUS.md, "The hybrid model". Clang is permitted to remove
retain/release pairs; `ObjCARCOpt` does it in an LLVM pass, and here it has to
be done at the source level because GCC will not
(measured: STATUS.md:1084-1102).

| § | Rule | Verdict | Evidence |
|---|---|---|---|
| 5.2 | ARC may assume non-ARC code balances sensibly | `IMPLEMENTED` by decision | The C API (`oz_retain`/`oz_release`) is a deliberate escape hatch, not an enforced invariant (#437) |
| 5.3 | Object liveness: an object in a `__strong` slot is live while a later computation depends on it | `IMPLEMENTED` | This is the **escape** half of every release decision — `alias_chain`, `return_needs_retain` (#351). Asking only provenance produced #351, #352, #359, #360 |
| 5.4 | No object lifetime extension | `IMPLEMENTED` | Scope-exit release rather than a pool; `objc_precise_lifetime` is the default here because there is no imprecise case |
| 5.5 | `objc_precise_lifetime` forces a precise release | `IMPLEMENTED` (as a strip) | Semantically a no-op — every release here is already precise — so the defect was only the unlowered spelling, and it is stripped since #461. **Severity note, measured:** this was filed as an instance of #428's macOS-clang-versus-Linux-gcc trap and is the reverse. The SDK's `arm-zephyr-eabi-gcc` 14.3.0 *warns* `attribute directive ignored [-Wattributes]` and compiles (an error only under `CONFIG_COMPILER_WARNINGS_AS_ERRORS`, off by default and unset here), while Apple clang **errors**: `objc_precise_lifetime only applies to retainable types`. Clang is the stricter compiler here |

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
| — | …and every *other* exit must therefore have its own release arm | `IMPLEMENTED` | Normal scope end (`arc_exit`), `return`, `break`/`continue` (`render_loop_jump`) and — since #454 — `goto` (`render_goto`) each have one. `goto` was the fourth and last: `is_jump_statement` counted it, which *suppresses* the trailing release, and no renderer replaced it. Needed no scope graph — the label's own position plus `ArcScope::start_byte` answers it. Clang refuses the hard shapes, jumping *into* a scope or *over* a declaration (§ 2.6.6) |

---

## Gaps, as a work queue

Ordered by direction, because a leak and a double free are not the same bug
(STATUS.md:2216-2219).

**Corrupting — these free an object that is still referenced:**

**None.** All four were closed in one pass: #458 (`ns_returns_not_retained`
against the create rule), #459 (a name-keyed strong-local set outliving its
body), and #460's two bridging casts. Every one was found by walking this
document's rules rather than by a report, and every one came from source
`clang -fobjc-arc -Weverything` accepts silently.

That is worth keeping as a claim someone can falsify rather than a boast: it
means the shapes *this file has verdicts for* no longer free a live object. It
does not mean the emitter honours every verdict at every site — see "What this
file does not cover".

**Leaking:**

| issue | § | what |
|---|---|---|
| #461 | 2.6.5, 2.7.2 | **closed** — refused, keyed on the `+1` so the SDK's borrowed fast-enumeration buffer stays legal |

**Silent degrade — accepted and then ignored, which this project's standing
rule forbids:**

| issue | § | what |
|---|---|---|
| #448 | 2.2 | `__autoreleasing` anywhere — silently stripped by `emit::STRIPPED_ARC_QUALIFIERS`. The `__weak` half is closed; this half is a verdict question (refuse, on #430's precedent) rather than a missing walk, and is awaiting that decision |
| #461 | 4.4, 5.5 | **closed** — the two are stripped at all four positions by one predicate. Narrowed by #458, which *refuses* the three that carry ownership meaning; what was left was the two that do not, and a lowering defect is all it was |

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
- `ownership_matrix.rs` drives `oz2c::transpile` with no AST — a configuration
  no shipped path uses. **But the second half of this used to read "i.e. the
  fall-back rule that leaks every `id`-typed ivar", and that does not bite.**
  Measured 2026-09-13 (#453's audit): the AST-less fallback (`model.rs:373-378`)
  differs from Clang's answer *only* for an `id`-typed **ivar**, and `DECLS`
  (`ownership_matrix.rs:47-112`) declares none — `Holder` has `Thing *_ivar` and
  `Thing *_arr[2]`, and `g_id_global` is file-scope rather than an ivar. So every
  row in that file gives the same answer with or without an AST.

  Pinning the AST-less form is therefore the *right* thing to do rather than a
  compromise: it isolates a 1040-line regression net from SDK clang availability.
  What is missing is an assertion that `DECLS` declares no `id`-typed ivar, so
  the day someone adds one the file stops silently pinning a rule no shipped path
  uses.

And #459 was expressible in neither, because both assert on a single emitted
function while that defect needs two methods *and* their order — which is why
it has its own file, `managed_locals_are_body_scoped.rs`, rather than a row.
