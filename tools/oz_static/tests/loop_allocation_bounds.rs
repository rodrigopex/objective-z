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
//   | destination                          | reused | released first | slots |
//   | ---                                  | ---    | ---            | ---   |
//   | managed local                        | yes    | yes            | 1     |
//   | ivar / global, store cannot read it   | yes   | yes            | 1     |
//   | ivar / global, store reads it        | yes    | **no**         | **2** |
//   | array element, const index, cannot read it | yes | yes       | 1     |
//   | array element, const index, reads it | yes    | **no**         | **2** |
//   | array element, varying index         | **no** | n/a            | loop  |
//   | ivar that is not an owned slot       | yes    | n/a, no release | loop |
//
// The ivar overlap was called inherent here, on the grounds that the new
// value must be evaluated before the old is released or a store whose
// right-hand side reads the ivar (`_ivar = [_ivar itself]`) would free a
// live object -- an illustration that used to be spelled
// `_ivar = [_ivar retain]`, which #428 made a located error. That holds
// only for a
// store that *reads* the ivar. `_ivar = [Foo make]` does not, and #405
// gave the ivar path the release-first shape locals had since #234, so two
// of the three now need one slot. `staticbar::overlapping_unless_released_first`
// asks `emit::classify_store` which it is, so the bar and the emitter
// cannot drift: refusing a release-first store over-rejects, and accepting
// a store lowered through a temporary hands the second allocation a full
// slab, because the previous object is still live while the new one is
// being made.
//
// That second half used to say such a store "miscompiles, because a loop
// lifts that temporary out of itself", and it did until #424: the lowering
// pushed the temporary's initialiser through `ctx.pre_stmts`, which a loop
// lifts above itself. The shared lowering now pushes a bare declaration
// and assigns inside the comma expression, so the refusal is about slab
// capacity and nothing else. Measured with the predicate relaxed: the
// refused shape runs correctly on a pool of two -- five allocations, five
// frees, no nil.
//
// #405 reached two of the three destination spellings. The subscript one
// kept answering `OverlappingStore` unconditionally, so an array-element
// store the emitter already lowered release-first stayed refused on a pool
// it fits -- and the assertion in this file that said so was itself the
// defect, which is why the case that used to claim "two slab slots" for a
// constant index now runs four iterations on one instead (#423). The same
// change found the spelling that was wrong in the *other* direction:
// `self->_ivar = [_ivar dup]` was accepted although the emitter lowers it
// through a temporary, because the destination extractor answered `self`.
// All three spellings now go through `staticbar::assigned_slot_name`. When
// that was found it miscompiled outright, for the #424 reason above; it
// now yields a nil from the second iteration instead, which is the same
// gap with a milder symptom and the same fix.
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

int g_freed = 0;

@interface Foo : OZObject
+ (Foo *)make;
- (Foo *)dup;
- (int)tag;
@end
@implementation Foo
+ (Foo *)make
{
	return [[Foo alloc] init];
}
/* A `+1` whose receiver is the thing being overwritten, which is what puts
 * a store in `LocalStore::Unsupported`: the new value cannot exist until
 * the old one has been read, so the old one cannot be released first. */
- (Foo *)dup
{
	return [[Foo alloc] init];
}
- (int)tag
{
	return 1;
}
- (void)dealloc
{
	g_freed++;
	[super dealloc];
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
/// slot is not enough. Still refused -- and the message must **not** name
/// the pool, because raising it is not the fix.
///
/// This assertion is the other half of #425, and it used to say the
/// opposite: "and how to fix it, since the shape is bounded -- just not at
/// one", asserting `oz-pool` appears in the diagnostic. It does not fix it.
/// `staticbar` never reads `PoolSizes`, so it cannot know the directive was
/// added, and the rejection stands unchanged at `Foo=1`, `Foo=2` and
/// `Foo=8` -- measured. The test asserting the advice was present is what
/// pinned it in place.
///
/// The ternary is what puts this shape in `LocalStore::Unsupported`: the
/// `+1` is real, but the store can read `_ivar`, so the emitter lowers it
/// through a temporary and the previous object is still live while the new
/// one is allocated. On one slot the second allocation gets a full slab.
///
/// Until #424 accepting it would have done more than exhaust the pool: the
/// temporary's initialiser went through `ctx.pre_stmts`, which a loop lifts
/// out of the loop entirely, so it read the ivar once while still nil and
/// released nil every iteration. The shared lowering now pushes a bare
/// declaration and assigns inside the comma expression, so the only reason
/// left is capacity -- and that reason is sufficient, which is why this
/// case is unchanged.
///
/// That history matters to what the message may claim. While the temporary
/// was hoisted, pool-awareness was *unsound* for this arm -- a bigger pool
/// could not move the temporary back inside the loop. After #424 it is
/// sound and merely unimplemented: measured, this shape runs correctly on a
/// pool of two. So the diagnostic must not say the shape would be wrong at
/// any pool size. What it says instead is the narrower fact that holds
/// either way -- raising the pool does not lift *this rejection*, because
/// the check never reads `PoolSizes`.
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
        !diags.contains("oz-pool") && !diags.contains("--pool-sizes"),
        "and must not suggest raising the pool, which cannot lift this rejection (#425); \
         got:\n{}",
        diags
    );
    assert!(
        diags.contains("local declared before the loop"),
        "the advice that does work has to still be there; got:\n{}",
        diags
    );
}

