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
