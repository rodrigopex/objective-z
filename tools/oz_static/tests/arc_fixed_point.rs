// SPDX-License-Identifier: Apache-2.0
//
// arc_fixed_point.rs - `arc::analyze` iterates to a fixed point over *both*
// sets it maintains, not just `methods` (#450).
//
// `OwningMethods` holds two: `methods`, keyed (class, selector), and
// `functions`, the plain top-level C functions that return `+1`. Iterating to
// a fixed point is the one improvement over the retired Python oracle's
// single pass -- it is what lets a factory whose `return` calls *another*
// factory be recognised. The loop guard read `owning.methods.len()`, so a
// pass that discovered only new owning C **functions** left that number
// unchanged and returned. `OwningMethods::len()` already summed both; the fix
// is to guard on it.
//
// **Source order is the whole test.** `scan_once` walks in source order and
// consults `functions` as it grows, so a chain written inner-first resolves
// entirely within one pass and proves nothing -- the bug is invisible to it.
// Written outer-first, each level needs its own pass:
//
//   pass 1: makeL3 sees makeL2 unknown, makeL2 sees makeL1 unknown,
//           makeL1 is owning        -> functions = {makeL1}
//   pass 2: makeL2 resolves         -> functions = {makeL1, makeL2}
//   pass 3: makeL3 resolves         -> functions = {makeL1, makeL2, makeL3}
//   pass 4: no change               -> done
//
// With the old guard, pass 1 leaves `methods` empty and the loop returns, so
// `makeL2` and `makeL3` are classified `+0`, their callers treat the result
// as borrowed, and nothing releases it.
//
// **Three levels, not two,** as #450 asks: a single-pass bug and an
// off-by-one both pass a two-level test. Two levels need two passes, which
// an off-by-one still reaches; three need three.
//
// Leak direction, so this has never crashed anything. The shape it matters
// for is the one the C-factory support was written for in the first place --
// `samples/arc_demo`'s `static Sensor *createSensor(int)`, where treating an
// owning C factory as borrowed left the one-slot slab occupied and produced
// an MPU fault.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// The class every case allocates, counting its own deallocations.
fn sensor() -> &'static str {
    "\
static int g_deallocs = 0;

@interface Sensor : OZObject {
	int _v;
}
- (id)initWithValue:(int)v;
- (int)value;
@end

@implementation Sensor
- (id)initWithValue:(int)v {
	self = [super init];
	if (self != nil) {
		_v = v;
	}
	return self;
}
- (int)value {
	return _v;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end
"
}

/// A three-deep C-factory chain, written outer-first, is recognised as
/// owning all the way up.
///
/// One allocation, so exactly one `-dealloc` -- and it has to have run
/// *before* the final `printf`, which is what binding to a local inside a
/// braced scope arranges. A `deallocs=0` is the leak: `makeL3` classified
/// `+0`, so the local was treated as borrowed and never released.
#[test]
fn a_three_deep_c_factory_chain_is_owning_at_every_level() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        sensor(),
        "\
#include <stdio.h>

/* Prototypes, so the definitions below can be written outer-first. That
 * order is deliberate: see the file header. */
static Sensor *makeL1(int v);
static Sensor *makeL2(int v);
static Sensor *makeL3(int v);

static Sensor *makeL3(int v)
{
	return makeL2(v);
}

static Sensor *makeL2(int v)
{
	return makeL1(v);
}

static Sensor *makeL1(int v)
{
	return [[Sensor alloc] initWithValue:v];
}

int main(void)
{
	{
		Sensor *s = makeL3(7);

		printf(\"v=%d\\n\", [s value]);
	}
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "arc_fixed_point_three_deep");
    assert_eq!(
        out, "v=7\ndeallocs=1\n",
        "every level of the chain returns +1, so the local owns it and its scope releases \
         it. deallocs=0 means the fixed point stopped early and makeL3 was called +0; \
         got:\n{}",
        out
    );
}

/// The same chain written **inner-first**, which one pass already resolved.
///
/// Not redundant -- it is the control that says the test above is about the
/// fixed point and not about C factories in general. This shape passed before
/// #450 and must keep passing, so a future change to the guard cannot claim
/// a fix by breaking the easy case.
#[test]
fn the_same_chain_written_inner_first_still_works() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        sensor(),
        "\
#include <stdio.h>

static Sensor *makeL1(int v)
{
	return [[Sensor alloc] initWithValue:v];
}

static Sensor *makeL2(int v)
{
	return makeL1(v);
}

static Sensor *makeL3(int v)
{
	return makeL2(v);
}

int main(void)
{
	{
		Sensor *s = makeL3(9);

		printf(\"v=%d\\n\", [s value]);
	}
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "arc_fixed_point_inner_first");
    assert_eq!(out, "v=9\ndeallocs=1\n", "unexpected:\n{}", out);
}

/// A chain that mixes a method into the middle was broken too, and I had it
/// backwards until this test said otherwise.
///
/// The guess was that `methods` and `functions` growing on different passes
/// would let a mixed chain through -- a pass that found a method *did*
/// re-iterate, so surely any chain containing one could reach full depth.
/// It fails, for a reason worth keeping: discovery runs in source order, and
/// here the method sits **upstream** of the C function it depends on. Pass 1
/// finds `Factory build:` unresolvable (its `return makeL1(v)` needs a
/// function not yet in the set), then `makeOuter` unresolvable (its
/// `[Factory build:v]` needs a method not yet in the set), then `makeL1`.
/// `methods` is still empty, so the old guard returned -- and the method was
/// never found on any later pass, because there were no later passes.
///
/// "A pass that finds a method re-iterates" is true and useless: the bug is
/// that a pass finding *only* a function does not, and a method downstream of
/// such a function therefore never gets its turn.
#[test]
fn a_chain_through_a_class_method_also_reaches_full_depth() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        sensor(),
        "\
#include <stdio.h>

static Sensor *makeL1(int v);

@interface Factory : OZObject
+ (Sensor *)build:(int)v;
@end
@implementation Factory
+ (Sensor *)build:(int)v {
	return makeL1(v);
}
@end

static Sensor *makeOuter(int v)
{
	return [Factory build:v];
}

static Sensor *makeL1(int v)
{
	return [[Sensor alloc] initWithValue:v];
}

int main(void)
{
	{
		Sensor *s = makeOuter(11);

		printf(\"v=%d\\n\", [s value]);
	}
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "arc_fixed_point_via_method");
    assert_eq!(out, "v=11\ndeallocs=1\n", "unexpected:\n{}", out);
}
