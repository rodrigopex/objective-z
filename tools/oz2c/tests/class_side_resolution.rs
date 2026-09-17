// SPDX-License-Identifier: Apache-2.0
//
// class_side_resolution.rs -- the class side of `self`, `super` and `+new`
// (#534, #535, #539).
//
// One root cause under #534 and #535: `EmitCtx` carried no notion of
// *which side* of the class the body being rendered belonged to, so
// `render_expr`'s `self` and `super` arms both produced an **instance**
// pointer type unconditionally. Every send off either then entered
// `render_message`'s instance branch, asked `find_defining_class` for an
// *instance* method, and failed -- which is why both issues quote the
// same "class 'X' has no method matching 'sel'".
//
// #535's title says class-method `super` "is looked up only in the
// immediate superclass". `class_method_super_fails_at_the_immediate_parent_too`
// is that claim as a fixture, and it refutes it:
// `find_defining_class` walks the whole chain already and always did --
// what it filters on is `is_class_method`, so a class-side `super` send
// arriving as an instance lookup misses at *every* link including the
// first. Class-method `super` was broken outright, not broken past one
// level.
//
// #539 is a second mechanism and could not be fixed by declaring `+new`
// in the SDK with an `[[self alloc] init]` body, even once #534 is
// fixed: a generated class method takes no receiver parameter, so that
// body would render once with `self` nailed to `OZObject` and
// `[Gadget new]` would hand back a `Gadget *` pointing into an
// OZObject-sized slab slot (the `samples/heap_alloc` failure recorded in
// `emit::render_message`). `+new` is therefore declared with no body and
// resolved at the *send site*, exactly as `+alloc` and `+dynamicAlloc`
// are.

mod common;
use common::{compile_and_run, compile_and_run_strict, expect_reject, ozobject_src as PREAMBLE};

/// #534, the canonical Cocoa factory -- `px-app`'s
/// `+altimeterWithId:ceiling:` with WA-009 taken back off.
///
/// Two call sites, so this also pins the pools half of the fix: `self`
/// is not a class name, so `pools::alloc_receiver_class` counted no site
/// for `[self alloc]` and `ever_slab_allocated` concluded the class was
/// never slab-allocated -- no `k_mem_slab` at all, and a factory that
/// transpiles, links and answers **nil**. A run is the only gate that
/// sees that; reading the generated C does not.
#[test]
fn self_in_a_class_method_is_the_class() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Sensor : OZObject {
	int _id;
	int _ceiling;
}
+ (instancetype)sensorWithId:(int)sensorId ceiling:(int)ceiling;
- (instancetype)initWithId:(int)sensorId ceiling:(int)ceiling;
- (int)sensorId;
- (int)ceiling;
@end
@implementation Sensor
+ (instancetype)sensorWithId:(int)sensorId ceiling:(int)ceiling
{
	return [[self alloc] initWithId:sensorId ceiling:ceiling];
}
- (instancetype)initWithId:(int)sensorId ceiling:(int)ceiling
{
	self = [super init];
	_id = sensorId;
	_ceiling = ceiling;
	return self;
}
- (int)sensorId { return _id; }
- (int)ceiling { return _ceiling; }
@end

#include <stdio.h>
int main(void)
{
	Sensor *a = [Sensor sensorWithId:7 ceiling:9000];
	Sensor *b = [Sensor sensorWithId:8 ceiling:1000];
	printf(\"live=%d id=%d ceiling=%d\\n\", (a != 0) + (b != 0), [a sensorId], [b ceiling]);
	return 0;
}
"
	);
	let out = oz2c::transpile(&src).expect("should transpile");
	assert!(
		out.source_c.contains("OZ_SLAB_DEFINE(oz_slab_Sensor, sizeof(struct Sensor), 2, 4)"),
		"`[self alloc]` is a slab site for the enclosing class, once per call site of \
		 the factory; got:\n{}",
		out.source_c
			.lines()
			.filter(|l| l.contains("OZ_SLAB_DEFINE"))
			.collect::<Vec<_>>()
			.join("\n")
	);
	let stdout = compile_and_run_strict(&src, "self_in_a_class_method_is_the_class");
	assert_eq!(stdout, "live=2 id=7 ceiling=1000\n");
}

