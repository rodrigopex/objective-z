// SPDX-License-Identifier: Apache-2.0
//
// loop_allocation_bounds.rs -- which allocations inside a loop one slab
// slot can serve, and which need saying so (#345).
//
// The rule this covers used to ask three questions that were all proxies
// for one: is the selector literally `alloc`, is it bound to a fresh
// per-iteration local, is it stored into an ARC-managed one. The real
// question is whether the reference outlives the iteration -- and if it is
// kept, whether the previous one is released *before* the next is
// allocated.
//
// Measured on a one-slot pool, four iterations each, which is what the
// cases below assert:
//
//   | destination                       | reused | released first | slots |
//   | ---                               | ---    | ---            | ---   |
//   | managed local                     | yes    | yes            | 1     |
//   | ivar / global, store cannot read it | yes  | yes            | 1     |
//   | ivar / global, store reads it     | yes    | **no**         | **2** |
//   | array element, varying index      | **no** | n/a            | loop  |
//
// The ivar overlap was called inherent here, on the grounds that the new
// value must be evaluated before the old is released or
// `_ivar = [_ivar retain]` would free a live object. That holds only for a
// store that *reads* the ivar. `_ivar = [Foo make]` does not, and #405
// gave the ivar path the release-first shape locals had since #234, so two
// of the three now need one slot. `staticbar::overlapping_unless_released_first`
// asks `emit::classify_store` which it is, so the bar and the emitter
// cannot drift: refusing a release-first store over-rejects, and accepting
// a temporary-hoisting one miscompiles, because a loop lifts that
// temporary out of itself.
//
// The old rule was wrong in **both** directions, and each direction has
// cases here. It refused the operand positions, which the emitter releases
// inside the iteration; and because it keyed on the selector name, it let
// every other way of producing a `+1` straight past -- so the same
// accumulation spelled through a factory was accepted and silently yielded
// `nil` from the second iteration on.

mod common;
use common::{compile_and_run_strict, expect_reject, ozobject_src};

/// `+make` is the spelling the old rule could not see: the allocation is
/// inside the factory, so `selector == "alloc"` never matched at the call
/// site.
const PRELUDE: &str = "\
#include <stdio.h>

@interface Foo : OZObject
+ (Foo *)make;
- (int)tag;
@end
@implementation Foo
+ (Foo *)make
{
	return [[Foo alloc] init];
}
- (int)tag
{
	return 1;
}
@end
";

fn program(body: &str) -> String {
    format!("/* oz-pool: Foo=1 */\n{}{}\n{}", ozobject_src(), PRELUDE, body)
}

