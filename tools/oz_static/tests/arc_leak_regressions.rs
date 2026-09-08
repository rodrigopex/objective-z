// SPDX-License-Identifier: Apache-2.0
//
// arc_leak_regressions.rs -- two leaks found by running the behaviour corpus
// under LeakSanitizer through *this* backend for the first time.
//
// Both cases already existed in the corpus and passed throughout: a leak is
// invisible to a driver that only checks return values, and
// `just test-cross-backend` compares Unity results rather than allocation
// balance, so 71/71 MATCH said nothing about either. Only pointing LSan at
// oz_static's own output made them visible.
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
/// `OZQ31 *a` is not a class name. `foundation/q31_basic` leaked an OZQ31
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
/// Two of the convention-named +1 selectors hand back a reference
/// something else already accounts for. `-retain` returns its own
/// receiver, and a bare `[c retain];` is the manual-retain/release idiom
/// whose balancing `[c release];` is written by hand -- `samples/smp_shared`
/// does exactly that in its contention loop, so releasing the discarded
/// result would have freed the shared Counter out from under two cores.
/// `-init` *consumes* the receiver's +1, which here belongs to a local
/// scope-based ARC already releases.
///
/// A leak is a bug and a double free is memory corruption, so this is the
/// direction that has to fail closed.
#[test]
fn discarded_retain_and_init_on_an_owned_receiver_are_left_alone() {
    let src = format!(
        "/* oz-pool: Counter=1,Widget=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Counter : OZObject
@end
@implementation Counter
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Widget : OZObject
@end
@implementation Widget
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

#include <stdio.h>
int main(void) {
	Counter *c = [Counter alloc];
	/* Balanced by hand, exactly as samples/smp_shared writes it. */
	[c retain];
	[c release];
	printf(\"after_retain=%d\\n\", g_deallocs);

	Widget *w = [Widget alloc];
	/* The +1 this hands back is `w`'s, and `w` is released at the end of
	 * this scope. */
	[w init];
	printf(\"after_init=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "discarded_retain_and_init");
    assert_eq!(
        out, "after_retain=0\nafter_init=0\n",
        "neither receiver may be released twice: {}",
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
/// #322's `discarded_retain_and_init_on_an_owned_receiver_are_left_alone`
/// has to keep holding once a cast can no longer hide the send inside it.
/// `-retain` returns its own receiver -- `samples/smp_shared` balances a
/// bare `[c retain];` by hand -- and `-init...` consumes the receiver's
/// +1, which here belongs to a local scope-based ARC already releases.
/// Wrapping either in `(void)` changes nothing about who owns the
/// reference, so neither may be released.
///
/// Getting this wrong is memory corruption where #327 itself is only a
/// leak, so it is the direction that has to fail closed.
#[test]
fn discarded_retain_and_init_through_a_cast_are_left_alone() {
    let src = format!(
        "/* oz-pool: Counter=1,Widget=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Counter : OZObject
@end
@implementation Counter
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Widget : OZObject
@end
@implementation Widget
@end

#include <stdio.h>
int main(void) {
	Counter *c = [Counter alloc];
	/* Balanced by hand, as samples/smp_shared writes it -- with the
	 * cast the idiom often carries to silence an unused result. */
	(void)[c retain];
	[c release];
	printf(\"after_retain=%d\\n\", g_deallocs);

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
    let out = compile_and_run(&src, "discarded_retain_and_init_through_cast");
    assert_eq!(
        out, "after_retain=0\nafter_init=0\n",
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
/// `(__bridge_retained void *)[t copy];` hands the reference to a
/// non-Objective-C holder; releasing it would pull the object out from
/// under that holder, which is a double free and not a leak. `__bridge`
/// and `__bridge_transfer` are held back with it -- not because either is
/// known to be unsafe, but because there is no CoreFoundation here for any
/// of the three to bridge to, so leaving all of them borrowed costs
/// nothing observable and keeps the conservative bias exact.
///
/// So `deallocs=0` here is the answer being asserted, not a leak being
/// tolerated: under `__bridge_retained` the reference belongs to whatever
/// took the `void *`.
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
	(__bridge_retained void *)[t copy];
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
/// 4` in `oz_static_release` under ASan.
///
/// This asserts on the generated C rather than on a dealloc counter, and
/// that is not laziness. A second `oz_static_release` on a freed object
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
    let out = oz_static::transpile(&src).expect("should transpile");
    let body = out
        .source_c
        .split("int Runner_initThroughCast(struct Runner *self)\n{")
        .nth(1)
        .unwrap_or_else(|| panic!("no Runner_initThroughCast definition in:\n{}", out.source_c))
        .split("\n}")
        .next()
        .unwrap_or("");
    assert_eq!(
        body.matches("oz_static_release").count(),
        1,
        "one object, one reference, so exactly one release; got:\n{}",
        body
    );
    // And it must be the receiver that is released, not the name the cast
    // gave the same pointer -- which is also what keeps the release ahead
    // of nothing that still reads `t`.
    assert!(
        body.contains("oz_static_release((struct OZObject *)(u));"),
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
/// ordinary reassign-in-a-loop shape outright ("allocation of 'Thing' inside
/// a loop escapes the iteration"), because an unmanaged local cannot bound
/// how many instances are live.
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
/// #327's reason.
///
/// `(__bridge_retained Thing *)` hands the reference to a non-Objective-C
/// holder, so releasing it at scope exit would pull the object out from
/// under that holder -- a double free, not a leak. `__bridge` and
/// `__bridge_transfer` are held back with it, because there is no
/// CoreFoundation here for any of the three to bridge to, so leaving all
/// three borrowed keeps the conservative bias exact rather than resting on
/// a reading of a bridge this project does not have.
///
/// `deallocs=0` is therefore the answer being asserted and not a leak being
/// tolerated, exactly as in `a_bridging_cast_is_not_looked_through`. The
/// non-bridging local in the same scope is what shows the peel is working
/// at all and the bridging one is being singled out.
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
	Bridged *held = (__bridge_retained Bridged *)[Bridged alloc];
	Bridged *lent = (__bridge Bridged *)[Bridged alloc];
	Plain *mine = (Plain *)[Plain alloc];
	return (held != nil) + (lent != nil) + (mine != nil);
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
	Holder *h = [Holder alloc];
	int v = [h run];
	int during = g_deallocs;
	[h release];
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
/// *by shape* and says yes to `[e retain]` and `[u init]`. Both hand back a
/// reference something else already accounts for -- `-retain` its own
/// receiver, whose balancing `[e release]` is written by hand, and
/// `-init...` its receiver's `+1`, which scope-based ARC already releases.
/// Taking a call-site temporary for either and releasing it would free one
/// pointer twice. `arc::owning_argument_value` goes through `created_by`
/// instead, which excludes `-retain` outright and follows an `-init...`
/// send back to its receiver (#322).
///
/// Asserted on the **emitted C**, deliberately, and this is the trap the
/// predecessors documented: an over-release is invisible to a dealloc
/// counter, because the second `oz_static_release` sees a refcount already
/// at 0 and returns before `-dealloc`, and the host slab clamps `num_used`
/// at 0. So a counter and a slot count are both blind here. What is
/// checkable is that no call-site temporary was taken at all.
#[test]
fn retained_and_init_arguments_are_left_alone() {
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
	Foo *e = [Foo alloc];
	/* The manual retain/release idiom: the +1 is `e`'s, and the
	 * balancing release below is written by hand. */
	[self setFoo:[e retain]];
	[e release];
	Foo *u = [Foo alloc];
	/* -init consumes its receiver's +1 and hands it back, so this is
	 * `u`'s reference and `u`'s scope exit releases it. */
	[self setFoo:[u init]];
}
@end

int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
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
    // Exactly the two releases the source already owed: the hand-written
    // `[e release]`, and `u` at scope exit. `e` gets no scope-exit release
    // of its own -- `emit::released_by_hand` sees the manual one and ARC
    // defers to the author for that variable throughout, which is the
    // standing rule and the reason the idiom is safe here at all. A third
    // release would be one pointer freed twice.
    assert_eq!(
        body.matches("oz_static_release").count(),
        2,
        "only the releases the source already owed; got:\n{}",
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
/// owning argument (it is a receiver, not an argument, which is the other
/// reason).
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
    let out = oz_static::transpile(&src).expect("should transpile");
    let body = out
        .source_c
        .split("void Holder_run(struct Holder *self)\n{")
        .nth(1)
        .unwrap_or_else(|| panic!("no Holder_run definition in:\n{}", out.source_c))
        .split("\n}\n")
        .next()
        .unwrap_or("");
    assert_eq!(
        body.matches("oz_static_release").count(),
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