/// #535: a class-side `super` send reaches a `+` method four levels up,
/// which is the shape `px-app`'s `+familyDepth` wanted (WA-010). The
/// instance-side `-sampleCount` of the same shape rides along in the
/// same fixture, because it is what the issue contrasts against and it
/// has to keep working.
#[test]
fn class_method_super_walks_the_whole_chain() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface L1 : OZObject {
	int _n;
}
+ (int)familyDepth;
- (int)sampleCount;
@end
@implementation L1
+ (int)familyDepth { return 1; }
- (int)sampleCount { return 10; }
@end

@interface L2 : L1
@end
@implementation L2
@end

@interface L3 : L2
@end
@implementation L3
@end

@interface L4 : L3
@end
@implementation L4
@end

@interface L5 : L4
+ (int)familyDepth;
- (int)sampleCount;
@end
@implementation L5
+ (int)familyDepth { return [super familyDepth] + 1; }
- (int)sampleCount { return [super sampleCount] + 100; }
@end

#include <stdio.h>
int main(void)
{
	L5 *o = [[L5 alloc] init];
	printf(\"depth=%d count=%d\\n\", [L5 familyDepth], [o sampleCount]);
	return 0;
}
"
	);
	let stdout = compile_and_run(&src, "class_method_super_walks_the_whole_chain");
	assert_eq!(stdout, "depth=2 count=110\n");
}

/// #535's framing, refuted: a one-level chain fails too.
///
/// `Base` declares and implements `+depth`; `Sub` overrides it with
/// `[super depth] + 1`. Under "looked up only in the immediate
/// superclass" this is the case that works. It did not -- the lookup was
/// for an *instance* `-depth`, which `Base` has no more than `L3` does.
/// Kept as its own test rather than folded into the one above, because
/// its only job is to hold the corrected diagnosis in place.
#[test]
fn class_method_super_reaches_the_immediate_parent() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Base : OZObject {
	int _b;
}
+ (int)depth;
@end
@implementation Base
+ (int)depth { return 1; }
@end

@interface Sub : Base
+ (int)depth;
@end
@implementation Sub
+ (int)depth { return [super depth] + 1; }
@end

#include <stdio.h>
int main(void)
{
	printf(\"depth=%d\\n\", [Sub depth]);
	return 0;
}
"
	);
	let stdout = compile_and_run(&src, "class_method_super_reaches_the_immediate_parent");
	assert_eq!(stdout, "depth=2\n");
}

/// `instancetype` off a class-side `super` send covaries with the class
/// whose method issued the send, not with the class that defines the
/// method -- the class-side twin of
/// `regression_instancetype_covariance.rs`'s case 2, which did not
/// exist because no class-side `super` send could be written at all.
///
/// `compile_and_run_strict` is the assertion: Apple clang only *warns*
/// on the wrong struct pointer type, so plain `compile_and_run` would
/// pass with `Base_make_cls()`'s `struct Base *` returned straight out
/// of a function declared `struct Sub *`.
#[test]
fn class_method_super_instancetype_covaries_with_the_enclosing_class() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Base : OZObject {
	int _tag;
}
+ (instancetype)make;
- (instancetype)init;
- (int)tag;
@end
@implementation Base
+ (instancetype)make { return [[self alloc] init]; }
- (instancetype)init
{
	self = [super init];
	_tag = 7;
	return self;
}
- (int)tag { return _tag; }
@end

@interface Sub : Base
+ (instancetype)make;
@end
@implementation Sub
+ (instancetype)make { return [super make]; }
@end

#include <stdio.h>
int main(void)
{
	Sub *s = [Sub make];
	printf(\"live=%d tag=%d\\n\", s != 0, [s tag]);
	return 0;
}
"
	);
	let stdout =
		compile_and_run_strict(&src, "class_super_instancetype_covaries_with_enclosing_class");
	assert_eq!(stdout, "live=1 tag=7\n");
}

/// #539: `+new` is `[[self alloc] init]`, resolved at the send site.
///
/// `Gadget` overrides `-init`, so this fails two ways if the SDK had
/// grown an ordinary `+new` body instead: the object would come out of
/// `OZObject`'s slab, and it would run `OZObject`'s `-init`. Asserting
/// `g=99` is what distinguishes a send-site resolution from a
/// declaring-class one.
#[test]
fn new_is_alloc_plus_init_at_the_receiver_class() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Widget : OZObject {
	int _n;
}
- (instancetype)init;
- (int)n;
@end
@implementation Widget
- (instancetype)init
{
	self = [super init];
	_n = 41;
	return self;
}
- (int)n { return _n; }
@end

