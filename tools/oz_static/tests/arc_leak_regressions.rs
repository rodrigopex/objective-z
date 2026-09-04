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
use common::{compile_and_run, ozobject_src as PREAMBLE};

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
