// SPDX-License-Identifier: Apache-2.0
//
// factory_pool_sizing.rs -- a slab is sized from the *call sites* of an
// owning factory, not from the single `alloc` inside it.
//
// `pools::count_sites` counts a `message_expression` whose receiver is a
// literal class name and whose selector is literally `alloc`
// (`pools::alloc_receiver_class`). Every other way of producing a `+1`
// is invisible to it, so a factory called N times is sized for the one
// `alloc` in its body and the program gets 1 slot where it needs N.
//
// Not a bigger selector list: counting an owning factory's call sites
// *and* the `alloc` inside it double-counts. An allocation site inside
// an owning method has to be attributed to that method's callers
// instead of to itself.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// Three live instances from a factory, with no pool directive at all.
///
/// Sizing must reach 3. It counted 1 -- the `alloc` inside `+make` -- so
/// the second and third calls drew from an exhausted slab and answered
/// nil, with nothing at build time to say so.
#[test]
fn a_factory_called_three_times_needs_three_slots() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Thing : OZObject {
	int _n;
}
+ (Thing *)make;
@end
@implementation Thing
+ (Thing *)make
{
	return [[Thing alloc] init];
}
@end

#include <stdio.h>
int main(void)
{
	Thing *a = [Thing make];
	Thing *b = [Thing make];
	Thing *c = [Thing make];
	printf(\"live=%d\\n\", (a != 0) + (b != 0) + (c != 0));
	return 0;
}
"
	);
	let stdout = compile_and_run(&src, "factory_called_three_times_needs_three_slots");
	assert_eq!(
		stdout, "live=3\n",
		"each call site of an owning factory needs its own slab slot"
	);
}

/// The precision that makes this worth doing properly: a factory that
/// allocates a helper it does **not** return costs one slot for the
/// helper and N for what it hands back.
///
/// Multiplying every site in an owning method by its call-site count
/// would give the helper 3 too. That is safe but wastes slab RAM, which
/// on this target is the scarce thing --
/// `arc::allocation_escapes_via_return` is what tells the two apart.
#[test]
fn a_helper_the_factory_drops_stays_at_one_slot() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Helper : OZObject {
	int _h;
}
@end
@implementation Helper
@end

@interface Thing : OZObject {
	int _n;
}
+ (Thing *)make;
@end
@implementation Thing
+ (Thing *)make
{
	Helper *h = [Helper alloc];
	if (h == 0) {
		return 0;
	}
	return [[Thing alloc] init];
}
@end

#include <stdio.h>
int main(void)
{
	Thing *a = [Thing make];
	Thing *b = [Thing make];
	Thing *c = [Thing make];
	printf(\"live=%d\\n\", (a != 0) + (b != 0) + (c != 0));
	return 0;
}
"
	);
	let out = oz2c::transpile(&src).expect("should transpile");
	assert!(
		out.source_c.contains("OZ_SLAB_DEFINE(oz_slab_Thing, sizeof(struct Thing), 3, 4)"),
		"Thing escapes +make, so it needs one slot per call site; got:\n{}",
		slab_lines(&out.source_c)
	);
	assert!(
		out.source_c.contains("OZ_SLAB_DEFINE(oz_slab_Helper, sizeof(struct Helper), 1, 4)"),
		"Helper dies inside +make, so one slot serves every call; got:\n{}",
		slab_lines(&out.source_c)
	);
	let stdout = compile_and_run(&src, "factory_helper_stays_at_one_slot");
	assert_eq!(stdout, "live=3\n", "and it still runs");
}

/// Nested factories multiply: `+pair` calls `+make` twice and is itself
/// called once, so `+make`'s allocation costs two slots.
#[test]
fn a_factory_called_from_a_factory_multiplies() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Thing : OZObject {
	int _n;
}
+ (Thing *)make;
+ (Thing *)pair;
@end
@implementation Thing
+ (Thing *)make
{
	return [[Thing alloc] init];
}
+ (Thing *)pair
{
	Thing *first = [Thing make];
	Thing *second = [Thing make];
	if (first == 0 || second == 0) {
		return 0;
	}
	return second;
}
@end

int main(void) { return [Thing pair] != 0; }
"
	);
	let out = oz2c::transpile(&src).expect("should transpile");
	assert!(
		out.source_c.contains("OZ_SLAB_DEFINE(oz_slab_Thing, sizeof(struct Thing), 2, 4)"),
		"two call sites of +make, reached once, is two slots; got:\n{}",
		slab_lines(&out.source_c)
	);
}

/// A call cycle has no finite answer, so it is a located error naming the
/// cycle and the override -- not a guess.
#[test]
fn an_escaping_allocation_in_a_call_cycle_is_refused() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Thing : OZObject {
	int _n;
}
+ (Thing *)ping:(int)n;
+ (Thing *)pong:(int)n;
@end
@implementation Thing
+ (Thing *)ping:(int)n
{
	if (n <= 0) {
		return [[Thing alloc] init];
	}
	return [Thing pong:n - 1];
}
+ (Thing *)pong:(int)n
{
	return [Thing ping:n - 1];
}
@end

int main(void) { return [Thing ping:4] != 0; }
"
	);
	let diags = expect_reject(&src);
	assert!(
		diags.contains("call cycle") && diags.contains("oz-pool"),
		"the diagnostic must name the cycle and how to size around it; got:\n{}",
		diags
	);
}

/// Every `OZ_SLAB_DEFINE` line, for a failure message worth reading.
fn slab_lines(source_c: &str) -> String {
	source_c
		.lines()
		.filter(|l| l.contains("OZ_SLAB_DEFINE"))
		.collect::<Vec<_>>()
		.join("\n")
}