@interface Gadget : Widget
- (instancetype)init;
@end
@implementation Gadget
- (instancetype)init
{
	self = [super init];
	_n = 99;
	return self;
}
@end

#include <stdio.h>
int main(void)
{
	Widget *w = [Widget new];
	Gadget *g = [Gadget new];
	printf(\"w=%d g=%d\\n\", [w n], [g n]);
	return 0;
}
"
	);
	let out = oz2c::transpile(&src).expect("should transpile");
	assert!(
		out.source_c.contains("OZ_SLAB_DEFINE(oz_slab_Widget, sizeof(struct Widget), 1, 4)")
			&& out
				.source_c
				.contains("OZ_SLAB_DEFINE(oz_slab_Gadget, sizeof(struct Gadget), 1, 4)"),
		"`[C new]` is a slab site for C, the same way `[C alloc]` is; got:\n{}",
		out.source_c
			.lines()
			.filter(|l| l.contains("OZ_SLAB_DEFINE"))
			.collect::<Vec<_>>()
			.join("\n")
	);
	assert!(
		out.source_c.contains("Widget_init((struct Widget *)(Widget_oz_alloc()))")
			&& out.source_c.contains("Gadget_init((struct Gadget *)(Gadget_oz_alloc()))"),
		"each send resolves to *its own* receiver's allocator and `-init`; that is the \
		 whole difference between this and a shared `+new` body:\n{}",
		out.source_c
	);
	/* Exactly one occurrence: the bodiless prototype `render_interface`
	 * emits for any declared class method, which `+dynamicAlloc` and
	 * `+dynamicAllocWithHeap:` have carried since #413 for the same
	 * reason -- they are resolved at the send site too, so nothing ever
	 * calls or defines them. A *second* occurrence would be a call, and
	 * a call is the OZObject-sized-slot bug. */
	assert_eq!(
		out.source_c.matches("OZObject_new_cls").count(),
		1,
		"`+new` is resolved at the send site, so the prototype must stay uncalled and \
		 undefined:\n{}",
		out.source_c
	);
	let stdout = compile_and_run_strict(&src, "new_is_alloc_plus_init_at_the_receiver_class");
	assert_eq!(stdout, "w=41 g=99\n");
}

/// A class declaring its own `+new` keeps its own body.
///
/// The SDK's `+new` is declared on the root class with no
/// implementation, and the send-site synthesis is keyed on the lookup
/// landing *there*. `tests/behavior/cases/arc/owning_argument.m` has
/// shipped a user-declared `+new` returning a bare `[Thing alloc]` --
/// no `-init` -- since long before #539, and intercepting every `new`
/// unconditionally would have silently started calling `-init` on it.
#[test]
fn a_class_declaring_its_own_new_keeps_it() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Thing : OZObject {
	int _t;
}
+ (instancetype)new;
- (int)mark;
@end
@implementation Thing
+ (instancetype)new { return [Thing alloc]; }
- (int)mark { return 5; }
@end

#include <stdio.h>
int main(void)
{
	Thing *t = [Thing new];
	printf(\"mark=%d\\n\", [t mark]);
	return 0;
}
"
	);
	let out = oz2c::transpile(&src).expect("should transpile");
	assert!(
		out.source_c.contains("Thing_new_cls"),
		"an override of `+new` is an ordinary class method:\n{}",
		out.source_c
	);
	let stdout = compile_and_run(&src, "a_class_declaring_its_own_new_keeps_it");
	assert_eq!(stdout, "mark=5\n");
}

/// `self` in a class method is a compile-time class name, not a value --
/// so it is a located error anywhere but the receiver of a send.
///
/// A generated class method takes no receiver parameter (see
/// `render_method_definition`), so there is nothing for `return self;`
/// to render into. Before #534 it produced a reference to a
/// nonexistent `self` parameter; after, without this check, it produced
/// `return Sensor;` -- a bare class name, which is not C. Either way
/// the failure landed on the C compiler with no Objective-C line
/// attached, which is the part this replaces.
#[test]
fn self_as_a_value_in_a_class_method_is_refused() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Sensor : OZObject {
	int _id;
}
+ (id)itself;
@end
@implementation Sensor
+ (id)itself { return self; }
@end
"
	);
	let errs = expect_reject(&src);
	assert!(
		errs.contains("'self' in a class method names the class")
			&& errs.contains("receiver of a message send"),
		"the refusal has to say what `self` is on the class side:\n{}",
		errs
	);
}