/// The rejection does not move when the pool is raised, which is the whole
/// of #425: the advice was to do something that changes nothing.
///
/// Measured rather than argued, because "the check never reads
/// `PoolSizes`" is a claim about code and this is a claim about behaviour.
/// The same program at `Foo=1`, `Foo=2` and `Foo=8` is refused identically,
/// so a diagnostic naming the pool sends the author round a loop of their
/// own.
#[test]
fn raising_the_pool_does_not_lift_an_overlapping_store() {
    let churn = "\
@interface Holder : OZObject {
	Foo *_thing;
}
- (void)churn;
@end
@implementation Holder
- (void)churn
{
	int i;

	for (i = 0; i < 4; i++) {
		_thing = [_thing dup];
	}
}
@end

int main(void) { return 0; }
";
    let mut seen = Vec::new();
    for pool in ["1", "2", "8"] {
        let src = format!(
            "/* oz-pool: Foo={} */\n{}{}\n{}",
            pool,
            ozobject_src(),
            PRELUDE,
            churn
        );
        let diags = expect_reject(&src);
        assert!(
            diags.contains("needs **two** slab slots"),
            "pool={} must still be refused for the same reason; got:\n{}",
            pool,
            diags
        );
        assert!(
            !diags.contains("oz-pool") && !diags.contains("--pool-sizes"),
            "pool={} must not be told to raise the pool it already raised; got:\n{}",
            pool,
            diags
        );
        seen.push(diags);
    }
    assert_eq!(
        seen[0], seen[1],
        "the diagnostic is identical at Foo=1 and Foo=2, which is why advising a raise was \
         advice to do nothing"
    );
    assert_eq!(seen[1], seen[2], "and identical again at Foo=8");
}

