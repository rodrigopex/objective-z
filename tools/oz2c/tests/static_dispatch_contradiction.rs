// SPDX-License-Identifier: Apache-2.0
//
// static_dispatch_contradiction.rs -- when the emitter resolves a
// receiver's class and `arc` does not, the ownership answer came from a
// poll over classes the send can never reach. That is a **contradiction**,
// not an ambiguity, and it is refused with a located error (#483).
//
// #481 and #502 were two instances of one shape, each closed by teaching
// `arc` one more binding form: a method parameter, then a for-in loop
// variable. #483 is the argument that the *class* is unbounded -- the
// design requires two enumerations to track each other and nothing checks
// that they do -- and Rodrigo chose option 4 from it: keep two resolvers,
// and refuse rather than guess.
//
// **Three instances were live when this landed**, all measured on
// `main` at 03db303 (i.e. with #502's fix in), each reading `deallocs=0`
// against an expected 1:
//
//   * an **ivar** receiver          -- `[_owner build]`
//   * a **file-scope object** receiver -- `[g_owner build]`
//   * a **cast** receiver           -- `[(Owner *)raw build]`
//
// **None of the three is a node kind**, which is why the cheapest option
// #483 lists -- a gate enumerating the kinds in
// `arc::collect_declared_types`'s match -- was measured incapable. An ivar
// is declared in the `@interface` and a file-scope variable at top level,
// both *outside* the `method_definition`/`function_definition` scope
// `arc::declared_class_of`'s walk starts from; and a cast receiver is not
// an `identifier` at all, so `message_target` never asks for a
// declaration. The emitter resolves all three anyway -- ivars and
// file-scope vars are seeded into `ctx.scope`, and a cast carries its own
// type -- and emitted a direct call while carrying a polled answer.
//
// **Deliberately not built on the for-in shape.** That is the trap this
// file exists to avoid: #503 teaches `arc` the for-in binding, so once it
// landed that shape stopped being a contradiction and a guard tested on it
// would have passed while asserting nothing. It appears below only as a
// *control* that it is still accepted, where a silent stop means a
// failure rather than a pass.
//
// The controls matter as much as the refusals: a refusal that fires
// unconditionally is decoration. `an_ivar_receiver_with_one_implementor_*`
// is the load-bearing one -- the identical ivar shape, one implementor, no
// disagreement -- and it must still compile and still release.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// `Thing` counts its own deallocations, which is the oracle for every
/// accepting case here. A grep for `oz_release` in the output is not: this
/// repo has five recorded cases of a text-absence claim holding while the
/// property was gone.
const THING: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag {
	return 7;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end
";

/// `Owner` hands back a `+1`; `Lender` hands back what it keeps. Two
/// classes, one non-create-rule selector, **disagreeing** -- which is what
/// makes the implementor poll ambiguous.
///
/// Neither is a subclass of the other, so `has_overriding_subclass` is
/// false and the emitter still emits a *direct* call: that is the whole
/// reason the existing `Ambiguous` refusal, which guards the `class_id`
/// switch, never saw these sends.
const DISAGREEING: &str = "\
@interface Owner : OZObject
- (Thing *)build;
@end
@implementation Owner
- (Thing *)build {
	return [[Thing alloc] init];
}
@end

@interface Lender : OZObject
{
	Thing *_held;
}
- (Thing *)build;
@end
@implementation Lender
- (Thing *)build {
	return _held;
}
@end
";

/// `-copy` on `Thing`, as a category so `THING` stays the shared fixture
/// every other case here reads.
///
/// It exists for exactly one case -- the nested shape in
/// `both_refusals_fire_on_one_nested_store`, whose outer send has to be a
/// create-rule selector -- and **it is not optional decoration.**
/// `is_create_rule_selector` answers from the *spelling*, so
/// `[[_owner build] copy]` is `+1` whether or not `-copy` is declared; but
/// the emitter separately refuses a send to a selector no class in the
/// program implements, and without this category that third diagnostic
/// rides along. Measured: three diagnostics without it, two with
/// (`class 'Thing' has no method matching 'copy'` is the extra). #511's
/// own description of the shape omits it, which is how a "both fire"
/// reading of a three-diagnostic result happens.
const COPYABLE: &str = "\
@interface Thing (Copying)
- (Thing *)copy;
@end
@implementation Thing (Copying)
- (Thing *)copy {
	return [[Thing alloc] init];
}
@end
";

/// Only `Owner` declares `-build`, so the poll is unanimous and the
/// receiver's class is never needed.
const AGREEING: &str = "\
@interface Owner : OZObject
- (Thing *)build;
@end
@implementation Owner
- (Thing *)build {
	return [[Thing alloc] init];
}
@end
";

fn program(classes: &str, tail: &str) -> String {
    format!(
        "/* oz-pool: Thing=4,Owner=2,Lender=2,P=1 */\n{}{}{}{}",
        PREAMBLE(),
        THING,
        classes,
        tail
    )
}

/// #507's store refusal, by the **shortest unique** fragment of its text
/// (`emit.rs:3777`). One constant so the absence and presence halves of
/// `an_ambiguous_send_into_an_array_element_is_this_refusal_not_507s`
/// cannot drift apart -- two literals would let the presence half keep
/// passing while the absence half went vacuous, which is the whole failure
/// this pairing exists to prevent.
const STORE_REFUSAL: &str = "storing a '+1' into an element of";

/// This file's own refusal (#483), by a fragment that identifies **only**
/// it (`emit.rs:4989`).
///
/// Measured the same way `STORE_REFUSAL` was, and for the same reason --
/// a needle that matches more than one refusal turns an identity
/// assertion back into a count:
///
/// | needle | `git grep -c -F <needle> -- 'tools/oz2c/src/*.rs'` |
/// |---|---|
/// | `is not supported` | **11** (collect.rs 2, emit.rs 3, staticbar.rs 6) |
/// | `storing a '+1' into` | **2** (emit.rs: #477's and #495's) |
/// | `storing a '+1' into an element of` | **1** (emit.rs:3777) |
/// | `dispatched directly here` | **1** (emit.rs:4989) |
///
/// The full sentence would be more obviously #483's to a reader, but it
/// is wrapped across two source lines by a `\` continuation, so no
/// line-oriented grep can measure its cardinality -- and an unmeasured
/// needle is the thing this table exists to refuse.
const CONTRADICTION_REFUSAL: &str = "dispatched directly here";

/// Transpile, expecting refusal, and hand back the diagnostics
/// **unjoined**.
///
/// `common::expect_reject` concatenates them into one string, which is
/// enough to ask "does this text appear" and not enough to ask "how
/// many diagnostics, and which one is at which column" -- the two
/// questions `both_refusals_fire_on_one_nested_store` is made of.
fn reject_diagnostics(source: &str) -> Vec<oz2c::Diagnostic> {
    match oz2c::transpile(source) {
        Ok(_) => panic!("expected transpile to be rejected, but it succeeded"),
        Err(diags) => diags,
    }
}

/// One line per diagnostic, with its position -- the failure context for
/// every assertion below.
fn render(diags: &[oz2c::Diagnostic]) -> String {
    diags
        .iter()
        .enumerate()
        .map(|(i, d)| format!("  [{}] {}:{} {}", i, d.line, d.col, d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Both refusals, each identified by its own cardinality-1 needle, each
/// at its own column, and **nothing else**.
///
/// The order is deliberate: the two identity assertions come before the
/// count, so removing either arm fails saying *which refusal is missing*
/// rather than "expected 2, got 1". A test that reports only the number
/// sends the next reader to count diagnostics instead of to the arm that
/// stopped firing.
///
/// And the count alone would not be enough even with both identities
/// right: `_a[0] = [[_owner build] copy];` into an *ivar* array with
/// `-copy` undeclared also yields exactly two diagnostics -- #483's plus
/// the method-existence refusal -- so a `len() == 2` assertion passes on
/// a program where #477's refusal never ran. That is measured, and it is
/// why the identities are asserted rather than the total.
fn assert_both_refusals(diags: &[oz2c::Diagnostic]) {
    let ctx = render(diags);
    let store: Vec<&oz2c::Diagnostic> =
        diags.iter().filter(|d| d.message.contains(STORE_REFUSAL)).collect();
    let contra: Vec<&oz2c::Diagnostic> =
        diags.iter().filter(|d| d.message.contains(CONTRADICTION_REFUSAL)).collect();
    assert_eq!(
        store.len(),
        1,
        "#477's store refusal is missing (or duplicated): expected exactly one \
         diagnostic containing {:?}, found {}. If the count below is 1, that arm \
         stopped firing -- it is not a wording change, or the needle's own \
         presence check in `..._is_this_refusal_not_507s` would be red too.\n{}",
        STORE_REFUSAL,
        store.len(),
        ctx
    );
    assert_eq!(
        contra.len(),
        1,
        "#483's contradiction refusal is missing (or duplicated): expected exactly \
         one diagnostic containing {:?}, found {}. If the count below is 1, this \
         file's own refusal stopped firing on a nested receiver -- every other test \
         here sends to an *unnested* one, so this is the only place that would \
         notice.\n{}",
        CONTRADICTION_REFUSAL,
        contra.len(),
        ctx
    );
    assert_eq!(
        diags.len(),
        2,
        "exactly two diagnostics, so no third refusal rides along unremarked -- \
         drop `COPYABLE` and `class 'Thing' has no method matching 'copy'` makes it \
         three:\n{}",
        ctx
    );
    /* The columns are the point: one program, one statement, two nodes.
     * The store's span starts at the statement, the send's inside it, so
     * the outer column is strictly the smaller -- which is what "their
     * own columns" means, and what a single diagnostic covering both
     * would not have. */
    assert!(
        store[0].line == contra[0].line && store[0].col < contra[0].col,
        "the two refusals must land on the same statement at their own columns, \
         the outer store's before the nested send's:\n{}",
        ctx
    );
}

/// Every refusal has to name the selector, both sides of the
/// disagreement, and why a poll ran at all.
fn assert_refusal(diags: &str, selector: &str) {
    for needle in [
        selector,
        "dispatched directly here",
        "'Owner' hands back a reference the caller must release",
        "'Lender' hands back one it keeps owning",
        "resolved for dispatch but not for ownership",
    ] {
        assert!(
            diags.contains(needle),
            "refusal should mention {:?}, got:\n{}",
            needle,
            diags
        );
    }
}

// ---------------------------------------------------------------------------
// The three live contradictions, now refused
// ---------------------------------------------------------------------------

/// An **ivar** receiver. `render_method_definition` does
/// `ctx.scope = ivars_scope.clone()`, so the emitter resolves `_owner` and
/// emits `Owner_build(...)`; `arc::declared_class_of` walks up to the
/// enclosing `method_definition` and scans *inside* it, where no
/// declaration of `_owner` exists -- the `@interface` is outside that
/// subtree entirely. Measured at `deallocs=0` before this refusal.
#[test]
fn an_ivar_receiver_is_refused_rather_than_leaked() {
    let src = program(
        DISAGREEING,
        "\
@interface P : OZObject
{
	Owner *_owner;
}
- (void)setup;
- (int)viaIvar;
@end
@implementation P
- (void)setup {
	_owner = [Owner alloc];
}
- (int)viaIvar {
	Thing *t = [_owner build];
	return [t tag];
}
@end

int main(void) {
	return 0;
}
",
    );
    assert_refusal(&expect_reject(&src), "build");
}

/// A **file-scope object** receiver. `emit::file_scope_vars` puts
/// `g_owner` into `ctx.scope` -- that is how a send to one resolves at all
/// -- and a top-level declaration is again outside the method scope
/// `arc::declared_class_of` searches. Measured at `deallocs=0`.
#[test]
fn a_file_scope_object_receiver_is_refused() {
    let src = program(
        DISAGREEING,
        "\
static Owner *g_owner;

@interface P : OZObject
- (int)viaFileScope;
@end
@implementation P
- (int)viaFileScope {
	Thing *t = [g_owner build];
	return [t tag];
}
@end

int main(void) {
	return 0;
}
",
    );
    assert_refusal(&expect_reject(&src), "build");
}

/// A **cast** receiver -- the px-keyboard shape, an object arriving as a
/// `void *` out of a `k_timer` slot and cast back at the send. The cast
/// carries the type, so the emitter resolves it; `message_target` only
/// asks for a declaration when the receiver is an `identifier`, and a
/// `cast_expression` is not one. Measured at `deallocs=0`.
#[test]
fn a_cast_receiver_is_refused() {
    let src = program(
        DISAGREEING,
        "\
@interface P : OZObject
- (int)viaCast:(void *)raw;
@end
@implementation P
- (int)viaCast:(void *)raw {
	Thing *t = [(Owner *)raw build];
	return [t tag];
}
@end

int main(void) {
	return 0;
}
",
    );
    assert_refusal(&expect_reject(&src), "build");
}

/// A **`super`** receiver -- a fourth instance, found by reading
/// `is_super_receiver` and then measured. Pinned deliberately, and it is
/// the one case here where the refusal is **stricter than it needs to
/// be**.
///
/// `super` is an `identifier` node, so `message_target` calls
/// `declared_class_of("super", ..)`, which finds no declaration of that
/// name and answers `None`. The emitter meanwhile keeps the send a direct
/// call by definition -- routing it through the `class_id` switch would
/// re-enter the override that issued it -- so the asymmetry holds and the
/// send was leaking before this.
///
/// **Unlike the three above, a correct answer is trivially available
/// here**: `super` is exactly the superclass of the enclosing
/// `@implementation`, with no cast and no collection element type to lie
/// about, so resolving it in `message_target` (via `enclosing_impl_class`
/// plus `ClassInfo::superclass`) would be sound in a way #502 measured
/// that the for-in binding is not. That is a widening of `arc`'s
/// resolution, which is precisely the direction #483 decided *not* to take
/// as the fix, so it is recorded here rather than done -- and note the
/// first `help` line the diagnostic offers does not apply to a `super`
/// send, only the second one does.
///
/// Nothing in the tree is affected: all 59 `[super ...]` sends in
/// `samples/`, `src/` and `tests/` are `init` (39), `dealloc` (12) or
/// `initWithDTSpec:` (5), and `is_initialiser` answers `Borrowed` for an
/// initialiser before the poll is ever reached.
///
/// **If a later change resolves a `super` receiver, this test fails.**
/// Delete it then, and say so -- do not relax the assertion.
#[test]
fn a_super_receiver_is_refused_although_it_is_resolvable() {
    let src = format!(
        "/* oz-pool: Thing=4,Base=2,Sub=2,Lender=2 */\n{}{}{}",
        PREAMBLE(),
        THING,
        "\
@interface Base : OZObject
- (Thing *)build;
@end
@implementation Base
- (Thing *)build {
	return [[Thing alloc] init];
}
@end

@interface Lender : OZObject
{
	Thing *_held;
}
- (Thing *)build;
@end
@implementation Lender
- (Thing *)build {
	return _held;
}
@end

@interface Sub : Base
- (int)viaSuper;
@end
@implementation Sub
- (int)viaSuper {
	Thing *t = [super build];
	return [t tag];
}
@end

int main(void) {
	return 0;
}
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("is dispatched directly here")
            && diags.contains("'Base' hands back a reference the caller must release"),
        "a `super` receiver is unresolved in `arc`, so the poll runs and disagrees:\n{}",
        diags
    );
}

/// The shape where this refusal and **#507's** could collide, pinned so
/// neither lane can shadow the other unnoticed.
///
/// #507 refuses a `+1` stored into an array element that is not an ivar.
/// Its precondition is `arc::binds_ownership(right, ..)` being **true**;
/// this refusal fires only when the poll came back `Ambiguous`, which
/// `is_owning_expr` reads as *borrowed* — the leaking direction, per the
/// standing rule. So on **one** expression the two preconditions are
/// disjoint by construction: the definiteness #507 needs is exactly what
/// this refusal exists because it was missing.
///
/// Measured rather than argued, by cherry-picking #507's two code commits
/// onto this branch and transpiling this program: **only this refusal is
/// emitted**, and the array-element context suppresses nothing. The
/// converse is also measured — `a[0] = [[Thing alloc] init];` gives only
/// #507's — and so is the one case where **both** fire, which the
/// same-node argument does not cover and which is worth stating plainly:
/// `a[0] = [[_owner build] copy];` nests them (this refusal on the inner
/// ambiguous send, #507's on the outer store, whose `copy` is a
/// create-rule selector) and produces **both** diagnostics at their own
/// columns, because `ctx.err` accumulates and `front_end` returns every
/// diagnostic rather than stopping at the first.
///
/// So "they cannot both fire" is true per node and false across nested
/// nodes. Either way neither is masked, and neither refusal is made
/// untestable by the other.
///
/// **Two programs, because half of this is an absence assertion.** An
/// absence check passes for two different reasons — the thing really did
/// not happen, or the needle no longer matches anything — and this repo
/// has a green-guard-asserting-nothing incident from exactly that. So the
/// second program is the **presence** half: a store that *is* an
/// unambiguous `+1`, which must produce #507's diagnostic. If the needle
/// ever stops matching #507's text, the presence half fails and names
/// that, instead of the absence half silently going vacuous.
///
/// **The needle is `storing a '+1' into an element of`, and its
/// cardinality is the reason.** Measured on the merged tree:
/// `is not supported` matches **11** places across `collect.rs`,
/// `emit.rs` and `staticbar.rs`, so asserting its absence under a message
/// naming #507 would fail for ten other reasons while blaming #507 — a
/// test that fails for a reason other than the one it states costs the
/// next reader more than one that fails silently. The shorter prefix
/// `storing a '+1' into` matches **2**: the other is #495's parameter
/// destination at `emit.rs:3433`, a *sibling* refusal differing by three
/// words in the middle, which is the version a reviewer's eye skips. Only
/// the full phrase is unique (`emit.rs:3777`). Narrowing halfway would
/// have felt like the fix.
#[test]
fn an_ambiguous_send_into_an_array_element_is_this_refusal_not_507s() {
    let src = program(
        DISAGREEING,
        "\
@interface P : OZObject
{
	Owner *_owner;
}
- (void)setup;
- (void)run;
@end
@implementation P
- (void)setup {
	_owner = [Owner alloc];
}
- (void)run {
	id a[2];
	a[0] = [_owner build];
}
@end

int main(void) {
	return 0;
}
",
    );
    let diags = expect_reject(&src);
    assert_refusal(&diags, "build");
    assert!(
        !diags.contains(STORE_REFUSAL),
        "#507's store refusal must not fire here -- its precondition is \
         `binds_ownership(right)`, and an ambiguous poll reads as borrowed:\n{}",
        diags
    );

    /* The presence half. Same store, same non-ivar array, but the value
     * is an unambiguous `+1` -- so `binds_ownership` is true, #507's
     * precondition holds, and its diagnostic must appear. This is what
     * keeps the absence assertion above honest: it fails if the needle
     * stops matching #507's text, rather than letting the absence check
     * pass because it matches nothing. */
    let unambiguous = program(
        DISAGREEING,
        "\
@interface P : OZObject
- (void)run;
@end
@implementation P
- (void)run {
	id a[2];
	a[0] = [[Thing alloc] init];
}
@end

int main(void) {
	return 0;
}
",
    );
    let store_diags = expect_reject(&unambiguous);
    assert!(
        store_diags.contains(STORE_REFUSAL),
        "the needle {:?} must still match #507's own diagnostic -- if it does not, the \
         absence assertion above is vacuous rather than passing:\n{}",
        STORE_REFUSAL,
        store_diags
    );
    /* And this program is #507's alone: no ambiguous send in it. */
    assert!(
        !store_diags.contains("dispatched directly here"),
        "an unambiguous `+1` gives no contradiction, so this refusal must stay quiet:\n{}",
        store_diags
    );
}

/// **Row 3: the one shape where both refusals fire** (#511 item 1).
///
/// The sibling test above pins that neither refusal *masks* the other on
/// a single node, where their preconditions are disjoint by
/// construction. This pins the complement, and it is the claim that was
/// unassertable until both refusals were on one tree (#507 and #508
/// landed separately, each branch carrying only its own):
///
/// ```objc
/// id a[2];
/// a[0] = [[_owner build] copy];
/// ```
///
/// Two nodes, one statement. The **inner** send's implementor poll is
/// ambiguous, so this file's refusal fires on it; the **outer** store is
/// a `+1` into a non-ivar array element, because `copy` is a create-rule
/// selector, so #477's fires on that. So the general statement is
/// **disjoint per node, co-existing across nested nodes** -- not "they
/// cannot both fire".
///
/// **Nothing short-circuits, and that is `main`'s property rather than
/// either PR's.** Verified on this tree rather than taken on trust,
/// because it is the whole premise:
///
/// * `EmitCtx::err` (`emit.rs:1478`) and `err_detailed` (`emit.rs:1496`)
///   both end in `self.diags.push(..)` (`emit.rs:1479`, `emit.rs:1511`).
///   Append only -- no replace, no first-wins guard, and `push` is the
///   sole way either writes.
/// * `lib.rs` gates **after** the pass returns:
///   `let result = emit::emit(..); if !result.diagnostics.is_empty() {
///   return Err(result.diagnostics) }` (`lib.rs:277-286`). The emit pass
///   has already finished walking by the time anything looks at the
///   vector.
/// * `emit.rs` contains **no** early return on a non-empty accumulated
///   vector: its only `is_empty` tests are on the locally-scoped
///   `reject_diags` from `staticbar::check_method_body` /
///   `check_function_body` (`emit.rs:7997`, `emit.rs:8763`), and each
///   extends `ctx.diags` and falls through to keep the body as raw text.
///   Neither returns, and neither reads `ctx.diags`.
///
/// If any of those three changes, this test is where it shows up -- and
/// it shows up as one refusal named missing, not as a count.
#[test]
fn both_refusals_fire_on_one_nested_store() {
    let src = program(
        &format!("{}{}", COPYABLE, DISAGREEING),
        "\
@interface P : OZObject
{
	Owner *_owner;
}
- (void)setup;
- (void)run;
@end
@implementation P
- (void)setup {
	_owner = [Owner alloc];
}
- (void)run {
	id a[2];
	a[0] = [[_owner build] copy];
}
@end

int main(void) {
	return 0;
}
",
    );
    let diags = reject_diagnostics(&src);
    assert_both_refusals(&diags);
    /* And #483's refusal is the *whole* refusal, not just its first
     * clause: the disagreement it names has to be the real one, or this
     * would pass on a refusal that merely reused the phrase. Rendered
     * through `Display` here, not through `render` above, because the
     * `note` tier this checks is only in the formatted form. */
    let formatted =
        diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
    assert_refusal(&formatted, "build");
}

// ---------------------------------------------------------------------------
// Controls: what the refusal must NOT reach
// ---------------------------------------------------------------------------

/// **The load-bearing control.** The identical ivar receiver, with only
/// one class declaring `-build`: the poll is unanimous, `arc` needs no
/// class, and the send must still compile and still release.
///
/// Without this the refusal could be keyed on the *shape* -- "an ivar
/// receiver is refused" -- which would reject ordinary code. It is keyed
/// on the disagreement.
#[test]
fn an_ivar_receiver_with_one_implementor_is_not_refused() {
    let src = program(
        AGREEING,
        "\
@interface P : OZObject
{
	Owner *_owner;
}
- (void)setup;
- (int)viaIvar;
@end
@implementation P
- (void)setup {
	_owner = [Owner alloc];
}
- (int)viaIvar {
	Thing *t = [_owner build];
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	[p setup];
	int v = [p viaIvar];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_ivar_one_impl"),
        "v=7 deallocs=1\n",
        "one implementor makes the poll unanimous, so the ivar shape alone must not refuse"
    );
}

/// The second control on the same axis: an ivar receiver, both disagreeing
/// implementors present in the program, but the *selector* returns
/// `void`. No implementation of `-poke` is in `OwningMethods` at all
/// (`returns_object_pointer` keeps it out), so the poll answers `Borrowed`
/// rather than `Ambiguous` and ownership is meaningless anyway.
///
/// Refusing here would reject exactly the polymorphism the language is
/// for.
#[test]
fn a_void_selector_on_an_ivar_receiver_is_not_refused() {
    let src = format!(
        "/* oz-pool: Thing=4,Owner=2,Lender=2,P=1 */\n{}{}{}{}",
        PREAMBLE(),
        THING,
        DISAGREEING,
        "\
static int g_pokes = 0;

@interface Owner (Poking)
- (void)poke;
@end
@implementation Owner (Poking)
- (void)poke {
	g_pokes = g_pokes + 1;
}
@end

@interface Lender (Poking)
- (void)poke;
@end
@implementation Lender (Poking)
- (void)poke {
	g_pokes = g_pokes + 100;
}
@end

@interface P : OZObject
{
	Owner *_owner;
}
- (void)setup;
- (void)run;
@end
@implementation P
- (void)setup {
	_owner = [Owner alloc];
}
- (void)run {
	[_owner poke];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	[p setup];
	[p run];
	printf(\"pokes=%d deallocs=%d\\n\", g_pokes, g_deallocs);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_void_selector"),
        "pokes=1 deallocs=0\n",
        "a void selector carries no ownership, so a disagreement cannot exist to refuse"
    );
}

/// A **local** receiver, which `arc` has always resolved. The control that
/// says this is about the resolution asymmetry rather than about the
/// disagreement.
#[test]
fn a_local_receiver_still_compiles_and_releases() {
    let src = program(
        DISAGREEING,
        "\
@interface P : OZObject
- (int)viaLocal;
@end
@implementation P
- (int)viaLocal {
	Owner *o = [Owner alloc];
	Thing *t = [o build];
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int v = [p viaLocal];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_local"),
        "v=7 deallocs=1\n",
        "a local receiver resolves in both, so the refusal must not reach it"
    );
}

/// A **method parameter** receiver -- #481's shape. `arc` has known
/// `method_parameter` since then, so this resolves in both and must stay
/// accepted.
///
/// A regression guard pointing backwards: if #481's one-token fix were
/// ever reverted, this control turns red rather than the whole shape
/// quietly becoming a build error.
#[test]
fn a_method_parameter_receiver_still_compiles_and_releases() {
    let src = program(
        DISAGREEING,
        "\
@interface P : OZObject
- (int)viaParameter:(Owner *)o;
@end
@implementation P
- (int)viaParameter:(Owner *)o {
	Thing *t = [o build];
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	Owner *o = [Owner alloc];
	P *p = [P alloc];
	int v = [p viaParameter:o];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_parameter"),
        "v=7 deallocs=1\n",
        "#481 taught `arc` `method_parameter`; this shape must not become a refusal"
    );
}

/// A **`self`** receiver, which `message_target` resolves through
/// `enclosing_impl_class`. Included because `self` is the most common
/// receiver in the corpus and a refusal reaching it would reject
/// essentially every program.
#[test]
fn a_self_receiver_still_compiles_and_releases() {
    let src = program(
        DISAGREEING,
        "\
@interface Owner (Twice)
- (int)twice;
@end
@implementation Owner (Twice)
- (int)twice {
	Thing *t = [self build];
	return [t tag] + [t tag];
}
@end

#include <stdio.h>
int main(void) {
	Owner *o = [Owner alloc];
	int v = [o twice];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_self"),
        "v=14 deallocs=1\n",
        "a `self` receiver resolves through `enclosing_impl_class` and must stay accepted"
    );
}
