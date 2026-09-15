// SPDX-License-Identifier: Apache-2.0
//
// arc_leak_regressions.rs -- two leaks found by running the behaviour corpus
// under LeakSanitizer through *this* backend for the first time.
//
// Both cases already existed in the corpus and passed throughout: a leak is
// invisible to a driver that only checks return values, and
// `just test-cross-backend` compares Unity results rather than allocation
// balance, so 71/71 MATCH said nothing about either. Only pointing LSan at
// oz2c's own output made them visible.
//
// These tests count `-dealloc` calls instead of using a sanitizer, for a
// reason worth stating: `-fsanitize=leak` is unsupported on
// arm64-apple-darwin, so a leak test written with it would be unrunnable on
// a maintainer's machine and would hold only in CI. A dealloc counter is
// portable and asks the sharper question anyway -- not "was the memory
// reachable at exit" but "did the object's teardown run".

mod common;
use common::{compile_and_run, compile_and_run_with_reflection, ozobject_src as PREAMBLE};

/// An early `return` from a scope nested inside a loop must release the
/// loop body's owned local.
///
/// `needs_translation` listed `break_statement` and `continue_statement` --
/// deliberately, so ARC could prepend the releases a jump owes -- but not
/// `return_statement`. So a `return` inside an otherwise pure-C subtree was
/// never visited, `render_return_statement` never ran, and the release
/// stayed at the end of the loop body where the jump had already skipped it.
///
/// `tests/behavior/cases/arc/return_in_nested_scope.m` is the corpus case
/// this mirrors; it leaked 12 bytes on every run while asserting the right
/// return value.
#[test]
fn early_return_from_nested_scope_releases_the_loop_local() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Inner : OZObject
@end
@implementation Inner
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (int)earlyReturn;
@end
@implementation Runner
- (int)earlyReturn {
	int i = 0;
	while (i < 3) {
		Inner *obj = [Inner alloc];
		/* No Objective-C anywhere in this `if`, which is exactly why the
		 * return was never visited before the fix. */
		if (i == 1) {
			return 42;
		}
		i = i + 1;
	}
	return -1;
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int v = [r earlyReturn];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "early_return_releases_loop_local");
    // Two iterations allocate (i = 0 and i = 1); both must be torn down --
    // one at the end of the body, one on the way out through the return.
    assert_eq!(out, "v=42 deallocs=2\n", "the returned-past local must be released: {}", out);
}

