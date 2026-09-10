// SPDX-License-Identifier: Apache-2.0
//
// strong_slots.rs -- a strong slot outside an ivar keeps what it is given
// (#359).
//
// Three kinds of strong storage exist besides an ivar, and none of them was
// tracked: a file-scope global, a function-scope `static`, and a plain C
// struct's field. Each got a plain C store, and the local's own scope-exit
// release then destroyed the object the slot still pointed at. Clang
// reports all three as `__strong` under `-fobjc-arc`, so this was a
// divergence from ARC rather than a subset boundary -- except for the
// struct field, which is now a located error instead, because its type is
// not resolved (sending a message to one is already refused, #355) and a
// strong field would also need releasing when the struct itself dies,
// which nothing tracks.
//
// Asserted on **lifetimes**, with a dealloc counter, not only on the
// emitted text: the whole complaint was that the object died at the wrong
// time, and a retain in the right place is only evidence that it might
// not. `ownership_matrix.rs` pins the emitted shape of these and every
// other sink; this file proves the objects live and die when they should.
//
// What made the global case worth fixing rather than documenting: reading
// the slot back compiles. `[g_global tag]` lowers to `Thing_tag(g_global)`,
// so the dangling pointer is reachable from ordinary code, not merely
// present.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

const THING: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject {
	int _tag;
}
- (id)initWithTag:(int)tag;
- (int)tag;
@end
@implementation Thing
- (id)initWithTag:(int)tag
{
	self = [super init];
	if (self != nil) {
		_tag = tag;
	}
	return self;
}
- (int)tag
{
	return _tag;
}
- (void)dealloc
{
	g_deallocs = g_deallocs + 1;
}
@end
";

fn program(body: &str) -> String {
    format!("/* oz-pool: Thing=6 */\n{}{}\n{}", PREAMBLE(), THING, body)
}

/// A global assigned from a local: the object outlives the function that
/// stored it, and is released when the slot is overwritten.
///
/// Before the fix the local's scope-exit release destroyed it immediately,
/// so `after store` read 1 and the read below was a use-after-free.
#[test]
fn a_global_assigned_from_a_local_keeps_the_object_alive() {
    let src = program(
        "\
#include <stdio.h>

static Thing *g_slot;

static void store(int tag)
{
	Thing *a = [[Thing alloc] initWithTag:tag];

	g_slot = a;
}

int main(void)
{
	store(7);
	printf(\"after store deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	store(9);
	printf(\"after overwrite deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "strong_slot_global_from_local");
    assert_eq!(
        stdout,
        "after store deallocs=0 tag=7\nafter overwrite deallocs=1 tag=9\n",
        "the global must hold the object past the storing function, and release it \
         exactly once when overwritten"
    );
}

/// The same slot written twice with no local in between -- the shape a
/// singleton accessor uses. The first object must be released by the
/// second store, and the second must survive.
///
/// Before the fix this leaked: two allocations, zero releases.
#[test]
fn a_global_overwritten_releases_what_it_held() {
    let src = program(
        "\
#include <stdio.h>

static Thing *g_slot;

int main(void)
{
	g_slot = [[Thing alloc] initWithTag:1];
	printf(\"first deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	g_slot = [[Thing alloc] initWithTag:2];
	printf(\"second deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	g_slot = nil;
	printf(\"cleared deallocs=%d\\n\", g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "strong_slot_global_overwritten");
    assert_eq!(
        stdout,
        "first deallocs=0 tag=1\nsecond deallocs=1 tag=2\ncleared deallocs=2\n",
        "each store must release what the slot held, and clearing it must release the last"
    );
}

/// A `static` local: strong like any object local, but with the program's
/// storage duration, so it is *not* released when the scope ends.
///
/// This is the shape that failed twice over -- the object was destroyed on
/// the way out of the first call, and the next call's release-of-old then
/// touched the freed block. A lazy cache is exactly this shape.
#[test]
fn a_static_local_survives_the_call_and_releases_on_the_next() {
    let src = program(
        "\
#include <stdio.h>

static Thing *peek(void);

static Thing *cache(int tag)
{
	static Thing *cached;

	cached = [[Thing alloc] initWithTag:tag];
	return cached;
}

int main(void)
{
	Thing *first = cache(3);

	printf(\"first deallocs=%d tag=%d\\n\", g_deallocs, [first tag]);

	/* The second call must release the first object and keep the new one. */
	Thing *second = cache(4);

	printf(\"second deallocs=%d tag=%d\\n\", g_deallocs, [second tag]);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "strong_slot_static_local");
    assert_eq!(
        stdout,
        "first deallocs=0 tag=3\nsecond deallocs=1 tag=4\n",
        "a static local must not be released at scope exit, and must release its \
         previous value on the next store"
    );
}

/// The refusals, and their messages -- a located error is the deliverable
/// for these two, so the wording is part of the contract.
#[test]
fn storing_into_untracked_slots_is_refused_with_a_message_that_says_what_to_do() {
    let struct_field = program(
        "\
struct box { Thing *held; };
static struct box g_box;

void intoField(void)
{
	Thing *a = [[Thing alloc] initWithTag:1];

	g_box.held = a;
}
",
    );
    let diags = common::expect_reject(&struct_field);
    assert!(diags.contains("C struct field"), "expected the struct-field refusal:\n{}", diags);
    assert!(
        diags.contains("__unsafe_unretained"),
        "the message must name the way to say the struct does not own it:\n{}",
        diags
    );

    let other_ivar = program(
        "\
@interface Pair : OZObject {
	Thing *_held;
}
- (void)fill:(Pair *)other;
@end
@implementation Pair
- (void)fill:(Pair *)other
{
	Thing *a = [[Thing alloc] initWithTag:1];

	other->_held = a;
}
@end
",
    );
    let diags = common::expect_reject(&other_ivar);
    assert!(
        diags.contains("another object's ivar") && diags.contains("Pair"),
        "expected the other-object refusal, naming the class:\n{}",
        diags
    );
}

/// A borrowed reference put into a C struct field is still allowed: nothing
/// releases it, so the field is an unowned reference and that is the
/// author's business, exactly as in C. Refusing this too would reject
/// ordinary code that cannot dangle.
///
/// The field is written `struct Thing *`, the tagged spelling, because the
/// bare `Thing *` one does not survive into compilable C at all -- a
/// file-scope C struct declaration is copied into the companion header
/// verbatim, so the class name arrives with no `struct` tag and the header
/// fails with `unknown type name 'Thing'`. That is a separate defect,
/// unrelated to ownership and older than this change, and using the
/// spelling that works is what keeps this test about ownership.
#[test]
fn a_borrowed_reference_may_still_be_stored_into_a_struct_field() {
    let src = program(
        "\
#include <stdio.h>

struct box { struct Thing *held; int n; };
static struct box g_box;

static void fill(Thing *borrowed)
{
	g_box.held = borrowed;
	g_box.n = 5;
}

int main(void)
{
	Thing *owner = [[Thing alloc] initWithTag:8];

	fill(owner);
	printf(\"n=%d deallocs=%d\\n\", g_box.n, g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "strong_slot_borrowed_field");
    assert_eq!(stdout, "n=5 deallocs=0\n");
}