/// No diagnostic in this family may recommend the pool, for any of the
/// three escapes.
///
/// A standing guard rather than three separate assertions: `Accumulates`
/// carried the same false remedy for its own reason -- the loop's trip
/// count is not a number this pass knows, so there was no pool size to
/// name -- and `Returned` never had one. Anything added here later has to
/// answer the same question, which is what a table-shaped test is for.
#[test]
fn no_loop_escape_recommends_raising_the_pool() {
    let bodies: &[(&str, &str)] = &[
        (
            "OverlappingStore",
            "\
@interface Holder : OZObject { Foo *_thing; }
- (void)go;
@end
@implementation Holder
- (void)go
{
	int i;
	for (i = 0; i < 4; i++) { _thing = [_thing dup]; }
}
@end
int main(void) { return 0; }
",
        ),
        (
            "Accumulates",
            "\
@interface Holder : OZObject { Foo *_arr[4]; }
- (void)go;
@end
@implementation Holder
- (void)go
{
	int i;
	for (i = 0; i < 4; i++) { _arr[i] = [Foo make]; }
}
@end
int main(void) { return 0; }
",
        ),
        (
            "Returned",
            "\
@interface Holder : OZObject
- (Foo *)go;
@end
@implementation Holder
- (Foo *)go
{
	int i;
	for (i = 0; i < 4; i++) { return [Foo make]; }
	return 0;
}
@end
int main(void) { return 0; }
",
        ),
    ];
    for (escape, body) in bodies {
        let diags = expect_reject(&program(body));
        assert!(
            !diags.contains("oz-pool")
                && !diags.contains("--pool-sizes")
                && !diags.contains("size the pool"),
            "{} still recommends the pool, which this check cannot read (#425); got:\n{}",
            escape,
            diags
        );
        /* And must not advise anything ARC forbids the author writing.
         * `Accumulates` briefly said "release each instance before the
         * next iteration allocates" -- unwritable, since ARC is always on
         * and `[x release]` is a Clang error under `-fobjc-arc`, so such a
         * source never reaches oz2c. Replacing an unactionable remedy with
         * an unwritable one is the same defect twice. */
        assert!(
            !diags.contains("release each")
                && !diags.contains("release it")
                && !diags.contains("release the instance"),
            "{} advises calling release, which ARC forbids the author writing (#425); got:\n{}",
            escape,
            diags
        );
    }
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

/// A *constant* index names the same element every iteration, and the
/// store is lowered release-first when the right-hand side cannot read the
/// element -- so **one** slot serves the loop.
///
/// **This inverts what this file asserted when #405 merged.** The case
/// below is the one that used to be here, unchanged except for its
/// conclusion: it asserted the refusal was correct, naming "two slab
/// slots". It was not. `render_strong_array_element_assign` has emitted
/// `(release(self->_arr[0]), self->_arr[0] = <new>)` for a `+1` that does
/// not read the element since #405, exactly as the ivar path does, so the
/// element is empty again before the next allocation asks the slab for a
/// slot. #405 routed the `identifier` and `field_expression` spellings
/// through `staticbar::overlapping_unless_released_first` and left this
/// third one answering `OverlappingStore` unconditionally, which is the
/// whole of #423.
///
/// Measured rather than asserted to transpile, and measured the way
/// `an_ivar_store_released_first_needs_only_one_slot` measures it: four
/// iterations on a pool of **one**, every `tag` printing `1`. A `0` is
/// `[nil tag]`, which is what a slab with no free slot hands back. The
/// `freed` count is the other half of the claim -- each iteration's object
/// is really released, not merely leaked into a slot that happened to be
/// reused -- and the trailing `nil` store drops the last one, so four
/// allocations produce four frees.
#[test]
fn a_constant_index_store_released_first_needs_only_one_slot() {
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
		printf(\"i=%d tag=%d\\n\", i, [_arr[0] tag]);
	}
	_arr[0] = nil;
	printf(\"freed=%d\\n\", g_freed);
}
@end

int main(void)
{
	Holder *h = [[Holder alloc] init];
	[h fill];
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "loopbound_array_const_released_first"),
        "i=0 tag=1\ni=1 tag=1\ni=2 tag=1\ni=3 tag=1\nfreed=4\n",
        "the store releases the previous element before allocating the next, so the one slot \
         is free again every iteration -- a tag=0 would be the nil a full slab returns, and a \
         freed count below 4 would mean an iteration's object was never released"
    );
}

/// The same constant index through the **explicit** spelling. Both reach
/// one predicate, which is the point of `assigned_slot_name`: `#360` had
/// already found that `self->_arr[i]` fell through a rule written for
/// `_arr[i]`, and a fix that accepted only the bare spelling would have
/// left the same asymmetry one layer up.
///
/// The store is `self->_arr[0]`; the read-back is `_arr[0]`, because
/// `self->_arr[0]` in *receiver* position is a separate and still-standing
/// limitation -- oz2c cannot type it ("cannot statically resolve the
/// receiver type ... (receiver type is 'id')") and refuses it. That is
/// orthogonal to this store and is why the two halves are spelled
/// differently here rather than the case being weakened to a
/// transpile-only check.
#[test]
fn the_self_arrow_array_spelling_is_bounded_the_same_way() {
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
		self->_arr[0] = [Foo make];
		printf(\"i=%d tag=%d\\n\", i, [_arr[0] tag]);
	}
	self->_arr[0] = nil;
	printf(\"freed=%d\\n\", g_freed);
}
@end

int main(void)
{
	Holder *h = [[Holder alloc] init];
	[h fill];
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "loopbound_array_const_self_arrow"),
        "i=0 tag=1\ni=1 tag=1\ni=2 tag=1\ni=3 tag=1\nfreed=4\n",
        "`self->_arr[0]` and `_arr[0]` are the same slot and must be bounded identically"
    );
}

/// The one array shape that must **stay** refused: a store that *reads*
/// the element.
///
/// `classify_store` puts this in `LocalStore::Unsupported`, and
/// `render_strong_array_element_assign` makes that a located error rather
/// than lowering it at all -- so unlike the ivar and local slots, the array
/// element has no temporary of any kind here, and #424's shared lowering
/// (`emit::render_overlapping_strong_store`) deliberately did not reach
/// this path. Refusing it at the bar as well is what keeps the message
/// about the loop rather than about the store, and what stops a later
/// relaxation of the emitter from silently making the shape reachable.
#[test]
fn a_constant_index_store_that_reads_the_element_stays_refused() {
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
		_arr[0] = [_arr[0] dup];
	}
}
@end

