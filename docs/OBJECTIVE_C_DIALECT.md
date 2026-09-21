<!-- SPDX-License-Identifier: Apache-2.0 -->

# The Objective-Z dialect

**What this file is.** One row per author-visible Objective-C construct, each
with exactly one verdict, so "can I write X?" has an answer that does not
require reading `tools/oz2c/src/staticbar.rs` (~3,600 lines of incident-driven
checks) or [docs/STATUS.md](STATUS.md) (~5,500 lines of lab notes). Those two
remain authoritative about *why*; this file answers *whether*.

It exists because the page a new author actually reads had drifted from the
code. Until #583 the README's ARC Guide taught four mechanisms the standing
rules refuse -- `__weak` "panics at runtime" where it is a located error,
`[super dealloc]`, `__bridge_retained` as the working exception, and an
`OZTimer` that was deleted in #193 -- and two subset boundaries had a
diagnostic and no user-facing line at all. A document describing a mechanism
the code does not have will describe it wrongly.

## How to read a verdict

The vocabulary is [docs/ARC.md](ARC.md)'s, deliberately unchanged, because a
second spelling of the same six ideas is a second thing to keep in step:

| verdict | meaning |
|---|---|
| `IMPLEMENTED` | oz2c lowers it. The evidence column names the test that proves it. |
| `DELEGATED` | `clang -fobjc-arc` refuses it before oz2c sees it, on every path that dumps an AST. |
| `REFUSED` | a located oz2c error. The evidence column names the check. |
| `N/A` | structurally impossible on this target. The evidence says why. |
| `GAP` | oz2c neither lowers nor refuses it. Every one cites the issue tracking it. |
| `UNEXAMINED` | no test reaches it, so it has been neither verified nor refused. Recorded so it is not mistaken for covered. |

**A `GAP` is the state this file exists to make visible**, and the one to read
first: it means the construct compiles to *something* and nobody has checked
what.

**Row ids are the stable handle.** `send.nil.scalar` survives any rewording of
the prose beside it, and `tools/oz2c/tests/dialect_ledger.rs` keys on the ids
rather than on the wording, so this document's sentences are not part of a test
API.

**Where the plan's seven fields went.** This table has five columns, not seven.
*Contract* carries the limitations, because a limitation an author must respect
*is* the contract and splitting them produced two cells that restated each
other. *Provenance* is the issue number in *Evidence*, which is the only
provenance this repo actually keeps. Said here so the collapse is recorded
rather than silent.

**Two limits on `DELEGATED`, inherited from ARC.md and equally true here.**
tree-sitter is the primary frontend and is more permissive than Clang, so the
pure `oz2c::transpile(source)` form and `--allow-missing-ast` see no Clang at
all; and Apple Clang accepts spellings in plain C that Linux GCC rejects. "The
compiler will catch it" is not a verdict.

---

## 1 — Classes, inheritance, categories

| id | Construct | Verdict | Contract | Evidence |
|---|---|---|---|---|
| `class.interface` | `@interface C : Super` | `IMPLEMENTED` | Becomes `struct C` with the superclass's struct as its first member, plus a dispatch row and a slab. Single inheritance only. | `collect.rs`; every corpus case |
| `class.root` | a class with no superclass | `IMPLEMENTED` | `--root-class` names it; `OZObject` by default. | `--root-class`; `object_protocol.rs` |
| `class.forward` | `@class C;` | `IMPLEMENTED` | Lowered to a comment. The real declaration must be reachable by `#import`. | `objc_node_disposition.rs` (`class_declaration`) |
| `class.extension` | `@interface C ()` | `IMPLEMENTED` | Members merge into the class. | `generics.rs`; `objc_node_disposition.rs` |
| `category.methods` | `@interface C (Cat)` adding methods | `IMPLEMENTED` | Merged into one dispatch table at build time; a category method is indistinguishable from a primary one in the output. | `adapted/bucket_b/dispatch_category_merge.m` |
| `category.override` | a category overriding an inherited method | `IMPLEMENTED` | Resolved at build time, whole-program. | `adapted/bucket_b/dispatch_category_merge.m` |
| `category.property` | a category adding a `@property` | `UNEXAMINED` | No test declares one. | — |
| `class.alias` | `@compatibility_alias A B;` | `UNEXAMINED` | Classified as an ObjC-only kind and gated on output, but no test declares one. | `objc_node_disposition.rs` (`compatibility_alias_declaration`) |
| `class.decl-impl` | a method declared in `@interface` and defined nowhere | `REFUSED` | `'-missing' is declared on 'Probe' and defined nowhere`, naming the C symbol that would have been called. A mismatched selector piece is refused the same way. | `decl_impl_reconciliation.rs::declared_and_never_defined_send_rejected`, `:72`, `:109`; #566 |
| `class.impl-only` | `@implementation C` with no `@interface C` | `REFUSED` | `'@implementation C' has no '@interface C' in this source`. Generated a class missing its synthesized allocator before #567. | `collect.rs`; `decl_impl_reconciliation.rs`; #567 |
| `method.private` | a method **defined** but never **declared** | `IMPLEMENTED` | Deliberately accepted -- this is how a private method is written. Only the reverse direction is an error. | `decl_impl_reconciliation.rs::defined_and_never_declared_private_method_accepted` |

