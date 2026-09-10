// SPDX-License-Identifier: Apache-2.0
//
// for_header_ownership.rs -- the one shape in #376 that is evaluated
// exactly once and still leaked: a `for` header's own `+1` declaration.
//
// `for (Thing *t = makeThing(); i < 1; i++) { [t n]; }` evaluates
// `makeThing()` once, so nothing about *when* it allocates was ever
// wrong. What was wrong is that `t` is declared inside the header, where
// no ARC scope could see the name -- `owned_locals_of` is reached from a
// `declaration` whose parent is a `compound_statement`, and a header
// declaration's parent is the `for_statement`. So the reference was
// created and abandoned, once per execution of the loop.
//
// The fix rewrites the header rather than wrapping the statement: the
// declaration is lifted into a braced group above the loop, the loop
// keeps an empty initialiser, and the release lands after it. See
// `emit::render_for_header_owned_declaration`.
//
// Two things about how these cases are written, both load-bearing, and
// both inherited from `operand_ownership.rs`:
//
// **Every case compiles and runs.** The failures this family produces are
// a leak that is valid C and quietly stops allocating, and a double
// release that is valid C and corrupts a freelist. Neither is visible in
// emitted text, and a text-only assertion would have passed on both --
// which is why this whole family survived as long as it did.
//
// **A leak is asserted through slab exhaustion as well as a dealloc
// count.** A generated slab holds one slot per *allocation site* (see
// `pools`), and `[Thing alloc]` appears exactly once in the shared
// prelude below -- inside `makeThing`. So the whole program has one
// `Thing` slot however many times it allocates, and calling a
// header-declaring function three times proves the release: if the
// reference is dropped, calls two and three get `nil`. The dealloc
// counter then says the teardown ran, and *how often*, which is what
// separates a missing release from a double one.

mod common;
use common::{compile_and_run_strict, ozobject_src};

/// `Thing` counts its own teardowns; `makeThing` is the single
/// `[Thing alloc]` site in the program, so the slab has exactly one slot.
///
/// `-itself` hands the receiver straight back, which is the only way to
/// write a **borrowed** initialiser for a `for` header without a second
/// allocation site: `for (Thing *t = [owned itself]; ...)` must not be
/// released, and if it were, the object `owned` still names would be
/// freed under it.
const PRELUDE: &str = "\
#include <stdio.h>

static int g_deallocs = 0;

@interface Thing : OZObject {
	int _n;
}
- (id)initWithN:(int)n;
- (int)n;
- (Thing *)itself;
@end
@implementation Thing
- (id)initWithN:(int)n
{
	self = [super init];
	if (self != nil) {
		_n = n;
	}
	return self;
}
- (int)n
{
	return _n;
}
- (Thing *)itself
{
	return self;
}
- (void)dealloc
{
	g_deallocs = g_deallocs + 1;
}
@end

Thing *makeThing(int n)
{
	return [[Thing alloc] initWithN:n];
}