int main(void) { return 0; }
",
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("one element of an array ivar") && diags.contains("two"),
        "a store that reads the element cannot release first, so this one really does need \
         two slots and must not be accepted along with the shapes that need one; got:\n{}",
        diags
    );
}

/// The fourth site, and the reason #423 is not only an over-rejection
/// being lifted.
///
/// `self->_ivar = [_ivar dup]` was **accepted**. The bar extracted the
/// store's destination with `find_last_identifier`, which answers `self`
/// for `self->_ivar` -- the field is a `field_identifier`, not an
/// `identifier` -- and then asked `classify_store` whether the right-hand
/// side mentions `self`. It does not, so the shape looked release-first
/// while `emit::assigned_ivar_name` keyed the actual store on `_ivar` and
/// lowered it through a temporary, which keeps the previous object live
/// across the new one's allocation. On this file's one-slot pool the second
/// iteration's allocation therefore finds a full slab.
///
/// It was worse than that when found, and the history is why this case
/// exists rather than being folded into the one above. The lowering then
/// put the temporary's *initialiser* in `ctx.pre_stmts`, so a loop lifted
/// it above itself:
///
/// ```c
/// struct OZObject *_oz_prev_L381_C3_1 = (struct OZObject *)(self->_ivar);
/// for (i = 0; i < 4; i++) {
///         (self->_ivar = Foo_dup(...), oz_static_release(_oz_prev_L381_C3_1));
/// }
/// ```
///
/// -- captured once while the ivar was still nil, then released again on
/// every iteration: a miscompile, not an exhausted pool. #424 replaced that
/// lowering with `emit::render_overlapping_strong_store`, which pushes a
/// bare declaration through `ctx.pre_stmts` and assigns inside the comma
/// expression, so a loop now lifts something that evaluates nothing.
///
/// **The gap this case guards is unchanged by that.** What the bar must not
/// do is call a store release-first when the emitter lowers it through a
/// temporary, because the two then disagree about how many slots the shape
/// needs. Only the symptom of getting it wrong moved -- from a stale
/// release to a nil from the second iteration -- and a silent nil is
/// precisely what this rule exists to prevent. `assigned_slot_name` is what
/// closes it, and it is the same function the subscript arm above uses, so
/// there is no fifth spelling to find.
#[test]
fn the_self_arrow_ivar_spelling_cannot_hide_a_store_that_reads_the_ivar() {
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
		self->_ivar = [_ivar dup];
	}
}
@end

int main(void) { return 0; }
",
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("an ivar") && diags.contains("two"),
        "the bare right-hand side hides nothing: `self->_ivar` and `_ivar` name one slot, and \
         a store that reads it needs two; got:\n{}",
        diags
    );
}

/// An ivar the emitter does **not** manage as a strong slot: nothing
/// releases the previous value, so the loop accumulates and no pool size
/// bounds it.
///
/// `scope.class_ivars` is every ivar; `render_strong_ivar_assign` and
/// `render_strong_array_element_assign` both gate on
/// `Program::owned_object_ivar_names`, which is fewer -- releasing a borrow
/// is the double free `__unsafe_unretained` exists to prevent, so they
/// decline the slot and the store lowers to a plain C one. The bar was
/// asking the wider question, so since #405 this was accepted with
/// `OverlappingStore`'s reasoning: "raise the pool and both live copies
/// fit". There is no second live copy to fit, because there is no release.
/// Found while giving the subscript arm the same gate, which would
/// otherwise have inherited it (#423).
#[test]
fn an_unretained_ivar_accumulates_rather_than_overlapping() {
    let src = program(
        "\
@interface Holder : OZObject {
	__unsafe_unretained Foo *_borrowed;
	__unsafe_unretained Foo *_borrowed_arr[4];
}
- (void)run;
- (void)fill;
@end
@implementation Holder
- (void)run
{
	int i;

	for (i = 0; i < 4; i++) {
		_borrowed = [Foo make];
	}
}
- (void)fill
{
	int i;

	for (i = 0; i < 4; i++) {
		_borrowed_arr[0] = [Foo make];
	}
}
@end

int main(void) { return 0; }
",
    );
    let diags = expect_reject(&src);
    assert_eq!(
        diags.matches("an ivar that is not an owned strong slot").count(),
        2,
        "both the scalar and the array spelling of an unretained ivar accumulate, and neither \
         should be told to raise a pool that cannot help; got:\n{}",
        diags
    );
    assert!(
        !diags.contains("oz-pool"),
        "no pool size bounds a loop that never releases, so the advice must not appear here; \
         got:\n{}",
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