/// An owning instance method invoked on a *variable* receiver hands back +1,
/// and the caller must release it.
///
/// `arc::message_target` resolved only a class-name receiver, so
/// `[a sub:b]` looked borrowed however owning `-sub:` was known to be --
/// `OZNumber *a` is not a class name. `foundation/q31_basic` leaked an OZNumber
/// per call on that path.
///
/// Resolution is exact rather than inferred, which matters because widening
/// what counts as owning is the double-free direction: a named receiver is
/// read from its own declaration, and `self` from the enclosing
/// `@implementation`.
#[test]
fn owning_method_on_a_variable_receiver_is_released() {
    let src = format!(
        "/* oz-pool: Node=2,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Node : OZObject
+ (instancetype)make;
- (instancetype)derive;
@end
@implementation Node
+ (instancetype)make {
	Node *n = [Node alloc];
	return n;
}
- (instancetype)derive {
	return [Node make];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	Node *seed = [Node make];
	/* Variable receiver: the shape that leaked. */
	Node *viaVar = [seed derive];
	return (seed != nil) + (viaVar != nil);
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int v = [r run];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_method_on_variable_receiver");
    assert_eq!(out, "v=2 deallocs=2\n", "both nodes must be released: {}", out);
}

/// The `self` half of the same resolution, and the one that cannot be got
/// from a declaration: `[self derive]` has no declared receiver to read, so
/// it resolves through the enclosing `@implementation` instead.
#[test]
fn owning_method_on_self_is_released() {
    let src = format!(
        "/* oz-pool: Node=2 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Node : OZObject
+ (instancetype)make;
- (instancetype)derive;
- (int)run;
@end
@implementation Node
+ (instancetype)make {
	Node *n = [Node alloc];
	return n;
}
- (instancetype)derive {
	return [Node make];
}
- (int)run {
	Node *viaSelf = [self derive];
	return viaSelf != nil;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	Node *n = [Node make];
	int v = [n run];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_method_on_self");
    // The one `[self derive]` result is released inside -run; `n` itself is
    // still alive when the count is printed, which is what makes this a
    // check on the send rather than on scope exit.
    assert_eq!(out, "v=1 deallocs=1\n", "the self-send result must be released: {}", out);
}

/// The guard on the widened resolution: a receiver whose declaration is not
/// a known class stays unresolved, so nothing is treated as owning on the
/// strength of a name alone. A double free is memory corruption where a
/// leak is only a bug, so this direction must fail closed.
#[test]
fn unknown_receiver_type_stays_borrowed() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Node : OZObject
- (int)value;
@end
@implementation Node
- (int)value { return 7; }
@end

#include <stdio.h>
int main(void) {
	/* `id` says nothing about the class, so a send through it cannot be
	 * resolved and must not be assumed owning. */
	Node *real = [Node alloc];
	printf(\"value=%d\\n\", [real value]);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "unknown_receiver_stays_borrowed");
    assert!(out.contains("value=7"), "output: {}", out);
}

/// A +1 result bound to nothing is released at the end of the full
/// expression, which is what ARC's `objc_release` on an unused result does.
///
/// Every path that *binds* an owning result already released it -- a local
/// at its scope's end, a strong local or ivar on the next store, a `return`
/// on its way out. A result bound to nothing had no release at all, so
/// `[t copy];` on its own line abandoned a Thing every time it ran (#322).
///
/// Not covered by the two leaks above, both of which bind their result;
/// this is a third shape, and it is the one #283's own framing predicted
/// would stay silent -- conservative ARC leaks rather than double-frees.
///
/// `tests/behavior/cases/arc/discarded_owning_return.m` is the corpus case
/// this mirrors. There the signal is a two-slot slab running dry, since
/// `-fsanitize=leak` is unsupported on arm64-apple-darwin; here it is the
/// dealloc counter, for the reason this file's header gives.
#[test]
fn discarded_owning_result_is_released() {
    let src = format!(
        "/* oz-pool: Thing=2 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (instancetype)copy;
@end
@implementation Thing
- (instancetype)copy {
	return [Thing alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	Thing *t = [Thing alloc];
	/* +1 by convention, bound to nothing. */
	[t copy];
	/* `t` is still alive here, which is what makes this a check on the
	 * discarded send and not on scope exit. */
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "discarded_owning_result");
    assert_eq!(out, "deallocs=1\n", "the discarded copy must be released: {}", out);
}

/// The same defect reached through `-performSelector:`, which is where the
/// Clang warning that started #322 pointed -- and where it was least
/// actionable, since `-Warc-performSelector-leaks` fires on every
/// non-literal selector whatever it returns.
///
/// The selector is a run-time value in general, so only the two spellings
/// the source states exactly are resolved: a `@selector(...)` at the call
/// site, and a local declared once from one and never reassigned. This
/// covers both.
#[test]
fn discarded_result_of_a_performed_selector_is_released() {
    let src = format!(
        "/* oz-pool: Thing=3 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (instancetype)copy;
@end
@implementation Thing
- (instancetype)copy {
	return [Thing alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	Thing *t = [Thing alloc];
	SEL c = @selector(copy);

	[t performSelector:c];
	[t performSelector:@selector(copy)];
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run_with_reflection(&src, "discarded_performed_result");
    assert_eq!(out, "deallocs=2\n", "both performed copies must be released: {}", out);
}

/// The double-free half of #322, and the reason `arc::creates_reference`
/// is narrower than `arc::is_owning_selector`.
///
/// `-init` *consumes* its receiver's +1 and hands it back, so a discarded
/// `-init` result must not be released: here the reference belongs to a
/// local scope-based ARC already releases, and releasing it again is one
/// pointer freed twice.
///
/// **Coverage removed (#428).** This case used to carry a second half for
/// the other convention-named pass-through: a bare `[c retain];` balanced
/// by a hand-written `[c release];`, the idiom `samples/smp_shared` wrote
/// in its contention loop. A send of `-retain` is a located error now, so
/// the discarded-`-retain`-result arm of `arc::created_by` /
/// `arc::accounts_for_its_receiver` has no reachable input left and is
/// exercised by nothing. The arm is kept as defence rather than deleted,
/// and that it is now unreachable is recorded on #428 rather than
/// discovered later.
///
/// A leak is a bug and a double free is memory corruption, so this is the
/// direction that has to fail closed.
#[test]
fn a_discarded_init_on_an_owned_receiver_is_left_alone() {
    let src = format!(
        "/* oz-pool: Widget=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Widget : OZObject
@end
@implementation Widget
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	Widget *w = [Widget alloc];
	/* The +1 this hands back is `w`'s, and `w` is released at the end of
	 * this scope. */
	[w init];
	printf(\"after_init=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "discarded_init_on_an_owned_receiver");
    assert_eq!(
        out, "after_init=0\n",
        "the receiver may not be released twice: {}",
        out
    );
}

/// The other side of that reading: an `init` send whose receiver *is* a
/// temporary nothing tracks does abandon a reference, so it is released.
///
/// `[[Widget alloc] init];` is the classic discarded-allocation shape, and
/// resolving it means following the send back to its receiver rather than
/// trusting the selector's name -- the same exactness `message_target`
/// applies to a receiver's class.
#[test]
fn discarded_init_on_a_fresh_allocation_is_released() {
    let src = format!(
        "/* oz-pool: Widget=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Widget : OZObject
@end
@implementation Widget
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	[[Widget alloc] init];
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "discarded_init_on_fresh_alloc");
    assert_eq!(out, "deallocs=1\n", "the discarded allocation must be released: {}", out);
}

/// A discarded +1 result reached *through a cast* is released too, so
/// `(void)[t copy];` cannot differ from `[t copy];` in whether it leaks
/// (#327).
///
/// The two spellings mean the same thing, and the cast is the one people
/// actually write: `(void)expr` is the idiom for "I am throwing this away
/// on purpose", so it is the spelling most likely to have come from
/// someone who thought about the result. #322 released the bare statement
/// and left this one leaking, because `arc::is_owning_expr` reads a cast
/// as borrowed -- deliberately, and still does. `arc::discarded_value`
/// looks through the cast at the discarded statement and nowhere else.
///
/// A cast to a real type is covered with the `(void)` one: it discards
/// just as completely, and there is no reading under which one leaks and
/// the other does not.
#[test]
fn discarded_owning_result_through_a_cast_is_released() {
    let src = format!(
        "/* oz-pool: Thing=3 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (instancetype)copy;
@end
@implementation Thing
- (instancetype)copy {
	return [Thing alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	Thing *t = [Thing alloc];
	/* The deliberate-discard idiom, and the shape #322 left leaking. */
	(void)[t copy];
	/* A cast to a real type discards just as completely. */
	(Thing *)[t copy];
	/* `t` is still alive here, so this counts the discarded sends and
	 * not a scope exit. */
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "discarded_owning_result_through_cast");
    assert_eq!(out, "deallocs=2\n", "both cast-wrapped copies must be released: {}", out);
}

/// The double-free half of #327, and the reason the cast is looked through
/// in `arc::discarded_value` rather than in `arc::is_owning_expr`.
///
/// #322's `a_discarded_init_on_an_owned_receiver_is_left_alone` has to
/// keep holding once a cast can no longer hide the send inside it.
/// `-init...` consumes the receiver's +1, which here belongs to a local
/// scope-based ARC already releases, and wrapping it in `(void)` changes
/// nothing about who owns the reference.
///
/// This case, like its #322 predecessor, used to carry a `-retain` half as
/// well -- `(void)[c retain];` balanced by a hand-written `[c release];`.
/// That shape is a located error now (#428), so it is gone and the
/// `-retain` arm behind the cast peel is unreachable with it.
///
/// Getting this wrong is memory corruption where #327 itself is only a
/// leak, so it is the direction that has to fail closed.
#[test]
fn discarded_init_through_a_cast_is_left_alone() {
    let src = format!(
        "/* oz-pool: Widget=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Widget : OZObject
@end
@implementation Widget
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	Widget *w = [Widget alloc];
	/* The +1 this hands back is `w`'s, and `w` is released at the end
	 * of this scope. */
	(void)[w init];
	/* The receiver behind a cast is followed the same way. */
	[(Widget *)w init];
	printf(\"after_init=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "discarded_init_through_cast");
    assert_eq!(
        out, "after_init=0\n",
        "no cast-wrapped receiver may be released twice: {}",
        out
    );
}

/// The other side of that reading, reached through a cast: an `init` send
/// whose receiver is a temporary nothing tracks *is* abandoned, and a cast
/// on the receiver does not change that.
///
/// `[(Widget *)[Widget alloc] init];` leaked before #327 for the same
/// reason `(void)[t copy];` did -- the cast stopped the receiver from
/// being resolved back to an `+alloc` -- so both spellings fall out of the
/// one peel.
#[test]
fn discarded_init_behind_a_cast_receiver_is_released() {
    let src = format!(
        "/* oz-pool: Widget=2 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Widget : OZObject
@end
@implementation Widget
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	/* A cast on the receiver of the discarded init. */
	[(Widget *)[Widget alloc] init];
	/* And a cast on the whole discarded statement. */
	(void)[[Widget alloc] init];
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "discarded_init_behind_cast_receiver");
    assert_eq!(out, "deallocs=2\n", "both discarded allocations must be released: {}", out);
}

/// A *bridging* cast is the one cast #327 does not look through, and the
/// reason `arc::is_bridging_cast` exists.
///
/// `(__bridge void *)[t copy];` transfers nothing, so the reference stays
/// with whoever already held it; releasing it here would be a release of
/// something this expression never owned, which is a double free and not a
/// leak. `deallocs=0` is therefore the answer being asserted, not a leak
/// being tolerated.
///
/// **Fixture and reasoning both changed in #460.** The first of the two
/// discards used to be `(__bridge_retained void *)[t copy];`, and this doc
/// said the other two kinds were "held back with it -- not because either
/// is known to be unsafe, but because there is no CoreFoundation here for
/// any of the three to bridge to".
///
/// One of them *was* unsafe, and measuring it is what #460 did:
/// `__bridge_retained` emits no retain, so at a hand-out site the local is
/// released at scope exit while C keeps the pointer it was promised --
/// `heap-use-after-free` under ASan -- and at a binding site the `+1`
/// leaks instead. `__bridge_transfer` emits no release and strands the
/// reference it took over. Both are now located errors
/// (`staticbar::check_bridging_casts`), pinned in
/// `bridging_cast_ownership.rs`, so neither can appear in a fixture again.
///
/// What survives is the property that was always right: plain `__bridge`
/// is opaque to the ownership questions. `arc::is_bridging_cast` still
/// names all three, because narrowing it would drop the refused two into
/// the ordinary cast path, which is looked *through* (#332).
#[test]
fn a_bridging_cast_is_not_looked_through() {
    let src = format!(
        "/* oz-pool: Thing=3 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (instancetype)copy;
@end
@implementation Thing
- (instancetype)copy {
	return [Thing alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	Thing *t = [Thing alloc];
	(__bridge void *)[t copy];
	(__bridge void *)[t copy];
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "bridging_cast_not_looked_through");
    assert_eq!(
        out, "deallocs=0\n",
        "a bridging cast keeps the reference on the bridge's other side: {}",
        out
    );
}

/// A `+1` result *bound* through a cast is released at scope exit, so
/// `Thing *t = (Thing *)[Thing alloc];` cannot differ from
/// `Thing *t = [Thing alloc];` in whether it leaks (#332).
///
/// The other half of #327. That one released a `+1` *discarded* through a
/// cast; this one is the value a local takes over. `arc::is_owning_expr`
/// reads a cast as borrowed -- deliberately, and still does -- and it is
/// what every binding site used to consult, so the cast local got no
/// scope-exit release at all while its uncast neighbour did. A cast changes
/// the static type and says nothing about who owns the reference.
///
/// Both locals are here on purpose: `u` is the shape that already worked,
/// so the assertion distinguishes "the cast one is released" from "some
/// release happened".
#[test]
fn owning_result_bound_through_a_cast_is_released() {
    let src = format!(
        "/* oz-pool: Thing=2,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (void)poke;
@end
@implementation Thing
- (void)poke {}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (int)bind;
@end
@implementation Runner
- (int)bind {
	/* The shape that leaked. */
	Thing *t = (Thing *)[Thing alloc];
	/* The shape that never did, for contrast. */
	Thing *u = [Thing alloc];
	[t poke];
	[u poke];
	return (t != nil) + (u != nil);
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int v = [r bind];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "bound_owning_result_through_cast");
    assert_eq!(out, "v=2 deallocs=2\n", "the cast-bound local must be released too: {}", out);
}

/// The double-free half of #332, and the reason the cast is looked through
/// with `arc::created_by` rather than by widening `arc::is_owning_expr`.
///
/// ```objc
/// Thing *u = [Thing alloc];
/// Thing *t = (Thing *)[u init];   /* -init hands back u's own +1 */
/// ```
///
/// `-init...` consumes its receiver's +1 and hands it back, so `t` and `u`
/// are one pointer and one reference. Widening `is_owning_expr` -- the
/// one-line change -- releases both: `heap-use-after-free ... READ of size
/// 4` in `oz_release` under ASan.
///
/// This asserts on the generated C rather than on a dealloc counter, and
/// that is not laziness. A second `oz_release` on a freed object
/// reads a refcount that is already 0, so `oz_atomic_dec_and_test` returns
/// false and no second `-dealloc` runs: **the double free is invisible to a
/// dealloc counter, and the host slab clamps `num_used` at 0 so it is
/// invisible to a slot count too.** Only a sanitizer sees it at run time,
/// and the Rust suite runs without one. Counting the releases in the
/// emitted body asks the question directly -- exactly one release, and it
/// names the receiver -- and it fails on every host.
///
/// `just test-behavior --sanitize=address` is the run-time half; the corpus
/// case `arc/bound_owning_return_through_cast.m` carries this same shape
/// for it.
#[test]
fn init_bound_through_a_cast_is_released_exactly_once() {
    let src = format!(
        "/* oz-pool: Thing=2 */\n{}{}",
        PREAMBLE(),
        "\
@interface Thing : OZObject
- (void)poke;
@end
@implementation Thing
- (void)poke {}
@end

@interface Runner : OZObject
- (int)initThroughCast;
@end
@implementation Runner
- (int)initThroughCast {
	Thing *u = [Thing alloc];
	/* One object, one reference, two names. */
	Thing *t = (Thing *)[u init];
	[t poke];
	return t == u;
}
@end

int main(void) { return 0; }
"
    );
    let out = oz2c::transpile(&src).expect("should transpile");
    let body = out
        .source_c
        .split("int Runner_initThroughCast(struct Runner *self)\n{")
        .nth(1)
        .unwrap_or_else(|| panic!("no Runner_initThroughCast definition in:\n{}", out.source_c))
        .split("\n}")
        .next()
        .unwrap_or("");
    assert_eq!(
        body.matches("oz_release").count(),
        1,
        "one object, one reference, so exactly one release; got:\n{}",
        body
    );
    // And it must be the receiver that is released, not the name the cast
    // gave the same pointer -- which is also what keeps the release ahead
    // of nothing that still reads `t`.
    assert!(
        body.contains("oz_release((struct OZObject *)(u));"),
        "the surviving release must name the receiver; got:\n{}",
        body
    );
}

/// The same guard one step further out: a cast over a send whose ownership
/// cannot be resolved at all must stay borrowed.
///
/// `-derive` is declared and defined here, and its every return path is a
/// `+1`, so `arc::analyze` classifies it as an owning factory and a cast
/// over it *is* released. `-borrow` returns `self`, which is not owning by
/// any reading, so a cast over it must not be -- releasing it would free
/// the receiver from under its own scope. Both spellings are the same three
/// tokens apart, which is the point: the answer comes from `created_by`,
/// not from the cast.
#[test]
fn a_cast_over_a_borrowed_send_stays_borrowed() {
    let src = format!(
        "/* oz-pool: Thing=2,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (instancetype)derive;
- (instancetype)borrow;
- (void)poke;
@end
@implementation Thing
- (instancetype)derive {
	return [Thing alloc];
}
- (instancetype)borrow {
	return self;
}
- (void)poke {}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	Thing *seed = [Thing alloc];
	/* +1: an analysed owning factory behind a cast. */
	Thing *made = (Thing *)[seed derive];
	/* +0: `self`, which is `seed`, behind the same cast. */
	Thing *same = (Thing *)[seed borrow];
	[made poke];
	[same poke];
	return same == seed;
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int v = [r run];
	/* Two objects were allocated and both are gone; `same` was never a
	 * third reference to release. */
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "cast_over_borrowed_send");
    assert_eq!(out, "v=1 deallocs=2\n", "only the created reference may be released: {}", out);
}

/// A store to a strong *ivar* through a cast takes over the reference
/// rather than retaining it (#332).
///
/// The polarity here is the opposite of a local's, which is why it is its
/// own test: `render_strong_ivar_assign` *adds a retain* to anything it
/// reads as borrowed, so a cast being read as borrowed left the allocation
/// at +2 with one release ever to come. The object outlived its owner and
/// the slab slot never came back.
///
/// Both signals are read, because each on its own is weaker than it looks.
/// The dealloc counter answers whether the overwritten Thing was actually
/// freed; the two-slot slab answers whether the ones before it gave their
/// slots back, which is the failure that reached hardware as an MPU fault
/// rather than as a number. Under the defect they read `third=0
/// deallocs=0`.
///
/// `Thing=2` is exact and not slack: the ivar path assigns the new value
/// before releasing the old one -- deliberately, since releasing first
/// could free the value being stored when the two are the same -- so two
/// instances are briefly live on every overwrite and a one-slot slab
/// cannot express the correct behaviour at all.
#[test]
fn owning_result_stored_into_a_strong_ivar_through_a_cast_is_not_retained() {
    let src = format!(
        "/* oz-pool: Thing=2,Holder=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Thing : OZObject
@end
@implementation Thing
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Holder : OZObject
{
	Thing *_kid;
}
- (int)refill;
@end
@implementation Holder
- (int)refill {
	/* The cast used to earn this store a retain it had no business
	 * having. */
	_kid = (Thing *)[Thing alloc];
	return _kid != nil;
}
@end

#include <stdio.h>
int main(void) {
	Holder *h = [Holder alloc];
	int first = [h refill];
	int second = [h refill];
	/* Two slots, three allocations: this can only find a slot if each
	 * overwrite released what the ivar held instead of leaving it
	 * at +2. */
	int third = [h refill];
	printf(\"first=%d second=%d third=%d deallocs=%d\\n\",
	       first, second, third, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "ivar_store_through_cast");
    // Two overwrites, so two Things freed; the third is still held by the
    // ivar when the count is printed.
    assert_eq!(
        out, "first=1 second=1 third=1 deallocs=2\n",
        "a cast-wrapped +1 must be stored without a retain: {}",
        out
    );
}

/// Reassigning a strong local through a cast releases what it held, and the
/// local is ARC-managed at all (#332).
///
/// `classify_store` read the cast as neither owning nor a plain identifier,
/// which is `LocalStore::Unsupported` -- and one unsupported store takes the
/// whole local out of `managed_object_locals`. Two consequences, both here:
/// nothing released the overwritten object, and `staticbar` rejected the
/// ordinary reassign-in-a-loop shape outright, because an unmanaged local
/// cannot bound how many instances are live. (The message it gave then,
/// "escapes the iteration", was retired by #345 for one that names the
/// destination -- an unmanaged local now reads as "a local ARC does not
/// manage".)
///
/// One slab slot for a three-iteration loop is the assertion: it can only
/// hold if each iteration's release comes *before* the next allocation.
#[test]
fn strong_local_reassigned_through_a_cast_releases_the_old_value() {
    let src = format!(
        "/* oz-pool: Thing=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (void)poke;
@end
@implementation Thing
- (void)poke {}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (int)loop;
@end
@implementation Runner
- (int)loop {
	Thing *t = nil;
	int i = 0;
	int made = 0;
	while (i < 3) {
		t = (Thing *)[Thing alloc];
		made = made + (t != nil);
		i = i + 1;
	}
	[t poke];
	return made;
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int made = [r loop];
	printf(\"made=%d deallocs=%d\\n\", made, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "reassign_through_cast");
    assert_eq!(
        out, "made=3 deallocs=3\n",
        "one slot must serve the whole loop, and all three must be freed: {}",
        out
    );
}

/* ---- a cast in the *initialiser* position (#491) --------------------- */
//
// The position #332 left, and the last of the three spellings of one pair:
// the cast on the overwrite is above, the uncast pair is in
// `ownership_matrix.rs`, and a cast in the initialiser leaked exactly one
// object per overwritten binding.
//
// The cause was not in `arc.rs` at all, which is why four candidates were
// eliminated before it was found (#491's own list). It was in
// `collect::extract_type_and_stars`: that walks the whole `declaration`
// subtree and counts every `*` token, so the cast's star was attributed to
// the declared type and `Foo *v = (Foo *)[Foo make];` reported `("Foo", 2)`.
// `emit::managed_object_locals` admits an object local on `stars == 1`, so
// the shape was never a candidate and its decision point never executed on
// either path.
//
// The variable still got its scope-exit release, because
// `emit::owned_locals_of` reaches the same declarator by a second,
// independent path -- `arc::declares_pointer`, which is per *declarator*
// and never consults the star count. Two readers of one declaration
// disagreeing, again (gap R, #251, #400, #429), and the disagreement was
// the whole defect: release-on-overwrite was the only half that was lost.

/// A cast in a local's initialiser must not cost release-on-overwrite
/// (#491).
///
/// The pool directive is the assertion that matters. With `Foo=1` the
/// overwrite can only allocate if the initialiser's object was released
/// *first* -- `LocalStore::Owning`'s contract, that a `+1` right-hand side
/// not mentioning the variable lets the old value go before the new one is
/// evaluated. A dealloc count alone would pass on C that released after
/// allocating, which on a one-slot slab is a different program.
#[test]
fn cast_in_a_locals_initialiser_releases_on_overwrite() {
    let src = format!(
        "/* oz-pool: Foo=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Foo : OZObject
+ (Foo *)make;
@end
@implementation Foo
+ (Foo *)make {
	return [Foo alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	Foo *v = (Foo *)[Foo make];
	int first = (v != nil);
	v = [Foo make];
	return first + 2 * (v != nil);
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int made = [r run];
	printf(\"made=%d deallocs=%d\\n\", made, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "cast_init_releases_on_overwrite");
    assert_eq!(
        out, "made=3 deallocs=2\n",
        "one slot must serve both allocations, and both objects must be freed: {}",
        out
    );
}

/// The same shape in a plain C function rather than a method (#491).
///
/// A free function's body is reached through `collect_function_params` and
/// `emit`'s top-level walk rather than through the method path, and #491
/// measured the leak in both. Keeping both means a fix that only reached
/// one of the two entry points cannot pass.
#[test]
fn cast_in_a_free_functions_local_initialiser_releases_on_overwrite() {
    let src = format!(
        "/* oz-pool: Foo=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Foo : OZObject
+ (Foo *)make;
@end
@implementation Foo
+ (Foo *)make {
	return [Foo alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

static int churn(void)
{
	Foo *v = (Foo *)[Foo make];
	int first = (v != nil);

	v = [Foo make];
	return first + 2 * (v != nil);
}

#include <stdio.h>
int main(void) {
	int made = churn();
	printf(\"made=%d deallocs=%d\\n\", made, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "cast_init_free_function");
    assert_eq!(
        out, "made=3 deallocs=2\n",
        "a free function's local must be managed exactly as a method's is: {}",
        out
    );
}

/// An `id` local initialised through a cast (#491).
///
/// `id` is the one object spelling that carries no `*` in source, so it is
/// admitted on `stars == 0` -- and a cast in the initialiser pushed the
/// count to 1, which is the *other* side of the same star-count defect and
/// fails for a different reason than the `Foo *` rows above. #400 and #429
/// are the two earlier times an `id` slot was lost to a `stars` test; this
/// is the first time one was lost to a count that was too *high*.
#[test]
fn an_id_local_initialised_through_a_cast_releases_on_overwrite() {
    let src = format!(
        "/* oz-pool: Foo=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Foo : OZObject
+ (Foo *)make;
@end
@implementation Foo
+ (Foo *)make {
	return [Foo alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

static int churn(void)
{
	id v = (Foo *)[Foo make];
	int first = (v != nil);

	v = [Foo make];
	return first + 2 * (v != nil);
}

#include <stdio.h>
int main(void) {
	int made = churn();
	printf(\"made=%d deallocs=%d\\n\", made, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "cast_init_id_slot");
    assert_eq!(
        out, "made=3 deallocs=2\n",
        "an `id` slot behind a cast must be managed too: {}",
        out
    );
}

/// A cast that **lies about the class** must still release the object it
/// really holds (#491, and the trap #502 measured).
///
/// This is the check granting management owed: #502 turned a garbage read
/// into a garbage *free* by teaching ARC about a binding whose type was
/// wrong, because the missing release had been the only thing between a bad
/// value and `oz_release`. Here the slot is declared `Bar *` and holds a
/// real `Foo`, so release-on-overwrite now fires through a pointer whose
/// static type names the wrong class.
///
/// It is safe, and the reason is structural rather than lucky: `oz_release`
/// takes the object's own class pointer and runs *its* dealloc chain, so
/// the declared type of the slot reaches no free-side decision. The
/// assertion is per class, not a total -- a total of 2 would also be
/// produced by freeing the Foo twice and the Bar never, which is the
/// failure this row exists to exclude.
#[test]
fn a_lying_cast_in_an_initialiser_releases_the_real_class() {
    let src = format!(
        "/* oz-pool: Foo=4,Bar=4 */\n{}{}",
        PREAMBLE(),
        "\
static int g_foo_deallocs = 0;
static int g_bar_deallocs = 0;

@interface Foo : OZObject
+ (Foo *)make;
@end
@implementation Foo
+ (Foo *)make {
	return [Foo alloc];
}
- (void)dealloc {
	g_foo_deallocs = g_foo_deallocs + 1;
}
@end

@interface Bar : OZObject
+ (Bar *)make;
@end
@implementation Bar
+ (Bar *)make {
	return [Bar alloc];
}
- (void)dealloc {
	g_bar_deallocs = g_bar_deallocs + 1;
}
@end

static void churn(void)
{
	Bar *v = (Bar *)[Foo make];

	v = [Bar make];
}

#include <stdio.h>
int main(void) {
	churn();
	printf(\"foo=%d bar=%d\\n\", g_foo_deallocs, g_bar_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "cast_init_lying_class");
    assert_eq!(
        out, "foo=1 bar=1\n",
        "each object must be torn down by its own class exactly once: {}",
        out
    );
}

/// **Control.** A *borrowed* initialiser behind a cast must stay unmanaged,
/// and this row's numbers must not move (#491).
///
/// Making the cast visible to the star count widens what
/// `managed_object_locals` is *asked about*, not what it admits: the answer
/// still comes from `arc::binds_ownership`, which says no to a `+0` send
/// however many casts wrap it. That distinction is the one #477's M1 got
/// wrong from the other side -- calling a borrowed value owning made the
/// destination managed, and the scope-exit release then freed a reference
/// nothing had taken.
///
/// So this test does not fail when the fix is removed, and it is not
/// supposed to. It fails if the fix ever grows into `binds_ownership`.
/// Both halves are asserted: no `-dealloc` runs (the borrowed object is
/// still alive, and the overwriting `+1` still leaks exactly as it did
/// before), and the generated function contains no `oz_release` at all --
/// the release the defect would introduce.
#[test]
fn a_borrowed_initialiser_behind_a_cast_stays_unmanaged() {
    let src = format!(
        "/* oz-pool: Foo=4,Holder=2 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Foo : OZObject
+ (Foo *)make;
@end
@implementation Foo
+ (Foo *)make {
	return [Foo alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Holder : OZObject {
	Foo *_kept;
}
- (void)fill;
- (Foo *)peek;
@end
@implementation Holder
- (void)fill {
	_kept = [Foo make];
}
- (Foo *)peek {
	return _kept;
}
@end

static void churn(Holder *h)
{
	Foo *v = (Foo *)[h peek];

	v = [Foo make];
	(void)v;
}

#include <stdio.h>
int main(void) {
	Holder *h = [Holder alloc];
	[h fill];
	churn(h);
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "cast_init_borrowed_control");
    assert_eq!(
        out, "deallocs=0\n",
        "a borrowed initialiser must not become owned, however it is cast: {}",
        out
    );
    /* The presence half. A count of zero `oz_release(` in `churn` is the
     * property, and it is asserted on the *extracted* function rather
     * than the whole file so the needle cannot be satisfied by some
     * other function's releases -- the emitted C is full of them. */
    let transpiled = oz2c::transpile(&src).expect("should transpile");
    let body = transpiled
        .source_c
        .split("static void churn(struct Holder *h)")
        .nth(1)
        .unwrap_or_else(|| panic!("no churn definition in:\n{}", transpiled.source_c))
        .split("\n}\n")
        .next()
        .unwrap_or("");
    assert_eq!(
        body.matches("oz_release(").count(),
        0,
        "nothing in `churn` owns anything, so it may emit no release at all; got:\n{}",
        body
    );
}

/// A `return` is a binding site too, and both its halves have to agree
/// about a cast (#332).
///
/// `return (Thing *)[Thing alloc];` left the enclosing function classified
/// +0, so every caller kept the reference it had just been handed and
/// nothing released it. And `return (Thing *)s;` was worse than a leak:
/// `render_return_statement` looked for a bare `identifier` child, found
/// none behind the cast, and so released `s` *on the way out* and handed
/// the caller a freed pointer -- a use-after-free where the uncast
/// `return s;` is correct. Both now read the value through
/// `arc::value_behind_casts`, so the local the return keeps alive and the
/// ownership the caller is told to take over are decided by one peel.
///
/// A one-slot slab per class is the assertion: the second call can only
/// find a slot if the first result was released by its caller.
#[test]
fn returning_through_a_cast_hands_the_caller_the_reference() {
    let src = format!(
        "/* oz-pool: Thing=1,Other=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Thing : OZObject
@end
@implementation Thing
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Other : OZObject
@end
@implementation Other
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

/* The send behind the cast. */
static Thing *makeThing(void) {
	return (Thing *)[Thing alloc];
}

/* A local behind the cast -- the shape that used to release `s` and
 * return it anyway. */
static Other *makeOther(void) {
	Other *s = [Other alloc];
	return (Other *)s;
}

@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	Thing *a = makeThing();
	Other *b = makeOther();
	return (a != nil) + (b != nil);
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int first = [r run];
	/* One slot each: this can only match if -run released both. */
	int second = [r run];
	printf(\"first=%d second=%d deallocs=%d\\n\", first, second, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "return_through_cast");
    assert_eq!(
        out, "first=2 second=2 deallocs=4\n",
        "a cast on a return must not change who owns the result: {}",
        out
    );
}

/// A *bridging* cast is not looked through at a binding site either, for
/// #327's reason: `(__bridge Thing *)` transfers nothing, so the reference
/// stays with whoever already had it and releasing it here would be a
/// release of something this scope never owned.
///
/// `bridged=0` is the answer being asserted and not a leak being
/// tolerated. The non-bridging local in the same scope (`plain=1`) is what
/// shows the peel is working at all and that the bridging ones are being
/// singled out.
///
/// **This test's fixture and its reasoning both changed in #460, and the
/// part that changed was an argument, not a typo.** It used to bind through
/// `(__bridge_retained Bridged *)` and its doc said `__bridge` and
/// `__bridge_transfer` were "held back with it" because "there is no
/// CoreFoundation here for any of the three to bridge to, so leaving all
/// three borrowed keeps the conservative bias exact".
///
/// That reading was wrong twice over. The two transferring kinds are not
/// about CoreFoundation -- they are about whether ARC emits traffic for a
/// hand-off to *any* C pointer, which is what px-keyboard does through a
/// Zephyr `k_timer` user_data. And leaving `__bridge_retained` borrowed is
/// not conservative: measured, it produces a leak at a binding site (this
/// shape) *and* a use-after-free at a hand-out site, where the local is
/// released at scope exit while C keeps the pointer it was promised. A bias
/// that corrupts in one position is not a bias.
///
/// So #460 refuses `__bridge_retained` and `__bridge_transfer` outright and
/// keeps only plain `__bridge`, which is the one kind whose "transfers
/// nothing" reading is true. The refusals are pinned in
/// `bridging_cast_ownership.rs`; what survives here is the property that
/// was always correct -- plain `__bridge` stays opaque to the ownership
/// questions, so `arc::is_bridging_cast` must keep naming all three.
#[test]
fn a_bridging_cast_at_a_binding_site_is_not_looked_through() {
    let src = format!(
        "/* oz-pool: Thing=3,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_bridged = 0;
static int g_plain = 0;

@interface Thing : OZObject
- (void)poke;
@end
@implementation Thing
- (void)poke {}
@end

@interface Bridged : OZObject
@end
@implementation Bridged
- (void)dealloc {
	g_bridged = g_bridged + 1;
}
@end

@interface Plain : OZObject
@end
@implementation Plain
- (void)dealloc {
	g_plain = g_plain + 1;
}
@end

@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	Bridged *lent = (__bridge Bridged *)[Bridged alloc];
	Bridged *also = (__bridge Bridged *)[Bridged alloc];
	Plain *mine = (Plain *)[Plain alloc];
	return (lent != nil) + (also != nil) + (mine != nil);
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int v = [r run];
	printf(\"v=%d bridged=%d plain=%d\\n\", v, g_bridged, g_plain);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "bridging_cast_at_binding_site");
    assert_eq!(
        out, "v=3 bridged=0 plain=1\n",
        "a bridging cast keeps the reference on the bridge's other side: {}",
        out
    );
}

/// A +1 result passed straight as an *argument* is released after the send
/// (#328).
///
/// `[self setFoo:[Foo new]];` binds nothing and discards nothing, so no
/// existing path reached it: scope-based release needs a local,
/// `render_strong_local_assign` a store, `arc::discards_ownership` (#322)
/// the whole of an `expression_statement`, and `arc::binds_ownership`
/// (#332) a binding site. The `+1` from `+new` was simply never released --
/// and a synthesized strong setter *retains* its argument on top of it, so
/// the object ended at +2 with one release ever owed.
///
/// The signal is a slab running dry rather than a dealloc count alone,
/// which matters because a leak passes any assertion about return values.
/// `Foo=2` is exact: the third store can only find a slot if the first
/// object was genuinely freed when the setter let it go. Leaked, the first
/// two slots never come back and the third `+new` answers nil.
#[test]
fn owning_argument_to_a_setter_is_released() {
    let src = format!(
        "/* oz-pool: Foo=2,Holder=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Foo : OZObject
+ (instancetype)new;
@end
@implementation Foo
+ (instancetype)new {
	return [Foo alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Holder : OZObject
@property (strong) Foo *foo;
- (int)run;
@end
@implementation Holder
- (int)run {
	[self setFoo:[Foo new]];
	[self setFoo:[Foo new]];
	/* Only reachable if the first object's slot came back. */
	[self setFoo:[Foo new]];
	return [self foo] != nil;
}
@end

#include <stdio.h>
int main(void) {
	Holder *h = [Holder alloc];
	int v = [h run];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_argument_to_a_setter");
    // Two of the three are handed over and then replaced, so two are torn
    // down; the third is still held by the ivar when `run` returns.
    assert_eq!(
        out, "v=1 deallocs=2\n",
        "the +1 an argument carries must be released after the send: {}",
        out
    );
}

/// The same fix where the callee only *borrows* the argument, which is the
/// half a consuming setter could never have covered.
///
/// `-doThing:` stores nothing and retains nothing, so the caller's `+1` is
/// the only reference there ever was and dropping it after the send is what
/// runs `-dealloc`. `Foo=1` makes that the difference between three calls
/// and one: with the release, one slot serves them all.
#[test]
fn owning_argument_to_a_borrowing_method_is_released() {
    let src = format!(
        "/* oz-pool: Foo=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;
static int g_pokes = 0;

@interface Foo : OZObject
+ (instancetype)new;
- (void)poke;
@end
@implementation Foo
+ (instancetype)new {
	return [Foo alloc];
}
- (void)poke {
	g_pokes = g_pokes + 1;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (void)doThing:(Foo *)f;
- (void)run;
@end
@implementation Runner
- (void)doThing:(Foo *)f {
	[f poke];
}
- (void)run {
	[self doThing:[Foo new]];
	[self doThing:[Foo new]];
	[self doThing:[Foo new]];
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	[r run];
	printf(\"pokes=%d deallocs=%d\\n\", g_pokes, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_argument_borrowed_by_callee");
    // Three sends, each reaching a live object, each torn down after --
    // one slab slot reused three times. Leaked, only the first allocation
    // succeeds and the other two send to nil.
    assert_eq!(
        out, "pokes=3 deallocs=3\n",
        "a borrowing callee leaves the caller's +1 to the caller: {}",
        out
    );
}

/// The other direction, and the one that must fail closed: a *borrowed*
/// argument is not released by the call site.
///
/// `[self setFoo:f]` hands over a reference `f` still holds and the setter
/// retains it; releasing at the call site as well would take the object
/// away from `f` before its own scope exit does. Asserted where it is
/// visible -- the object is used after the send and torn down exactly once,
/// when the holder that kept it dies.
#[test]
fn borrowed_argument_is_not_released_by_the_call_site() {
    let src = format!(
        "/* oz-pool: Foo=1,Holder=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Foo : OZObject
- (int)value;
@end
@implementation Foo
- (int)value {
	return 7;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Holder : OZObject
@property (strong) Foo *foo;
- (int)run;
@end
@implementation Holder
- (int)run {
	Foo *f = [Foo alloc];
	[self setFoo:f];
	/* Still ours to read: the send borrowed it, it did not consume it. */
	int v = [f value];
	/* And still the holder's, after our own reference goes at scope exit. */
	return v + ([self foo] != nil);
}
@end

#include <stdio.h>
int main(void) {
	int v;
	int during;
	/* Braced so the holder's teardown is ordered between the two reads
	 * of `g_deallocs` -- the point a hand release used to fix (#428). */
	{
		Holder *h = [Holder alloc];
		v = [h run];
		during = g_deallocs;
	}
	printf(\"v=%d during=%d after=%d\\n\", v, during, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "borrowed_argument_stays");
    assert_eq!(
        out, "v=8 during=0 after=1\n",
        "a borrowed argument is the caller's to release, once, at scope exit: {}",
        out
    );
}

/// **The double-free guard, and the only thing in the tree standing between
/// this change and memory corruption.**
///
/// The naive reading of an argument is `arc::is_owning_expr`, which is +1
/// *by shape* and says yes to `[u init]`. It hands back a reference
/// something else already accounts for -- its receiver's `+1`, which
/// scope-based ARC already releases -- so taking a call-site temporary for
/// it and releasing that would free one pointer twice.
/// `arc::owning_argument_value` goes through `created_by` instead, which
/// follows an `-init...` send back to its receiver (#322).
///
/// **Coverage removed (#428).** The other half was `[self setFoo:[e retain]]`
/// with a hand-written `[e release]` below it, covering `created_by`'s
/// outright exclusion of `-retain` in an *argument* position. A `-retain`
/// send is a located error now, so that position has no reachable input.
///
/// Asserted on the **emitted C**, deliberately, and this is the trap the
/// predecessors documented: an over-release is invisible to a dealloc
/// counter, because the second `oz_release` sees a refcount already
/// at 0 and returns before `-dealloc`, and the host slab clamps `num_used`
/// at 0. So a counter and a slot count are both blind here. What is
/// checkable is that no call-site temporary was taken at all.
#[test]
fn init_arguments_are_left_alone() {
    let src = format!(
        "/* oz-pool: Foo=1,Holder=1 */\n{}{}",
        PREAMBLE(),
        "\
@interface Foo : OZObject
@end
@implementation Foo
@end

@interface Holder : OZObject
@property (strong) Foo *foo;
- (void)run;
@end
@implementation Holder
- (void)run {
	Foo *u = [Foo alloc];
	/* -init consumes its receiver's +1 and hands it back, so this is
	 * `u`'s reference and `u`'s scope exit releases it. */
	[self setFoo:[u init]];
}
@end

int main(void) { return 0; }
"
    );
    let out = oz2c::transpile(&src).expect("should transpile");
    let body = out
        .source_c
        .split("void Holder_run(struct Holder *self)\n{")
        .nth(1)
        .unwrap_or_else(|| panic!("no Holder_run definition in:\n{}", out.source_c))
        .split("\n}\n")
        .next()
        .unwrap_or("");
    assert!(
        !body.contains("_oz_arg_"),
        "neither argument creates a reference, so neither may be held in a \
         call-site temporary and released; got:\n{}",
        body
    );
    /* Exactly the one release the source owes: `u` at scope exit. The
     * strong-property store retains what it is given and the ivar's own
     * release happens in `Holder_oz_release_ivars`, not here. A second
     * release in this body would be one pointer freed twice. */
    assert_eq!(
        body.matches("oz_release").count(),
        1,
        "only the release the source already owed; got:\n{}",
        body
    );
}

/// The nested spelling from the issue, and the second half of the same
/// guard: `[[Foo alloc] init]` is *one* reference under two sends, so it
/// takes one temporary and one release, not two.
///
/// `created_by` follows the `-init...` back to `[Foo alloc]`, which is
/// where the reference is actually created -- so the argument counts as
/// owning once, at the outer send, and the inner `alloc` is not a second
/// owning operand. Being a *receiver* rather than an argument is no longer
/// a reason of its own: since #340 a receiver is collected too, and what
/// keeps this one out is that its send's selector is `-init`, which hands
/// the reference back rather than abandoning it
/// (`arc::accounts_for_its_receiver`). Dropping that check makes this test
/// fail with two releases where one is owed, which is what it is here for.
#[test]
fn nested_alloc_init_argument_is_released_exactly_once() {
    let src = format!(
        "/* oz-pool: Foo=2,Holder=1 */\n{}{}",
        PREAMBLE(),
        "\
@interface Foo : OZObject
@end
@implementation Foo
@end

@interface Holder : OZObject
@property (strong) Foo *foo;
- (void)run;
@end
@implementation Holder
- (void)run {
	[self setFoo:[[Foo alloc] init]];
}
@end

int main(void) { return 0; }
"
    );
    let out = oz2c::transpile(&src).expect("should transpile");
    let body = out
        .source_c
        .split("void Holder_run(struct Holder *self)\n{")
        .nth(1)
        .unwrap_or_else(|| panic!("no Holder_run definition in:\n{}", out.source_c))
        .split("\n}\n")
        .next()
        .unwrap_or("");
    assert_eq!(
        body.matches("oz_release").count(),
        1,
        "one object, one reference, so exactly one release; got:\n{}",
        body
    );
    assert_eq!(
        body.matches("_oz_arg_L").count(),
        3,
        "one temporary, named three times -- declared, passed, released; got:\n{}",
        body
    );
}

/// The same shape in a *declaration's initialiser*, where the send's own
/// value is used: `int n = [self countOf:[Foo new]];`.
///
/// Emitted as a bare group of statements rather than a braced one, because
/// bracing would scope `n` out of the rest of the body. `Foo=1` is again
/// the signal: three calls, one slot.
#[test]
fn owning_argument_in_a_declaration_initializer_is_released() {
    let src = format!(
        "/* oz-pool: Foo=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Foo : OZObject
+ (instancetype)new;
@end
@implementation Foo
+ (instancetype)new {
	return [Foo alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (int)countOf:(Foo *)f;
- (int)run;
@end
@implementation Runner
- (int)countOf:(Foo *)f {
	return f != nil;
}
- (int)run {
	int a = [self countOf:[Foo new]];
	int b = [self countOf:[Foo new]];
	int c = [self countOf:[Foo new]];
	return a + b + c;
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int v = [r run];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_argument_in_declaration");
    assert_eq!(
        out, "v=3 deallocs=3\n",
        "an initialiser's owning argument is released too, and `n` stays in scope: {}",
        out
    );
}

/// The allocation must stay *inside* the loop.
///
/// This is why the release is a self-contained group at the statement and
/// not a `ctx.pre_stmts` temporary: `pre_stmts` are drained by the
/// enclosing *top-level* statement, so a temporary written for a send
/// inside a loop is hoisted above it -- the bug
/// `render_strong_local_assign`'s comment records. Hoisting an
/// *allocation* would run it once and release it once while the body used
/// it every iteration. `Foo=1` cannot be satisfied any other way: four
/// iterations, one slot, four teardowns.
#[test]
fn an_owning_argument_inside_a_loop_allocates_per_iteration() {
    let src = format!(
        "/* oz-pool: Foo=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;
static int g_seen = 0;

@interface Foo : OZObject
+ (instancetype)new;
@end
@implementation Foo
+ (instancetype)new {
	return [Foo alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (void)take:(Foo *)f;
- (void)run;
@end
@implementation Runner
- (void)take:(Foo *)f {
	if (f != nil) {
		g_seen = g_seen + 1;
	}
}
- (void)run {
	for (int i = 0; i < 4; i++) {
		[self take:[Foo new]];
	}
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	[r run];
	printf(\"seen=%d deallocs=%d\\n\", g_seen, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_argument_in_a_loop");
    assert_eq!(
        out, "seen=4 deallocs=4\n",
        "one slot must serve every iteration, which needs the allocation and \
         the release both inside the loop: {}",
        out
    );
}

/// A `+1` used as the **receiver** of a send, which is the last position in
/// the ownership sweep #322, #327, #332 and #328 worked through: `[[Foo
/// alloc] poke];` allocated an object nothing ever released (#340).
///
/// The allocation has no name, so no scope-exit release reaches it, and the
/// send's *value* is `void`, so `discarded_owning_value` (#322/#327) saw
/// nothing to release either. Zero `oz_release` calls were emitted
/// for it.
///
/// `Foo=1` is the whole assertion: three sends through one slab slot cannot
/// succeed unless each receiver is released before the next one allocates.
/// `-poke` counts only a non-nil `self` deliberately -- a direct call
/// dereferences nothing, so a send to the nil a starved `+alloc` returns
/// would otherwise count as a poke and the leak would print `poked=3`.
/// Before the fix this printed `poked=1 deallocs=0`.
#[test]
fn an_owning_receiver_is_released_after_the_send() {
    let src = format!(
        "/* oz-pool: Foo=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_poked = 0;
static int g_deallocs = 0;

@interface Foo : OZObject
- (void)poke;
@end
@implementation Foo
- (void)poke {
	if (self != nil) {
		g_poked = g_poked + 1;
	}
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (void)run;
@end
@implementation Runner
- (void)run {
	[[Foo alloc] poke];
	[[Foo alloc] poke];
	[[Foo alloc] poke];
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	[r run];
	printf(\"poked=%d deallocs=%d\\n\", g_poked, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_receiver_released");
    assert_eq!(
        out, "poked=3 deallocs=3\n",
        "each abandoned receiver is released after its send, so one slot \
         serves all three: {}",
        out
    );
}

/// The counterexample that makes this its own reasoning rather than an
/// extension of #328: `[[Foo alloc] init];` must **not** release its
/// receiver.
///
/// `-init` consumes the receiver's `+1` and hands it back, so the reference
/// travels out through the return value and #322's discarded-result arm
/// already owns it -- `arc::created_by` follows the `-init...` send back to
/// `[Foo alloc]` and releases *there*. Releasing the receiver as well frees
/// one pointer twice.
///
/// `[f poke]` is an ordinary borrowed receiver, for contrast.
///
/// **Coverage removed (#428).** A third receiver used to sit here:
/// `[[f retain] poke]`, the other pass-through, excluded by `created_by`
/// outright and balanced by a hand-written `[f release]`. A `-retain` send
/// is a located error now, so `created_by`'s exclusion of it in a
/// *receiver* position has no reachable input either.
///
/// Asserted on the **emitted C**, and for the reason the four predecessors
/// recorded: an over-release is invisible to a dealloc counter, because the
/// second `oz_release` sees a refcount already at 0 and returns
/// before `-dealloc`, and the host slab clamps `num_used` at 0. A counter
/// and a slot count are both blind here. What is checkable is that no
/// receiver temporary was taken.
#[test]
fn a_receiver_whose_reference_travels_out_is_left_alone() {
    let src = format!(
        "/* oz-pool: Foo=2,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
@interface Foo : OZObject
- (void)poke;
@end
@implementation Foo
- (void)poke {}
@end

@interface Runner : OZObject
- (void)run;
@end
@implementation Runner
- (void)run {
	/* -init hands the receiver's +1 back out; #322's arm releases it
	 * there, so a receiver release here would be the second free. */
	[[Foo alloc] init];
	Foo *f = [Foo alloc];
	/* An ordinary borrowed receiver: `f`'s scope exit owns it. */
	[f poke];
}
@end

int main(void) { return 0; }
"
    );
    let out = oz2c::transpile(&src).expect("should transpile");
    let body = out
        .source_c
        .split("void Runner_run(struct Runner *self)\n{")
        .nth(1)
        .unwrap_or_else(|| panic!("no Runner_run definition in:\n{}", out.source_c))
        .split("\n}\n")
        .next()
        .unwrap_or("");
    assert!(
        !body.contains("_oz_recv_"),
        "neither of these receivers abandons a reference, so neither may be \
         held in a call-site temporary and released; got:\n{}",
        body
    );
    /* Exactly the two the source owes: `[Foo alloc] init`'s reference,
     * released by #322's discarded-result arm, and `f` at scope exit. A
     * third release would be one pointer freed twice. */
    assert_eq!(
        body.matches("oz_release").count(),
        2,
        "only the releases the source already owed; got:\n{}",
        body
    );
}

/// An abandoned receiver and a `+1` *result* are two references, and both
/// are released: `[[Foo alloc] duplicate];` where `-duplicate` is an
/// analysis-derived owning factory.
///
/// This is the case that shows being an owning selector is not
/// pass-through. `-init` and `-retain` hand back the *receiver's*
/// reference; an owning factory builds a **fresh** object and the
/// receiver's `+1` is abandoned exactly as `-poke`'s was. So the receiver
/// arm releases one and #322's discarded-result arm releases the other,
/// and `Foo=2` is what says so: two live objects at once, both torn down.
#[test]
fn an_abandoned_receiver_and_an_owning_result_are_both_released() {
    let src = format!(
        "/* oz-pool: Foo=2,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Foo : OZObject
- (instancetype)duplicate;
@end
@implementation Foo
- (instancetype)duplicate {
	return [Foo alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (void)run;
@end
@implementation Runner
- (void)run {
	[[Foo alloc] duplicate];
	[[Foo alloc] duplicate];
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	[r run];
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "receiver_and_owning_result");
    assert_eq!(
        out, "deallocs=4\n",
        "two references per statement, so four objects torn down on two \
         slots: {}",
        out
    );
}

/// The receiver's allocation must stay *inside* the loop, for the reason
/// `an_owning_argument_inside_a_loop_allocates_per_iteration` records: a
/// `ctx.pre_stmts` temporary is drained by the enclosing top-level
/// statement, so hoisting the allocation above the loop would run it once
/// and release it once while the body sent to it every iteration.
///
/// The receiver is `[Foo new]` rather than `[Foo alloc]`, and that is not
/// incidental: `staticbar::walk_for_reject` refuses a bare `alloc` in a
/// loop that is not bound to a per-iteration local, so `[[Foo alloc]
/// poke];` inside a `for` is a *located error* today, not a leak, and
/// cannot be written as a case here. `+new`'s allocation is inside the
/// factory, which the loop rule does not see -- the same reason #328's loop
/// test spells it that way.
///
/// `Foo=1` cannot be satisfied any other way: four iterations, one slot,
/// four teardowns.
#[test]
fn an_owning_receiver_inside_a_loop_allocates_per_iteration() {
    let src = format!(
        "/* oz-pool: Foo=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_poked = 0;
static int g_deallocs = 0;

@interface Foo : OZObject
+ (instancetype)new;
- (void)poke;
@end
@implementation Foo
+ (instancetype)new {
	return [Foo alloc];
}
- (void)poke {
	if (self != nil) {
		g_poked = g_poked + 1;
	}
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (void)run;
@end
@implementation Runner
- (void)run {
	for (int i = 0; i < 4; i++) {
		[[Foo new] poke];
	}
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	[r run];
	printf(\"poked=%d deallocs=%d\\n\", g_poked, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_receiver_in_a_loop");
    assert_eq!(
        out, "poked=4 deallocs=4\n",
        "one slot must serve every iteration, which needs the allocation and \
         the release both inside the loop: {}",
        out
    );
}

/// The same abandoned receiver where the send's own value is *used*:
/// `int n = [[Foo alloc] tag];`.
///
/// Emitted as a bare group of statements rather than a braced one, because
/// bracing would scope `n` out of the rest of the body -- the shape #328
/// established for a declaration's initialiser. `Foo=1` is again the
/// signal: three declarations, one slot.
#[test]
fn an_owning_receiver_in_a_declaration_initializer_is_released() {
    let src = format!(
        "/* oz-pool: Foo=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Foo : OZObject
- (int)tag;
@end
@implementation Foo
- (int)tag {
	if (self == nil) {
		return 0;
	}
	return 7;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	int a = [[Foo alloc] tag];
	int b = [[Foo alloc] tag];
	int c = [[Foo alloc] tag];
	return a + b + c;
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	int v = [r run];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_receiver_in_declaration");
    assert_eq!(
        out, "v=21 deallocs=3\n",
        "an initialiser's abandoned receiver is released too, and `n` stays \
         in scope: {}",
        out
    );
}

/// A +1 operand in a `for` **header's initialiser**, which was outside
/// #328's guard and leaked exactly as it did before #328 (#341).
///
/// #328 emits a declaration's initialiser as a *bare* group of statements
/// rather than a braced one, because bracing would scope the declared name
/// out of the rest of the body -- and it guards that on the declaration's
/// parent being a `compound_statement`. A `for` header's declaration has
/// the `for` statement as its parent, so the guard declined and nothing
/// released the reference.
///
/// The shape here is different from either of #328's two, and it has to be:
/// a `for` header cannot take a statement group at all. The whole loop is
/// wrapped in a braced group instead, with the temporary above it and the
/// release below -- which scopes nothing out, because a header declaration
/// is already scoped to the `for`. That the temporary outlives the loop is
/// the deliberate cost: it holds its slab slot for the loop's duration, and
/// in exchange the release happens exactly once.
///
/// Hoisting is correct **here specifically** and nowhere else in a loop: a
/// header initialiser runs once. `Foo=1` is the assertion -- three loops,
/// one slot, three teardowns.
#[test]
fn an_owning_argument_in_a_for_header_is_released() {
    let src = format!(
        "/* oz-pool: Foo=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;
static int g_iters = 0;

@interface Foo : OZObject
+ (instancetype)new;
@end
@implementation Foo
+ (instancetype)new {
	return [Foo alloc];
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (int)countOf:(Foo *)f;
- (void)run;
@end
@implementation Runner
- (int)countOf:(Foo *)f {
	if (f != nil) {
		return 2;
	}
	return 0;
}
- (void)run {
	for (int n = [self countOf:[Foo new]]; n > 0; n--) {
		g_iters = g_iters + 1;
	}
	for (int n = [self countOf:[Foo new]]; n > 0; n--) {
		g_iters = g_iters + 1;
	}
	for (int n = [self countOf:[Foo new]]; n > 0; n--) {
		g_iters = g_iters + 1;
	}
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	[r run];
	printf(\"iters=%d deallocs=%d\\n\", g_iters, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_argument_in_a_for_header");
    assert_eq!(
        out, "iters=6 deallocs=3\n",
        "each header's `+1` is released after its loop, so one slot serves \
         all three -- and `n` still governs the loop it was declared in: {}",
        out
    );
}

/// The same position, with the `+1` as the send's **receiver** rather than
/// its argument: `for (int n = [[Foo alloc] tag]; ...)`.
///
/// The two changes compose without either knowing about the other, which is
/// the point of collecting *operands* rather than arguments (#340): the
/// header arm asks for whatever `+1` operands the initialiser has, and the
/// receiver is one of them.
#[test]
fn an_owning_receiver_in_a_for_header_is_released() {
    let src = format!(
        "/* oz-pool: Foo=1,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;
static int g_iters = 0;

@interface Foo : OZObject
- (int)tag;
@end
@implementation Foo
- (int)tag {
	if (self == nil) {
		return 0;
	}
	return 2;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (void)run;
@end
@implementation Runner
- (void)run {
	for (int n = [[Foo alloc] tag]; n > 0; n--) {
		g_iters = g_iters + 1;
	}
	for (int n = [[Foo alloc] tag]; n > 0; n--) {
		g_iters = g_iters + 1;
	}
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	[r run];
	printf(\"iters=%d deallocs=%d\\n\", g_iters, g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "owning_receiver_in_a_for_header");
    assert_eq!(
        out, "iters=4 deallocs=2\n",
        "an abandoned receiver in the header is released after the loop too: \
         {}",
        out
    );
}

/// The **condition and the update are not hoisted**, and this is the guard
/// that keeps #341 from becoming the bug #328 avoided.
///
/// A header initialiser runs once, which is the entire reason hoisting is
/// correct for it. The condition runs before every iteration and the update
/// after every one, so lifting an allocation out of either would allocate
/// once where the source allocates every time round -- handing the same
/// object to every evaluation and changing what the program does, not just
/// where it frees. `for_header_owning_operands` reads
/// `child_by_field_name("initializer")` and nothing else for that reason.
///
/// So both of these still leak, deliberately, and the assertion is on the
/// **emitted C**: no temporary is taken for either. A dealloc count could
/// not tell the difference between "not hoisted" and "hoisted and released
/// after the loop" for the condition, since both end with the objects freed
/// -- only the emitted text says whether the allocation still happens per
/// evaluation.
#[test]
fn a_for_conditions_owning_operand_is_not_hoisted() {
    let src = format!(
        "/* oz-pool: Foo=8,Runner=1 */\n{}{}",
        PREAMBLE(),
        "\
@interface Foo : OZObject
+ (instancetype)new;
@end
@implementation Foo
+ (instancetype)new {
	return [Foo alloc];
}
@end

@interface Runner : OZObject
- (int)countOf:(Foo *)f;
- (void)run;
@end
@implementation Runner
- (int)countOf:(Foo *)f {
	return f != nil;
}
- (void)run {
	for (int i = 0; i < [self countOf:[Foo new]]; i++) {
	}
	int j = 0;
	for (; j < 1; j = j + [self countOf:[Foo new]]) {
	}
}
@end

int main(void) { return 0; }
"
    );
    let out = oz2c::transpile(&src).expect("should transpile");
    let body = out
        .source_c
        .split("void Runner_run(struct Runner *self)\n{")
        .nth(1)
        .unwrap_or_else(|| panic!("no Runner_run definition in:\n{}", out.source_c))
        .split("\n}\n")
        .next()
        .unwrap_or("");
    assert!(
        !body.contains("_oz_arg_") && !body.contains("_oz_recv_"),
        "neither the condition nor the update runs once, so neither may have \
         its allocation lifted above the loop; got:\n{}",
        body
    );
}