/// An uncalled **class method** contributes nothing, and an uncalled
/// instance method still contributes one. The asymmetry is the whole
/// reason sizing did not blow up when #410 landed.
///
/// A class-method receiver is always statically known
/// (`pools::alloc_receiver_class`), so "nothing calls it here" is a fact.
/// An instance send can arrive through dynamic dispatch, so the same
/// silence means "unknown" and has to keep the floor of one -- the
/// failure mode of guessing low is a nil from an exhausted slab.
///
/// Measured against `src/OZNumber.m`, which is what found this: it has
/// seventeen `+fixedWith...` forwarders that allocate nothing and are
/// uncalled in most programs. Giving each of them a floor of one sized
/// OZNumber at 16 in `samples/hello_category`, which uses no OZNumber at all.
#[test]
fn an_uncalled_class_method_contributes_no_slot() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Thing : OZObject {
	int _n;
}
+ (Thing *)unused;
+ (Thing *)used;
@end
@implementation Thing
+ (Thing *)unused
{
	return [[Thing alloc] init];
}
+ (Thing *)used
{
	return [[Thing alloc] init];
}
@end

int main(void)
{
	Thing *a = [Thing used];
	Thing *b = [Thing used];
	return (a != 0) + (b != 0);
}
"
	);
	let out = oz2c::transpile(&src).expect("should transpile");
	assert!(
		out.source_c.contains("OZ_SLAB_DEFINE(oz_slab_Thing, sizeof(struct Thing), 2, 4)"),
		"two call sites of +used, and nothing for +unused; got:\n{}",
		slab_lines(&out.source_c)
	);
}

/// `[self alloc]` in a `+` method is a slab site for the enclosing class
/// (#534).
///
/// **The nil-at-runtime trap this whole file is about, in its worst
/// form.** `alloc_receiver_class` required the receiver *text* to be a
/// literal class name, so a `self` receiver counted nothing -- and since
/// #419 that is not just a slot count: `slab_sites` never learned the
/// class, `ever_slab_allocated` answered no, and `for_class` returned 0,
/// which the emitters read as "emit no `k_mem_slab` at all". Fixing
/// `self`-as-class in `emit.rs` alone would have turned #534's hard
/// located error into a factory that transpiles, links, and hands back
/// **nil** on the first call -- strictly worse, and invisible to every
/// gate that reads the generated C instead of running it.
///
/// Three call sites, so a missing multiplicity fails here too rather
/// than only the missing slab.
#[test]
fn self_alloc_in_a_class_method_is_a_slab_site() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Thing : OZObject {
	int _n;
}
+ (instancetype)make;
@end
@implementation Thing
+ (instancetype)make
{
	return [[self alloc] init];
}
@end

#include <stdio.h>
int main(void)
{
	Thing *a = [Thing make];
	Thing *b = [Thing make];
	Thing *c = [Thing make];
	printf(\"live=%d\\n\", (a != 0) + (b != 0) + (c != 0));
	return 0;
}
"
	);
	let out = oz2c::transpile(&src).expect("should transpile");
	assert!(
		out.source_c.contains("OZ_SLAB_DEFINE(oz_slab_Thing, sizeof(struct Thing), 3, 4)"),
		"`[self alloc]` allocates from the enclosing class's slab, once per call site \
		 of the factory; got:\n{}",
		slab_lines(&out.source_c)
	);
	let stdout = compile_and_run(&src, "self_alloc_in_a_class_method_is_a_slab_site");
	assert_eq!(
		stdout, "live=3\n",
		"a slab that was never emitted answers nil, and nothing at build time says so"
	);
}

/// `[C new]` is a slab site for C, the same way `[C alloc]` is (#539).
///
/// The inherited `+new` is synthesized at the send site as the
/// receiver's own `alloc` plus `init`, so it takes a slot -- but the
/// selector comparison here is whole-string against `alloc`, so it
/// counted nothing and the class lost its slab exactly as above.
///
/// A class declaring **its own** `+new` is the other half: that is an
/// ordinary class method whose body allocates however it says, so
/// counting the send as well as the `alloc` inside it would double-count.
/// `emit::new_is_synthesized` is the one predicate both sides read.
#[test]
fn new_is_a_slab_site_only_where_it_is_synthesized() {
	let src = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Plain : OZObject {
	int _n;
}
@end
@implementation Plain
@end

@interface Own : OZObject {
	int _n;
}
+ (instancetype)new;
@end
@implementation Own
+ (instancetype)new
{
	return [[Own alloc] init];
}
@end

#include <stdio.h>
int main(void)
{
	Plain *p = [Plain new];
	Plain *q = [Plain new];
	Own *o = [Own new];
	printf(\"live=%d\\n\", (p != 0) + (q != 0) + (o != 0));
	return 0;
}
"
	);
	let out = oz2c::transpile(&src).expect("should transpile");
	assert!(
		out.source_c.contains("OZ_SLAB_DEFINE(oz_slab_Plain, sizeof(struct Plain), 2, 4)"),
		"two synthesized `[Plain new]` sends are two slots; got:\n{}",
		slab_lines(&out.source_c)
	);
	assert!(
		out.source_c.contains("OZ_SLAB_DEFINE(oz_slab_Own, sizeof(struct Own), 1, 4)"),
		"`+new`'s own body owns the `alloc`, counted once for its one call site -- not \
		 twice; got:\n{}",
		slab_lines(&out.source_c)
	);
	let stdout = compile_and_run(&src, "new_is_a_slab_site_only_where_it_is_synthesized");
	assert_eq!(stdout, "live=3\n");
}