void keep(Thing *t)
{
	if (t == 0) {
		printf(\"nil\\n\");
	} else {
		printf(\"n=%d\\n\", [t n]);
	}
}
";

fn program(body: &str) -> String {
    format!("{}{}\n{}", ozobject_src(), PRELUDE, body)
}

/// The leak itself: a `+1` bound by the header, released after the loop.
///
/// Three calls against a one-slot slab. Before the fix this printed
/// `n=1`, `nil`, `nil` and `deallocs=0` -- the measured signature of the
/// defect, and exactly what a device would do rather than report
/// anything.
#[test]
fn a_for_headers_own_plus_one_declaration_is_released_after_the_loop() {
    let src = program(
        "\
void spin(int n)
{
	int i = 0;

	for (Thing *t = makeThing(n); i < 1; i++) {
		keep(t);
	}
}

int main(void)
{
	spin(1);
	spin(2);
	spin(3);
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "for_header_decl_released"),
        "n=1\nn=2\nn=3\ndeallocs=3\n"
    );
}

/// A `return` out of the loop unwinds through the group and releases it.
///
/// This is why the lifted declaration has to be a real ARC scope rather
/// than a plain brace: a `return` inside the body jumps straight past the
/// trailing release, so without the scope the leak is back on exactly the
/// path an early exit takes. Three calls, one slab slot, so the second
/// call gets `nil` if the first did not release.
#[test]
fn a_return_out_of_the_loop_releases_the_header_declaration() {
    let src = program(
        "\
int find(int n)
{
	int i = 0;

	for (Thing *t = makeThing(n); i < 5; i++) {
		if ([t n] == n) {
			return [t n];
		}
	}
	return -1;
}

int main(void)
{
	printf(\"a=%d\\n\", find(11));
	printf(\"b=%d\\n\", find(12));
	printf(\"c=%d\\n\", find(13));
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "for_header_decl_return"),
        "a=11\nb=12\nc=13\ndeallocs=3\n"
    );
}

/// A `break` out of the loop must **not** release it -- the trailing
/// release still runs when the loop finishes, and a release at the jump
/// as well is a double free.
///
/// This is what pins the group's `start_byte` to the `for_statement`'s
/// own. `releases_up_to_jump_target` releases exactly the scopes that
/// began *strictly inside* the construct being left, so a group starting
/// at the same byte as the loop is left alone by a `break` out of it. Set
/// one byte later -- or given the loop body's own offset -- the release
/// would run twice: `deallocs=6` against the three this asserts, with a
/// freelist corrupted on the way.
#[test]
fn a_break_out_of_the_loop_does_not_double_release_the_header_declaration() {
    let src = program(
        "\
void stop(int n)
{
	int i = 0;

	for (Thing *t = makeThing(n); i < 5; i++) {
		keep(t);
		break;
	}
}

int main(void)
{
	stop(21);
	stop(22);
	stop(23);
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "for_header_decl_break"),
        "n=21\nn=22\nn=23\ndeallocs=3\n"
    );
}

/// A `continue` must not release it either, and here the consequence is a
/// use-after-free rather than a double free: the next iteration reads `t`.
///
/// The same boundary as `break` answers it, which is the point --
/// `continue` crosses a `switch` where `break` stops at one, and the byte
/// comparison gets both without a flag per construct.
#[test]
fn a_continue_inside_the_loop_keeps_the_header_declaration_alive() {
    let src = program(
        "\
void skip(int n)
{
	int i = 0;

	for (Thing *t = makeThing(n); i < 3; i++) {
		if (i == 0) {
			continue;
		}
		keep(t);
	}
}

int main(void)
{
	skip(31);
	skip(32);
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "for_header_decl_continue"),
        "n=31\nn=31\nn=32\nn=32\ndeallocs=2\n"
    );
}

/// A **borrowed** initialiser is not released.
///
/// `-itself` hands back the receiver, so `t` is `owned`'s object and
/// releasing it would free what `owned` still names. `deallocs=0` before
/// `main` returns says the loop released nothing, and `still=9` says the
/// object is intact -- the assertion a dealloc count alone cannot make,
/// since a freed slab block can still read back its old contents.
///
/// This is the case `arc::binds_ownership` decides: it is the *only*
/// predicate that separates this header from the one above, and widening
/// it would turn this test into a use-after-free.
#[test]
fn a_borrowed_for_header_declaration_is_not_released() {
    let src = program(
        "\
int main(void)
{
	Thing *owned = makeThing(9);
	int i = 0;

	for (Thing *t = [owned itself]; i < 1; i++) {
		keep(t);
	}
	printf(\"deallocs=%d\\n\", g_deallocs);
	printf(\"still=%d\\n\", [owned n]);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "for_header_decl_borrowed"),
        "n=9\ndeallocs=0\nstill=9\n"
    );
}

/// A non-object header declaration comes out **byte-identical**.
///
/// The plain C `for` is by far the most common statement this arm's guard
/// is asked about, and the guard has to answer no: `arc::binds_ownership`
/// says the initialiser takes over nothing, and `arc::declares_pointer`
/// says the slot could not hold an object anyway. Asserted on the emitted
/// text as well as the run, because "unchanged" is a claim about the
/// output and not about the behaviour -- a lifted `int i` would still
/// print `sum=3`.
#[test]
fn a_non_object_for_header_declaration_is_byte_identical() {
    let src = program(
        "\
int main(void)
{
	int sum = 0;

	for (int i = 0; i < 3; i++) {
		sum = sum + i;
	}
	printf(\"sum=%d\\n\", sum);
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("plain C for loop must transpile");
    assert!(
        out.source_c.contains("for (int i = 0; i < 3; i++) {"),
        "the header must be left exactly as written:\n{}",
        out.source_c
    );
    assert!(
        !out.source_c.contains("for (; i < 3;"),
        "nothing may be lifted out of a non-object header:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "for_header_decl_plain_c"), "sum=3\n");
}

/// A header slot that is **not a pointer** is not lifted either, however
/// owning its initialiser looks.
///
/// `arc::declares_pointer` is the guard, and it is not redundant with the
/// provenance one: `binds_ownership` looks through a non-bridging cast
/// (#332), so `(long)makeThing()` reports a `+1` while the slot holding
/// it cannot be released as an object. Measured rather than reasoned --
/// the statement-level twin of this shape, `long n = (long)makeThing();`,
/// emits `oz_static_release((struct OZObject *)(n))` against a `long`
/// today, because `owned_locals_of_in` has no such check of its own.
/// That is a separate defect; the guard here is what keeps this arm from
/// reproducing it.
///
/// Only the emitted text is asserted, deliberately: the reference *does*
/// go unreleased in this shape, and that is not what this test is about
/// -- releasing through an integer is the worse of the two wrongs, and
/// which of them to fix is a question about `declares_pointer` and not
/// about the `for` header.
#[test]
fn a_non_pointer_for_header_declaration_is_left_alone() {
    let src = program(
        "\
int main(void)
{
	int i = 0;

	for (long n = (long)makeThing(7); i < 1; i++) {
		printf(\"live=%d\\n\", n != 0);
	}
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("a cast-to-long header must transpile");
    assert!(
        out.source_c.contains("for (long n = (long)(makeThing(7)); i < 1; i++) {"),
        "a non-pointer header must be left as written:\n{}",
        out.source_c
    );
    assert!(
        !out.source_c.contains("oz_static_release((struct OZObject *)(n))"),
        "a non-pointer slot must not be released as an object:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "for_header_decl_non_pointer"), "live=1\n");
}
