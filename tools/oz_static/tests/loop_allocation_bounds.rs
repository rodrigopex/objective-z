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
//   | destination                  | reused | released first | slots |
//   | ---                          | ---    | ---            | ---   |
//   | managed local                | yes    | yes            | 1     |
//   | ivar / file-scope variable   | yes    | **no**         | **2** |
//   | array element, varying index | **no** | n/a            | loop  |
//
// The ivar overlap is inherent rather than a defect elsewhere: the new
// value must be evaluated before the old is released, or
// `_ivar = [_ivar retain]` would free a live object.
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

/// An ivar store: reused, but released **after** the new value exists, so
/// two are briefly live and one slot is not enough.
///
/// Refused now for **both** spellings. The `alloc` spelling was already
/// refused; the factory spelling was accepted and produced
/// `arr[0] = object` followed by nils, which is the silent failure this
/// case exists to prevent. The message has to name the pool, because
/// raising it is the fix.
#[test]
fn an_ivar_store_names_the_two_slot_overlap() {
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