/// The four operand positions, all newly accepted, all on **one slot**.
///
/// This is #345 as filed: `[[Foo alloc] poke]` was refused although
/// nothing escaped, because the reference is held in a temporary and
/// released after the send, inside the loop body. A receiver, an argument,
/// a discarded result and a controlling expression are all that shape.
///
/// Sixteen allocations through one slab slot: every `tag` printing `1`
/// rather than `0` is the claim, since `[nil tag]` is 0.
#[test]
fn every_operand_position_is_bounded_by_one_slot() {
    let src = program(
        "\
void takes(Foo *f)
{
	printf(\"arg %d\\n\", [f tag]);
}

int main(void)
{
	int i;

	for (i = 0; i < 4; i++) {
		printf(\"recv %d\\n\", [[Foo alloc] tag]);
	}
	for (i = 0; i < 4; i++) {
		takes([[Foo alloc] init]);
	}
	for (i = 0; i < 4; i++) {
		[[Foo alloc] init];
	}
	for (i = 0; i < 4; i++) {
		if ([[Foo alloc] tag] > 100) {
			printf(\"unreachable\\n\");
		}
	}
	printf(\"done\\n\");
	return 0;
}
",
    );
    let out = compile_and_run_strict(&src, "loopbound_operands");
    assert_eq!(out.matches("recv 1").count(), 4, "got:\n{}", out);
    assert_eq!(out.matches("arg 1").count(), 4, "got:\n{}", out);
    assert!(out.ends_with("done\n"), "got:\n{}", out);
    assert!(
        !out.contains("recv 0") && !out.contains("arg 0"),
        "a 0 means an allocation found no free slot, so the release is not \
         happening inside the iteration; got:\n{}",
        out
    );
}

/// A managed local: reused, and released **before** the next allocation,
/// so one slot serves the loop.
///
/// The half of the old rule that was right, kept as a case because the new
/// predicate has to keep agreeing with it -- and because it is the
/// measured contrast with the ivar case below, which differs only in
/// ordering.
#[test]
fn a_managed_local_is_bounded_by_one_slot() {
    let src = program(
        "\
int main(void)
{
	Foo *f;
	int i;

	for (i = 0; i < 4; i++) {
		f = [Foo make];
		printf(\"i=%d tag=%d\\n\", i, [f tag]);
	}
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "loopbound_local"),
        "i=0 tag=1\ni=1 tag=1\ni=2 tag=1\ni=3 tag=1\n",
        "a managed local's previous value is released before the next allocation, so one \
         slot serves every iteration"
    );
}

/// An ivar store whose right-hand side cannot read the ivar: released
/// first, so one slot serves every iteration.
///
/// This was refused until #405 -- the emitter evaluated the new value
/// before releasing the old for *any* ivar store, so the shape really did
/// need two slots and the bar was right to refuse it. Both halves changed
/// together: `render_strong_ivar_assign` now releases first here, and the
/// bar asks it rather than assuming. `[Foo make]` is the factory spelling
/// on purpose, since that is the one the old selector-name rule could not
/// see at all.
#[test]
fn an_ivar_store_released_first_needs_only_one_slot() {
    let src = program(
        "\
@interface Holder : OZObject {
	Foo *_ivar;
}
- (void)run;
@end
@implementation Holder
- (void)run
{
	int i;

	for (i = 0; i < 4; i++) {
		_ivar = [Foo make];
		printf(\"i=%d tag=%d\\n\", i, [_ivar tag]);
	}
}
@end

int main(void)
{
	Holder *h = [[Holder alloc] init];
	[h run];
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "loopbound_ivar_released_first"),
        "i=0 tag=1\ni=1 tag=1\ni=2 tag=1\ni=3 tag=1\n",
        "the store releases the previous Foo before allocating the next, so the one slot \
         is free again every iteration -- a nil would print tag=0"
    );
}

/// An ivar store whose right-hand side *does* read the ivar: the new value
/// has to exist before the old one can go, so two are briefly live and one
/// slot is not enough. Still refused, and the message still has to name the
/// pool, because raising it is the fix.
///
/// The ternary is what puts this shape in `LocalStore::Unsupported`: the
/// `+1` is real, but the store can read `_ivar`, so the emitter keeps the
/// hoisted temporary. Accepting it would do more than exhaust the pool --
/// that temporary goes through `ctx.pre_stmts`, which a loop lifts out of
/// the loop entirely, so it would read the ivar once while still nil and
/// release nil on every iteration.
#[test]
fn an_ivar_store_that_reads_the_ivar_names_the_two_slot_overlap() {
    let src = program(
        "\
@interface Holder : OZObject {
	Foo *_ivar;
}
- (void)run;
@end
@implementation Holder
- (void)run
{
	int i;

	for (i = 0; i < 4; i++) {
		_ivar = i > 0 ? [Foo make] : _ivar;
	}
}
@end

int main(void) { return 0; }
",
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("an ivar") && diags.contains("two"),
        "the diagnostic must name the destination and the two-slot overlap; got:\n{}",
        diags
    );
    assert!(
        diags.contains("oz-pool"),
        "and how to fix it, since the shape is bounded -- just not at one; got:\n{}",
        diags
    );
}

/// The under-rejection, and the reason the trigger moved off the selector
/// name: an array element chosen per iteration accumulates, and the
/// allocation being inside `+make` is no excuse.
///
/// Before this change the same program built, ran, and printed
/// `arr[0] = object` then three nils.
#[test]
fn a_factory_accumulating_into_an_array_is_refused() {
    let src = program(
        "\
@interface Holder : OZObject {
	Foo *_arr[4];
}
- (void)fill;
@end
@implementation Holder
- (void)fill
{
	int i;

	for (i = 0; i < 4; i++) {
		_arr[i] = [Foo make];
	}
}
@end

int main(void) { return 0; }
",
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("an array element chosen per iteration"),
        "a factory's +1 accumulating into a varying element must be refused, and the \
         allocation being inside the factory is exactly why the old selector-name test \
         missed it; got:\n{}",
        diags
    );
}

/// A *constant* index names the same element every iteration, so it
/// behaves like an ivar -- two slots, not unbounded. Worth separating,
/// because a rule that called every subscript unbounded would say
/// something false about this one.
#[test]
fn a_constant_index_overlaps_rather_than_accumulates() {
    let src = program(
        "\
@interface Holder : OZObject {
	Foo *_arr[4];
}
- (void)fill;
@end
@implementation Holder
- (void)fill
{
	int i;

	for (i = 0; i < 4; i++) {
		_arr[0] = [Foo make];
	}
}
@end

int main(void) { return 0; }
",
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("one element of an array ivar") && diags.contains("two"),
        "a constant index is the same slot each time, so this overlaps at two rather than \
         accumulating; got:\n{}",
        diags
    );
}

/// Returning it from inside the loop: the iteration does not end its life
/// at all.
#[test]
fn returning_from_inside_the_loop_is_refused() {
    let src = program(
        "\
@interface Holder : OZObject
- (Foo *)first;
@end
@implementation Holder
- (Foo *)first
{
	int i;

	for (i = 0; i < 4; i++) {
		return [Foo make];
	}
	return 0;
}
@end

int main(void) { return 0; }
",
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("is returned"),
        "a returned allocation outlives the iteration; got:\n{}",
        diags
    );
}

/// One allocation, one diagnostic: `[[Foo alloc] init]` is a single
/// object, because `-init` consumes its receiver's `+1` and hands it back.
///
/// Reported at the outer send, so the escape walk starts from the
/// expression that is actually stored. Without that, an accumulating
/// `_arr[i] = [[Foo alloc] init];` produced two messages for one mistake.
#[test]
fn an_alloc_init_pair_reports_once() {
    let src = program(
        "\
@interface Holder : OZObject {
	Foo *_arr[4];
}
- (void)fill;
@end
@implementation Holder
- (void)fill
{
	int i;

	for (i = 0; i < 4; i++) {
		_arr[i] = [[Foo alloc] init];
	}
}
@end

int main(void) { return 0; }
",
    );
    let diags = expect_reject(&src);
    assert_eq!(
        diags.matches("inside a loop is stored into").count(),
        1,
        "one allocation must produce one diagnostic; got:\n{}",
        diags
    );
}