**One selector name, one return type, whole program.** Two unrelated classes
may not declare the same selector with different return types (#290). This is
the constraint authors hit first and it is not obvious from any single file.

## 2 — Methods and message sends

| id | Construct | Verdict | Contract | Evidence |
|---|---|---|---|---|
| `send.instance` | `[obj method]` | `IMPLEMENTED` | A direct call when the receiver's class is known whole-program; a switch on `_meta.class_id` when an override exists. No `objc_msgSend`. | `behavior_dispatch.rs`; `behavior/cases/dispatch/` |
| `send.class` | `[C method]` | `IMPLEMENTED` | `C_method_cls(void)` -- no receiver parameter. | `class_side_resolution.rs` |
| `send.super` | `[super method]` | `IMPLEMENTED` | Always a direct call to the inherited implementation, so it cannot re-enter an override. | `behavior_dispatch.rs::super_calls_parent_dispatch`; `behavior/cases/dispatch/super_calls_parent.m`; on a board, `tests/zephyr/src/test_dispatch.c:14` |
| `send.nil.scalar` | send to `nil`, scalar return | `IMPLEMENTED` | Answers zero. **Conditional on `CONFIG_OBJZ_NIL_SAFE_SENDS` (default `y`)**; with `n` the guards are removed and a nil receiver is a null dereference. | `nil_receiver.rs::a_send_to_nil_answers_zero`, `:494`; #528 |
| `send.nil.object` | send to `nil`, object return | `IMPLEMENTED` | Answers `nil`. Same configuration. | `nil_receiver.rs::the_guards_zero_is_valid_for_every_return_shape` |
| `send.nil.void` | void send to `nil` | `IMPLEMENTED` | A no-op; the body is not entered. Same configuration. | `nil_receiver.rs::a_send_to_nil_answers_zero` |
| `send.nil.aggregate` | send to `nil`, struct/union/enum return | `IMPLEMENTED` | A zeroed value via `(T){0}`. Same configuration. | `nil_receiver.rs::a_struct_returning_send_to_nil_answers_a_zeroed_struct`, `:440`, `:494`; #533 |
| `method.declaration` | `- (T)name:(A)a b:(B)b;` | `IMPLEMENTED` | Multi-part selectors deliver each argument to its own parameter. | `adapted/apple_spec/multipart_selector_returns.m` |
| `return.scalar` | scalar and computed returns | `IMPLEMENTED` | — | `adapted/apple_spec/multipart_selector_returns.m` |
| `return.struct` | `- (struct S)v;` and a typedef of one | `IMPLEMENTED` | By value, across a header/impl split, declaration order preserved. **Proven on the host only** -- no test returns a struct on ARM or RISC-V. | `nil_receiver.rs::a_struct_returning_send_to_nil_answers_a_zeroed_struct`, `:440`; `split_output.rs::hoisted_c_types_keep_source_order_so_a_typedef_can_precede_its_user` |
| `return.union.bare` | `- (union U)v;` and a typedef of one | `IMPLEMENTED` | By value, tag preserved in the prototype. The tag was dropped until #595, so GCC refused the generated C for the bare spelling while a typedef'd union worked. | `type_extraction.rs`; `nil_receiver.rs::the_guards_zero_is_valid_for_every_return_shape` (the six-shape matrix); #595 |
| `return.instancetype` | `instancetype` | `IMPLEMENTED` | Covaries with the enclosing class, including through `super`. | `regression_instancetype_covariance.rs::super_init_covaries_with_enclosing_class` |
| `method.variadic` | `- (void)log:(char *)f, ...;` | `REFUSED` | A dispatch shim declares one concrete signature per selector, so an ellipsis has nowhere to go. A variadic plain **C** function is unaffected -- `OZLog` is one. | `staticbar::check_variadic_parameter`; #538 |
| `method.duplicate-param` | two parameters with one name | `REFUSED` | Was reaching GCC. | `method_declaration_refusals.rs`; `staticbar::check_duplicate_parameter_names` |
| `selector.empty-piece` | `- (void)a:(int)x :(int)y;` | `UNEXAMINED` | Would build the selector `a::`, which `selector_to_c` mangles to `a__` with no collision check against a literal `a__`. Nothing exercises it. | `emit.rs:1264` |

## 3 — Properties and ivars

| id | Construct | Verdict | Contract | Evidence |
|---|---|---|---|---|
| `ivar.declaration` | `@interface C { int _x; }` | `IMPLEMENTED` | A member of `struct C`. Leading underscore is the convention. | every corpus case |
| `ivar.visibility` | `@public` / `@protected` / `@private` | `N/A` | Generated C is one translation unit's worth of plain structs; there is no access control to enforce. Parsed and gated on output. | `objc_node_disposition.rs` (`visibility_specification`) |
| `property.scalar` | `@property (assign) int x;` | `IMPLEMENTED` | Synthesized getter and setter over a backing ivar. | `behavior/cases/properties/getter_setter_gen.m` |
| `property.readonly` | `@property (readonly) int x;` | `IMPLEMENTED` | Getter only. Not *enforcement* -- the ivar is writable from the implementation, and a missing setter is a link error rather than a diagnostic. | `behavior/cases/properties/readonly_property.m` |
| `property.object.strong` | `@property (strong) T *x;` | `IMPLEMENTED` | The setter retains the new value and releases the old; the owner's dealloc releases the ivar. | `behavior/cases/properties/strong_vs_assign.m` (refcounts); `adapted/bucket_b/arc_property_retain.m` (the allocator side, #596) |
| `property.object.copy` | `@property (copy) T *x;` | `GAP` | **Silently identical to `strong`** -- the attribute reaches a catch-all, no `-copy` is sent, and the setter retains. An author expecting a private snapshot gets a shared reference, with no diagnostic. | #599 |
| `property.class` | `@property (class, ...) T x;` | `GAP` | **Lowered as an instance property.** The `class` attribute is discarded: an instance ivar and instance accessors are synthesized, and no class-side accessor is generated at all. | #599 |
| `property.weak` | `@property (weak) T *x;` | `REFUSED` | oz2c's own rule, **not** delegated -- Clang accepts the declaration. | `collect.rs:824`; [ARC.md](ARC.md) § 2.4 |
| `property.accessor-names` | `getter=`/`setter=` | `IMPLEMENTED` | — | `collect.rs`; `behavior/cases/properties/custom_accessors.m` |
| `property.dot-syntax` | `obj.x`, `self.x = v` | `IMPLEMENTED` | Lowers to the accessor call, including through `super`. | `behavior_dot_syntax.rs::super_property_read_calls_the_superclass_accessor`; `behavior/cases/properties/dot_syntax.m` |
| `property.synthesize` | `@synthesize x = _x;` | `IMPLEMENTED` | The remedy `@dynamic`'s refusal points at. | `behavior/cases/properties/` |
| `property.dynamic` | `@dynamic x;` | `REFUSED` | `@dynamic` promises an accessor will appear at runtime; one dispatch table fixed at build time has no moment at which that could happen. Until #574 it was commented out and the accessors synthesized anyway -- the opposite of what it asks for. | `staticbar::walk_at_keywords`; #574 |
| `property.attr-unknown` | any other attribute (`atomic`, `nullable`, `direct`, …) | `GAP` | Accepted as a silent no-op. `readwrite` and `atomic` happen to be correct because the catch-all leaves each flag at a matching default; the nullability attributes and `direct` have no lowering here. Nobody has decided which is which. | #599 |

## 4 — Protocols and qualified types

| id | Construct | Verdict | Contract | Evidence |
|---|---|---|---|---|
| `protocol.declaration` | `@protocol P` … `@end` | `IMPLEMENTED` | — | `object_protocol.rs`; `behavior/cases/protocol/` |
| `protocol.forward` | `@protocol P;` | `IMPLEMENTED` | Lowered to a comment, the analogue of `@class`. Had no arm until #582's gate found it. | `objc_node_disposition.rs`; #582 |
| `protocol.conformance` | `@interface C : S <P>` | `IMPLEMENTED` | Checked at build time; an unimplemented `@required` member is a located error. | `generics.rs`; `protocol_optional_members.rs::a_required_member_is_still_required` |
| `protocol.dispatch` | a send through `id<P>` | `IMPLEMENTED` | An `OZ_PROTOCOL_SEND_*` dispatcher switching on `_meta.class_id`, guarded against a nil receiver before the read. | `nil_receiver.rs::a_protocol_dispatcher_guards_before_switching`; `behavior/cases/protocol/` |
| `protocol.optional.instance` | `@optional` instance method | `IMPLEMENTED` | May be omitted; `-respondsToSelector:` is the whole observable behaviour. | `protocol_optional_members.rs::an_optional_member_may_be_omitted`, `protocol_optional_members.rs::responds_to_selector_still_distinguishes_the_two_conformers` |
| `protocol.optional.class` | `@optional` **class** method | `UNEXAMINED` | The conformance check skips optional members before consulting `is_class_method`, so it is plausibly correct and wholly unasserted. All four `@optional` tests use instance methods. | `emit.rs:8522` |
| `protocol.qualified-param` | `- (void)f:(id<P>)p` | `IMPLEMENTED` | Lowered. Reached the output unlowered until #367, which is why this row names a test rather than the code. | #367 |
| `protocol.class-receiver` | `Class<P> c; [c make];` | `REFUSED` | The receiver arrives as `void *`. Located, and previously absent from every user-facing list. | oz2c-challenges M106 |
| `protocol.as-value` | `@protocol(P)` as an expression | `REFUSED` | Accepted only as the argument of `-conformsToProtocol:`; a protocol has no runtime value here. | `staticbar`; `behavior/cases/reflection/` |
| `generics.parameterized` | `NSArray<T> *` style parameters | `IMPLEMENTED` | Constraint-checked, then erased. | `generics.rs` |

## 5 — Literals, subscripting, enumeration

| id | Construct | Verdict | Contract | Evidence |
|---|---|---|---|---|
| `literal.string` | `@"text"` | `IMPLEMENTED` | A statically allocated, immortal `OZString`. It is never freed and holds no slab slot. **`OZString` cannot be allocated at runtime.** | `behavior_immortal_literals.rs`; `behavior/cases/foundation/` |
| `literal.number` | `@42`, `@YES` | `IMPLEMENTED` | A boxed `OZNumber`: Q31+shift fixed point, converting to int8/16/32 and float. **No int64 and no double.** | `behavior/cases/foundation/` |
| `literal.array` | `@[a, b]` | `IMPLEMENTED` | A boxed `OZArray` built by a generated builder; the element pool is sized from allocation sites. | `companion.rs`; `behavior/cases/foundation/array_basic.m` |
| `literal.dictionary` | `@{k: v}` | `IMPLEMENTED` | As above, for `OZDictionary`. | `behavior/cases/foundation/dictionary_basic.m` |
| `literal.boxed-expr` | `@(expr)` | `IMPLEMENTED` | — | `tests/zephyr/src/` (`boxed_expr` suite) |
| `subscript.index` | `arr[0]`, `dict[key]` | `IMPLEMENTED` | — | `behavior/cases/foundation/` |
| `forin.fast-enumeration` | `for (id x in collection)` | `IMPLEMENTED` | — | `behavior/cases/forin/` |
| `forin.ownership` | a `+1` produced inside a `for-in` body | `IMPLEMENTED` | Released per iteration; `break` and `continue` release the loop local. | `behavior/cases/arc/break_releases_loop_local.m`, `behavior/cases/arc/continue_releases_loop_local.m` |
| `literal.in-brace-init` | storing a `+1` in a brace initialiser | `REFUSED` | The remedy is to initialise with nil and store through an ivar. | `staticbar`; #359 |

## 6 — Blocks, and C / Zephyr interoperation

| id | Construct | Verdict | Contract | Evidence |
|---|---|---|---|---|
| `block.non-capturing` | `^void(int x) { … }` capturing nothing | `IMPLEMENTED` | Hoisted to a named function. | `behavior/cases/blocks/` |
| `block.capturing` | a block capturing a stack local | `REFUSED` | Located. There is no block object and no `_Block_copy`. | `staticbar::check_block_capture` |
| `block.ozfn` | `OZFN(^void(struct k_timer *t) { … })` | `IMPLEMENTED` | The spelling that hands a block to a Zephyr macro. **Write the block's return type explicitly.** | `ozfn_argument_validation.rs`; `staticbar::check_ozfn_argument` |
| `block.ozfn-typecheck` | the *contents* of `OZFN`/`OZM` | `GAP` | `OZFN` expands to `0` for Clang, so the AST never contains the block and nothing checks the source inside it. A signature mismatch surfaces as GCC on generated C, pointing into a generated file. #548 closed the missing-brace half; the typecheck half is open. | #583; oz2c-challenges R2 |
| `interop.c-function` | calling a plain C function | `IMPLEMENTED` | Including variadic ones. | `src/OZLog.c` |
| `interop.zephyr-macro` | `ZBUS_CHAN_DECLARE`, `K_THREAD_DEFINE`, … | `IMPLEMENTED` | Carried through the passthrough as the author's own bytes, unexpanded. That passthrough is the product; it is also how an unnamed ObjC construct once reached GCC as `stray '@'`, which is why the output is gated now. | `objc_node_disposition.rs`; #582 |
| `bridge.plain` | `(__bridge T)x` | `IMPLEMENTED` | Transfers no ownership. | [ARC.md](ARC.md) § 1.3.4; `arc.rs:1417` |
| `bridge.retained` | `(__bridge_retained T)x` | `REFUSED` | Emitted no retain, so the local was still released at scope exit and C was handed a freed slot -- and the stale read *succeeded* first, which is why it was silent. | `staticbar::check_bridging_casts`; #460 |
| `bridge.transfer` | `(__bridge_transfer T)x` | `REFUSED` | Emitted no release, stranding the `+1` taken over from C. | `staticbar::check_bridging_casts`; #460 |
| `interop.int-cast` | casting an object to an integer type | `IMPLEMENTED` | Clang allows it; oz2c deliberately leaks rather than releasing through an integer slot. | `integer_slot_ownership.rs`; #380 |

## 7 — ARC and object lifetime

**This section delegates.** [docs/ARC.md](ARC.md) gives **one verdict per
normative rule** of Clang's ARC specification, in the spec's own section order,
with `tools/oz2c/tests/arc_conformance.rs` pinning the `DELEGATED` and
`REFUSED` ones. Restating it here would produce a second, weaker record of
something already gated -- which is the duplication this document is supposed
to prevent.

The four things an author must know, with ARC.md as the detail:

| id | Construct | Verdict | Contract | Evidence |
|---|---|---|---|---|
| `arc.always-on` | ARC | `IMPLEMENTED` | Always enabled; there is no non-ARC mode. | [ARC.md](ARC.md) |
| `arc.manual-sends` | `retain` / `release` / `autorelease` / `dealloc` / `retainCount` **sent** | `REFUSED` | A located error, and so is declaring or defining any but `dealloc`. The rule is exactly what `clang -fobjc-arc` refuses. Reading a refcount is fine, through `oz_retain_count` -- the only refcount entry point ObjC source may spell. | `staticbar::check_manual_memory_sends`; #428, #436 |
| `arc.dealloc-override` | `- (void)dealloc` override | `IMPLEMENTED` | The cleanup hook. The chain above it runs automatically, so `[super dealloc]` is redundant rather than required -- and sending it is an error. | `companion::dealloc_chain` |
| `arc.autoreleasepool` | `@autoreleasepool { … }` | `REFUSED` | With no `-autorelease` nothing can be pending, so a pool has nothing to drain. Write a plain braced scope, which is what it compiled to. | `staticbar::check_autoreleasepool`; #430 |
| `arc.weak` | `__weak` | `REFUSED` | No runtime can zero a weak reference here. Refused rather than ignored, in all ten positions. | `staticbar::check_refused_qualifiers`; `weak_every_position.rs`; #448 |
| `arc.weak-zeroing` | weak-reference zeroing *semantics* | `N/A` | By decision. `__unsafe_unretained` is the supported opt-out and the cycle-breaker; it is non-owning and is **never nilled**, so after the owner dies it dangles. | [ARC.md](ARC.md) § 2.2 |
| `arc.unsafe-unretained` | `__unsafe_unretained` | `IMPLEMENTED` | Honoured at every site. The way to break a retain cycle. | `adapted/apple_spec/retain_cycle_break.m`; [ARC.md](ARC.md) § 2.2 |
| `arc.autoreleasing` | `__autoreleasing` | `REFUSED` | Delete the qualifier; the declaration then carries the default `__strong`, which with no pool is the only release timing available. | #425 |
| `arc.ownership-attrs` | `ns_consumed`, `ns_returns_retained`, … | `REFUSED` | They move a release across a call boundary, which oz2c does not model. One reached a use-after-free. | `staticbar::check_ownership_attributes`; #458 |

**Slabs, not a heap.** Each class has a fixed-size slab sized from its
allocation sites. An allocation that finds no free slot answers `nil` -- so
`[C alloc]` returning nil is normal and must be handled. `/* oz-pool: C=N */`
overrides a size in a test; `--pool-sizes` does it on the command line.

## 8 — Reflection and selectors

| id | Construct | Verdict | Contract | Evidence |
|---|---|---|---|---|
| `selector.literal` | `@selector(name:)` | `IMPLEMENTED` | Requires `--reflection`. | `behavior/cases/reflection/selector_send.m` |
| `selector.undeclared` | `@selector` of a method no class declares | `REFUSED` | Refused even when used only with `-respondsToSelector:`, where a "no" answer would be the correct result. | #575; oz2c-challenges R13 |
| `reflection.perform` | `-performSelector:` and its two `withObject:` forms | `IMPLEMENTED` | The wrapper has one fixed shape, so a selector whose return type is neither void nor an object cannot go through it -- a struct or an `int` return is a located refusal. | `emit.rs:2906` |
| `reflection.kind` | `-isKindOfClass:`, `-conformsToProtocol:`, `-respondsToSelector:` | `IMPLEMENTED` | Answered at compile time from the whole-program class set, and therefore **not overridable** -- an override's body would never run, and that is a located error. Nil-safe. | `behavior/cases/reflection/kind_and_conformance.m` |
| `reflection.class-identity` | `[obj class]`, `+class` | `IMPLEMENTED` | Requires `--introspection`. | `behavior/cases/reflection/class_identity.m` |
| `reflection.sel-storage` | storing a `SEL` | `IMPLEMENTED` | **Costly:** storing any `SEL` makes `-performSelector:`-ability retype unrelated methods across the program. | #541 |
| `reflection.encode` | `@encode(T)` | `REFUSED` | A compile-time operator yielding a type-encoding string only a runtime that reads those strings can use. Use `sizeof`, or write the string. | `staticbar::walk_at_keywords`; #563 |
| `reflection.dynamic-class` | creating a class at runtime, method swizzling, associated objects, `forwardInvocation:` | `N/A` | One dispatch table fixed at build time. | — |

## 9 — Preprocessor

| id | Construct | Verdict | Contract | Evidence |
|---|---|---|---|---|
| `preproc.import` | `#import` | `IMPLEMENTED` | Resolved by oz2c with per-origin provenance. | `imports.rs` |
| `preproc.at-import` | `@import Foundation;` | `REFUSED` | oz2c's, not Clang's: a source declaring no class never gets an AST dump, and `--allow-missing-ast` skips it. Found by #582's gate on its first run. | `staticbar::walk_at_keywords`; #582 |
| `preproc.objc-in-macro-body` | Objective-C inside a `#define` body | `REFUSED` | A macro body is one opaque token to the parser, so this is a located error rather than C that will not compile. Objective-C in a macro **argument** is fine and the invocation is preserved. | `staticbar::check_macro_body`; #238 |
| `preproc.conditional-objc` | Objective-C under `#if` / `#ifdef` | `IMPLEMENTED` | Resolved at transpile time -- a class becomes a struct, a dispatch row and a slab, and there is no way to hand GCC half a dispatch table. A conditional carrying only C passes through untouched. | `preproc.rs`; #570, #573 |
| `preproc.kconfig-decides` | a Kconfig or `-D` macro deciding a conditional around ObjC | `GAP` | oz2c parses the raw file, not the preprocessed translation unit, so a command-line macro cannot select the arm. | #586 |
| `preproc.macro-shadow` | a `#define` colliding with a generated name | `REFUSED` | A source `#define` is copied into the generated C and would rewrite what oz2c emitted. | `macro_shadowing.rs`; `staticbar::check_macro_shadows_emitted_name` |
| `preproc.header-provenance` | a shared header declaring no class | `GAP` | Its content reaches no other origin. | #594 |

## 10 — Explicitly unsupported

Each of these is a **located error**, not a silent omission. That is the
standing rule: oz2c never silently degrades, and since #582 the *output* is
gated too -- every ObjC node kind is lowered or refused, never merely unnamed,
because the alternative kept arriving as `stray '@' in program` in a generated
file with oz2c exiting 0 (#563, #573, #574).

| id | Construct | Verdict | Evidence |
|---|---|---|---|
| `exc.try` | `@try` / `@catch` / `@finally` | `REFUSED` | exception handling needs runtime unwinding info this backend does not generate |
| `exc.throw` | `@throw` | `REFUSED` | `staticbar::walk_at_keywords`; #563 |
| `at.available` | `@available(…)` | `REFUSED` | a single-target static build has no version to test |
| `at.defs` | `@defs(C)`, in both grammar spellings | `REFUSED` | `staticbar::is_defs_shape`, `atdef_field`; #582 |
| `at.unrecognised` | any other `@`-keyword | `REFUSED` | `staticbar::check_at_keywords` and `outputbar`, so an unnamed keyword cannot reach GCC |
| `sync.synchronized` | `@synchronized(obj) { … }` | `IMPLEMENTED` | the one entry in this section that is supported; the body is restricted, and `smp_shared` is the only place it faces real contention |
| `id.reserved` | `id` as an identifier | `REFUSED` | `id` is a **reserved word** (#317); renames are permanent |

---

## Keeping this file honest

`tools/oz2c/tests/dialect_ledger.rs` is the gate, modelled on
`arc_conformance.rs`. It asserts:

1. the verdict vocabulary is closed and matches ARC.md's;
2. every `GAP` row cites an issue number -- a `GAP` with no issue is the state
   this file is meant to make impossible;
3. every in-repo evidence path exists on disk;
4. every `tests/adapted/apple_spec/*.m` has exactly one `_test.c` and a row here;
5. the row count is pinned;
6. **every ObjC-only grammar kind has a row or a named exemption**, keyed to
   `objc_node_disposition.rs`'s classification, so a `tree-sitter-objc` bump
   cannot add a construct nobody documented.

**Check 6 is keyed to the ObjC-only kinds, not to all 191.** The grammar's 191
named kinds are mostly plain C and preprocessor spellings -- `C_AND_PREPROCESSOR`
alone is ~130 of them -- which ride the passthrough on purpose and are not this
document's subject. The anchor is `GATED` (41 kinds) plus
`OBJC_ONLY_VIA_PARENT` (5, all under `@available`) plus the author-facing
members of `CLANG_EXTENSION_OR_SHARED`. Keying to 191 would demand a row for
`abstract_array_declarator`, which would be noise standing in for rigour.

**Boundary evidence is in-repo wherever possible.** A handful of rows cite
`oz2c-challenges` R/M ids, because that is the only place those boundaries are
recorded. Those citations are **not gated** -- that repo grades against a
moving oz2c and keeps its own drift section -- so the standing preference is to
**port a challenge case into `tools/oz2c/tests/`** as a refusal test and cite
that instead. An imported case is gated like everything else; a cross-repo id
is a promise nobody can check.
