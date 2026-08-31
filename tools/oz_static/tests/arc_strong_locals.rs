// SPDX-License-Identifier: Apache-2.0
//
// arc_strong_locals.rs -- ARC on strong object *locals* (#234).
//
// oz_static already did retain-new/release-old for strong ivars
// (`emit::render_strong_ivar_assign`) and for properties (a synthesized
// setter). A plain local was the one strong storage class doing neither, so
// reassigning one abandoned whatever it held. That is why `staticbar` had to
// reject an ordinary loop that reassigns a local rather than emit it: pool
// sizing counts an allocation site once (`pools::count_sites`), which is
// sound only if each iteration's object dies before the next allocates.
//
// The releases these tests pin were checked against the Python pipeline,
// which has the same transform (`emit.py::_emit_strong_local_assign`) and
// emits the release in the same place -- release old, then assign, plus one
// scope-exit release. Two things it cannot do that oz_static now does are
// recorded in `bare_declaration_gets_arcs_implicit_nil` and
// `reassigned_local_needs_only_one_slab_slot` below.

mod common;
use common::{
    compile_and_run, expect_reject, ozarray_src, ozobject_src as PREAMBLE, ozq31_src,
};

/// A local reassigned in a loop needs exactly one slab slot, because the
/// release happens *before* the next allocation -- the slot goes back to the
/// slab and the very next `alloc` can take it again.
///
/// This is the shape #234 was filed about, and the pool directive is what
/// makes the test mean something: with `Counter=1`, 100 successful
/// allocations are only possible if each one released the previous. Without
/// the release the second iteration would get nil from an exhausted slab and
/// `ok` would be 1.
///
/// The Python pipeline rejects this source outright -- `OZ004: allocation
/// count for 'Counter' cannot be determined statically ... Pass an explicit
/// override via --pool-sizes` -- even though its own generated C releases
/// before allocating and so would also run correctly on one slot. Its sizing
/// check is inconsistent with its own ARC; matching that would be a
/// regression dressed as parity.
#[test]
fn reassigned_local_needs_only_one_slab_slot() {
    // `Runner` is a separate class on purpose: the driver instance must not
    // come out of the pool under test, or `Counter=1` would be spent before
    // the loop starts and the test would fail for a reason unrelated to ARC.
    let src = format!(
        "/* oz-pool: Counter=1 */\n{}{}",
        PREAMBLE(),
        "\
@interface Counter : OZObject {
	int _n;
}
@end
@implementation Counter
@end

@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	Counter *c;
	int ok = 0;
	for (int i = 0; i < 100; i++) {
		c = [Counter alloc];
		if (c) {
			ok = ok + 1;
		}
	}
	return ok;
}
@end

#include <stdio.h>
int main(void) {
	Runner *driver = [Runner alloc];
	printf(\"ok=%d\\n\", [driver run]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "reassigned_local_needs_only_one_slab_slot");
    assert_eq!(stdout, "ok=100\n", "every iteration must reuse the one slot");
}

/// Each overwrite must actually run the previous object's `-dealloc`, not
/// merely decrement a count. 100 allocations through one variable means 99
/// deallocs from the overwrites plus 1 when the scope ends.
#[test]
fn each_overwrite_deallocates_the_previous_object() {
    let src = format!(
        "/* oz-pool: Counter=1 */\n{}{}",
        PREAMBLE(),
        "\
static int g_deallocs = 0;

@interface Counter : OZObject {
	int _n;
}
@end
@implementation Counter
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end

@interface Runner : OZObject
- (void)run;
@end
@implementation Runner
- (void)run {
	Counter *c;
	for (int i = 0; i < 100; i++) {
		c = [Counter alloc];
	}
}
@end

#include <stdio.h>
int main(void) {
	Runner *driver = [Runner alloc];
	[driver run];
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "each_overwrite_deallocates_the_previous_object");
    assert_eq!(
        stdout, "deallocs=100\n",
        "99 overwrites plus the scope-exit release of the last value"
    );
}

/// ARC zero-initializes a strong local, and here it is load-bearing rather
/// than tidy: the first assignment releases whatever the variable held, so
/// an indeterminate pointer would reach `oz_static_release`, which
/// dereferences it. Reading uninitialized memory is not something a test can
/// assert on directly, so this pins the generated text instead.
///
/// The Python pipeline cannot transpile this shape at all: under
/// `-fobjc-arc` Clang represents the implicit nil as an
/// `ImplicitValueInitExpr`, and `oz_transpile` has no emission rule for that
/// node -- `OZ003: unhandled AST node 'ImplicitValueInitExpr'`, even with an
/// explicit `--pool-sizes`. Verified directly, not inferred.
#[test]
fn bare_declaration_gets_arcs_implicit_nil() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Counter : OZObject {
	int _n;
}
- (void)run;
@end
@implementation Counter
- (void)run {
	Counter *c;
	c = [Counter alloc];
}
@end
int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("struct Counter *c = 0;"),
        "a strong local declared bare must get ARC's implicit nil; got:\n{}",
        out.source_c
    );
}

