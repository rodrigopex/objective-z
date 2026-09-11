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
- (Thing *)copy;
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
/* Tags the copy differently, so a test can tell the two objects apart --
 * a leak and a correct replacement both leave *a* Thing in the slot. */
- (Thing *)copy
{
	return [[Thing alloc] initWithTag:_tag + 10];
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

/* ---- a store that reads the slot it is about to overwrite (#424) ------
 *
 * The store-shape question `emit::classify_store` asks has three answers,
 * and only two of them release the previous value by naming the slot
 * directly. The third -- a right-hand side that reads the slot, or a
 * borrowed call result -- needs the new value to exist first, so it needs a
 * temporary. A strong *ivar* got one; the other two strong slots got
 * `None` from `render_strong_local_assign` and fell through to a plain C
 * store, so the previous value was simply dropped.
 *
 * Measured on `main` before the fix, with these exact programs: the static
 * local and the global both printed `deallocs=0` where they now print 1,
 * and the borrowed-call row's `[g_slot tag]` after `[a release]` was a
 * read of freed memory. A leak in two rows and a use-after-free in the
 * third, and nothing in the tree said those slots were only partly
 * managed.
 */

/// `cached = [cached copy]` -- `+1`, and it reads the slot, so the copy
/// has to exist before the original can go.
#[test]
fn a_static_local_releases_the_previous_value_when_the_store_reads_it() {
    let src = program(
        "\
#include <stdio.h>

static int tick(void)
{
	static Thing *cached;

	cached = [[Thing alloc] initWithTag:1];
	printf(\"alloc deallocs=%d tag=%d\\n\", g_deallocs, [cached tag]);
	cached = [cached copy];
	printf(\"copy deallocs=%d tag=%d\\n\", g_deallocs, [cached tag]);
	return [cached tag];
}

int main(void)
{
	tick();
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "strong_slot_static_self_read");
    assert_eq!(
        stdout,
        "alloc deallocs=0 tag=1\ncopy deallocs=1 tag=11\n",
        "the copy must replace the original in the slot and the original must be \
         released exactly once -- it read `deallocs=0` before #424"
    );
}

/// The same shape on a file-scope global, which reaches the store through
/// `is_file_scope_object` rather than `arc_managed_slots`. Two different
/// membership tests, one lowering -- which is the whole point of routing
/// them through one function.
#[test]
fn a_global_releases_the_previous_value_when_the_store_reads_it() {
    let src = program(
        "\
#include <stdio.h>

static Thing *g_slot;

int main(void)
{
	g_slot = [[Thing alloc] initWithTag:2];
	printf(\"alloc deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	g_slot = [g_slot copy];
	printf(\"copy deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	g_slot = nil;
	printf(\"cleared deallocs=%d\\n\", g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "strong_slot_global_self_read");
    assert_eq!(
        stdout,
        "alloc deallocs=0 tag=2\ncopy deallocs=1 tag=12\ncleared deallocs=2\n",
        "each store must release what the slot held, whatever the store's right-hand \
         side reads"
    );
}

/// The other half of the same store kind: a **borrowed** right-hand side
/// that is not a plain identifier. `g_slot = pick(a)` is `+0`, so the slot
/// has to retain it as well as release what it replaced -- exactly what
/// `g_slot = a` already did. The two spell the same reference, and keying
/// on the spelling is what left them different.
///
/// The retain is what the last line proves: after the caller's own
/// `[a release]` the slot is the only owner left, so reading the tag is a
/// live read. Before the fix it was a read of freed memory.
#[test]
fn a_global_retains_a_borrowed_call_result_and_releases_what_it_replaced() {
    let src = program(
        "\
#include <stdio.h>

static Thing *g_slot;

static Thing *pick(Thing *t)
{
	return t;
}

static void stash(Thing *t)
{
	g_slot = pick(t);
}

int main(void)
{
	Thing *a = [[Thing alloc] initWithTag:1];

	g_slot = [[Thing alloc] initWithTag:2];
	printf(\"first deallocs=%d\\n\", g_deallocs);
	stash(a);
	printf(\"stashed deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	[a release];
	printf(\"released deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "strong_slot_global_borrowed_call");
    assert_eq!(
        stdout,
        "first deallocs=0\nstashed deallocs=1 tag=1\nreleased deallocs=1 tag=1\n",
        "the slot must release the object it replaced and retain the borrowed one it \
         was given, so the caller's release cannot free it"
    );
}

/// The temporary this store needs is **declared** above the enclosing
/// statement and **assigned** inside the comma expression, and this is the
/// shape that says why.
///
/// `ctx.pre_stmts` is drained by the enclosing top-level statement, so
/// anything pushed there is lifted above a loop -- above a *braced* body
/// too, measured. Pushing the declaration together with its initialiser
/// read the slot once, before the loop, and then released that same
/// pointer on every iteration: the second pass released an object already
/// freed by the first. Splitting the two keeps the capture inside the
/// iteration.
///
/// A `+1` store of this kind inside a loop is refused outright by
/// `staticbar` (it needs two slab slots), so the reachable shape is the
/// borrowed one -- which is why the loop stores a `+0` call result. Both
/// go through the same lowering.
///
/// Asserted twice over, on the emitted text and on the lifetimes, because
/// the text is where the invariant lives and the behaviour is what it is
/// for: the declaration must sit outside the loop and the capture inside
/// it, and after three iterations the object the loop replaced must have
/// died exactly once while the one it holds is still live.
#[test]
fn a_slot_store_that_reads_it_inside_a_loop_captures_once_per_iteration() {
    let body = "\
#include <stdio.h>

static Thing *g_slot;

static Thing *pick(Thing *t)
{
	return t;
}

int main(void)
{
	Thing *a = [[Thing alloc] initWithTag:1];

	g_slot = [[Thing alloc] initWithTag:2];
	for (int i = 0; i < 3; i++) {
		g_slot = pick(a);
	}
	printf(\"loop deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	[a release];
	printf(\"end deallocs=%d tag=%d\\n\", g_deallocs, [g_slot tag]);
	return 0;
}
";
    let src = program(body);

    let out = oz_static::transpile(&src).expect("should transpile");
    let main_body = out.source_c.split("int main(void)").nth(1).expect("a main");
    /* Every statement is preceded by the source it came from, as a
     * comment, so a search for emitted text finds the *comment* first and
     * the whole assertion then reads the input back to itself. This file's
     * own instance of the trap: skip the comment lines once, up front. */
    let code: Vec<&str> =
        main_body.lines().map(|line| line.trim()).filter(|line| !line.starts_with("/*")).collect();
    /* The hoisted line: a declaration and nothing else. An `=` on it would
     * be the capture, and a capture above the loop reads the slot once. */
    let declaration = code
        .iter()
        .find(|line| line.contains("_oz_prev_") && !line.contains("g_slot ="))
        .expect("the temporary is declared");
    assert!(
        !declaration.contains('='),
        "the hoisted line must declare the temporary without initialising it, \
         got `{}`:\n{}",
        declaration,
        main_body
    );
    /* The store itself: capture, assign, retain, release, all in the one
     * expression the loop body runs every iteration. */
    let store = code
        .iter()
        .find(|line| line.contains("g_slot = pick(a)"))
        .expect("the store survives");
    assert!(
        store.contains("_oz_prev_") && store.contains("= (struct OZObject *)(g_slot)"),
        "the capture must happen inside the loop, once per iteration, got `{}`",
        store
    );

    let stdout = compile_and_run(&src, "strong_slot_self_read_loop");
    assert_eq!(
        stdout,
        "loop deallocs=1 tag=1\nend deallocs=1 tag=1\n",
        "the object the first iteration replaced must die exactly once, and the one \
         the slot ends up holding must outlive the caller's own release"
    );
}