/// `Foo *f = nil;` means exactly what a bare `Foo *f;` means -- the variable
/// starts empty -- so both must get the same ARC treatment. Without this the
/// explicit spelling would silently lose release-on-overwrite while the
/// implicit one kept it, which is the worse way round: `= nil` is the form a
/// careful author writes, and the only form the Python pipeline can consume.
///
/// One slot and 100 iterations again, so this asserts the behaviour rather
/// than the generated text.
#[test]
fn explicit_nil_initializer_is_managed_like_a_bare_declaration() {
    let src = format!(
        "/* oz-pool: Counter=1 */\n{}{}",
        PREAMBLE(),
        "\
@interface Counter : OZObject {
	int _n;
}
@end
@implementation Counter
@end

@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	Counter *c = nil;
	int ok = 0;
	for (int i = 0; i < 100; i++) {
		c = [Counter alloc];
		if (c) {
			ok = ok + 1;
		}
	}
	return ok;
}
@end

#include <stdio.h>
int main(void) {
	Runner *driver = [Runner alloc];
	printf(\"ok=%d\\n\", [driver run]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "explicit_nil_initializer_is_managed_like_a_bare_declaration");
    assert_eq!(stdout, "ok=100\n", "`= nil` must be managed exactly like a bare declaration");
}

/// Self-assignment must not free the value being stored. The generated form
/// retains before releasing for exactly this reason -- the same ordering
/// `render_strong_ivar_assign` uses, and what makes `_x = _x` safe there.
#[test]
fn self_assignment_is_safe() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Counter : OZObject {
	int _n;
}
- (int)run;
@end
@implementation Counter
- (int)run {
	Counter *c;
	c = [Counter alloc];
	c = c;
	return c != 0 ? 1 : 0;
}
@end

#include <stdio.h>
int main(void) {
	Counter *driver = [Counter alloc];
	printf(\"alive=%d\\n\", [driver run]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "self_assignment_is_safe");
    assert_eq!(stdout, "alive=1\n");
}

/// ARC defers to manual retain/release: a local the body releases by hand is
/// left entirely to the body, because adding an automatic release to code
/// that already releases is a double free. oz_static supports manual memory
/// management as a feature of its own, so this is not a corner case.
#[test]
fn manual_release_suppresses_arc() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Counter : OZObject {
	int _n;
}
- (void)run;
@end
@implementation Counter
- (void)run {
	Counter *c;
	c = [Counter alloc];
	[c release];
}
@end
int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    // The *definition*, not the companion prototype that precedes it.
    let run = out
        .source_c
        .split("void Counter_run(struct Counter *self)\n{")
        .nth(1)
        .unwrap_or_else(|| panic!("no Counter_run definition in:\n{}", out.source_c))
        .split("\n}")
        .next()
        .unwrap_or("");
    assert!(
        !run.contains("_oz_prevloc") && !run.contains(", c = "),
        "a hand-released local must not also be ARC-managed; got:\n{}",
        run
    );
    // Exactly one release: the author's own.
    assert_eq!(
        run.matches("oz_static_release").count(),
        1,
        "expected only the hand-written release; got:\n{}",
        run
    );
}

/// A `+0` right-hand side that is not a plain identifier would need a
/// temporary to be retained exactly once, and a temporary cannot be placed
/// correctly inside a loop here (`ctx.pre_stmts` drains at the enclosing
/// *top-level* statement, so it would hoist above the `for` -- which is a
/// mistake this change actually made and had to be backed out of). Rather
/// than manage such a local half-way, which would let the scope-exit release
/// free a value nothing retained, it is not managed at all -- and then the
/// allocation rule still applies to it, because nothing bounds how many of
/// its objects are live.
///
/// The two halves of that are what this pins: `b` has one owning store in
/// the loop and one unsupported store after it, so it is *not* managed, and
/// the loop allocation is therefore still rejected.
#[test]
fn unsupported_store_shape_leaves_the_local_unmanaged_and_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Box : OZObject
- (Box *)peer;
@end
@implementation Box
- (Box *)peer { return 0; }
@end

@interface Counter : OZObject
- (void)run;
@end
@implementation Counter
- (void)run {
	Box *b;
	Box *seed = [Box alloc];
	for (int i = 0; i < 3; i++) {
		b = [Box alloc];
	}
	b = [seed peer];
}
@end
int main(void) { return 0; }
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("escapes the iteration"), "diagnostics: {}", diags);
}

/// The same boundedness, for a *collection literal*. This is what justifies
/// narrowing OZ-098's rule (see `static_bar_rejects`): a literal reassigned to
/// a strong local was rejected as escaping its iteration, and it does not
/// escape once ARC releases the previous one.
///
/// Both pools are pinned to their minimum so the claim cannot be satisfied by
/// slack: `OZArray=1` slot, and a 2-slot element pool for the two-element
/// literal. 100 successful iterations are only possible if freeing an OZArray
/// returns both its slab slot and its element buffer.
#[test]
fn reassigned_literal_needs_only_one_slot_and_one_buffer() {
    let src = format!(
        "/* oz-pool: OZArray=1, OZQ31=2 */\n/* oz-item-pool: 2 */\n{}{}{}{}",
        PREAMBLE(),
        ozq31_src(),
        ozarray_src(),
        "\
@interface Runner : OZObject
- (int)run;
@end
@implementation Runner
- (int)run {
	OZArray *arr;
	int ok = 0;
	for (int i = 0; i < 100; i++) {
		arr = @[@(1), @(2)];
		if (arr) {
			ok = ok + 1;
		}
	}
	return ok;
}
@end

#include <stdio.h>
int main(void) {
	Runner *r = [Runner alloc];
	printf(\"ok=%d\\n\", [r run]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "reassigned_literal_needs_only_one_slot_and_one_buffer");
    assert_eq!(
        stdout, "ok=100\n",
        "the array slot and its element buffer must both be reused"
    );
}
