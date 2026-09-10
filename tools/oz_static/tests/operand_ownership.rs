// SPDX-License-Identifier: Apache-2.0
//
// operand_ownership.rs -- the positions a `+1` expression can appear in,
// and who releases it (#355 and its follow-ups).
//
// One cause behind every case here: until this work, only an
// `expression_statement` and a `declaration` ever asked the ownership
// question. `emit::collect_owning_operands` was reached from those two
// arms alone, and every other position that can hold an expression --
// a `return`, an `if` or `switch` condition, a plain C call's argument
// list -- never asked at all. Nine positions were wrong; five are fixed
// here and the rest are tracked separately, because they are evaluated
// *more than once* and a hoisted temporary would allocate once where the
// source allocates every time round.
//
// Two things about how these are written, both load-bearing.
//
// **Every case compiles and runs.** The two failures this file exists for
// are a use-after-free and a leak, and neither is visible in emitted text
// that has not been through a compiler and a run: the UAF is valid C that
// segfaults, and the leak is valid C that quietly stops allocating. A
// text assertion would have passed on both.
//
// **A leak is asserted through slab exhaustion, not through a count.** A
// generated slab holds one slot per *allocation site* (see `pools`), so a
// function containing one `[Thing alloc]` has exactly one slot however
// many times it is called. Calling it three times therefore proves the
// release: if the reference is dropped, calls two and three get `nil`.
// That is the measured signature of the original defect --
// `keep(makeThing(1)); keep(makeThing(2)); keep(makeThing(3));` printed
// "an object", "nil", "nil" -- and it is what a device would do rather
// than report anything.

mod common;
use common::{compile_and_run_strict, ozobject_src};

/// `Thing` with a counter, `Holder` whose `-take:` hands its argument
/// straight back, and `-itself` which hands the receiver back. Those two
/// selectors are the whole point: a method that returns a *borrowed*
/// reference derived from a `+1` operand is what makes the operand's
/// temporary and the statement's result the same object.
const PRELUDE: &str = "\
#include <stdio.h>

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
@end

@interface Holder : OZObject
- (Thing *)take:(Thing *)t;
- (int)count:(Thing *)t;
@end
@implementation Holder
- (Thing *)take:(Thing *)t
{
	return t;
}
- (int)count:(Thing *)t
{
	return [t n];
}
@end

Thing *makeThing(int n)
{
	return [[Thing alloc] initWithN:n];
}

void keep(Thing *t)
{
	printf(\"keep %s\\n\", t == 0 ? \"nil\" : \"ok\");
}
";

fn program(body: &str) -> String {
    format!("{}{}\n{}", ozobject_src(), PRELUDE, body)
}

/// The use-after-free, in the argument spelling.
///
/// `Thing *z = [h take:makeThing(41)];` hoists `makeThing(41)` into a
/// temporary and released it at the end of the statement -- but `-take:`
/// hands its argument back, so `z` *was* that temporary and the release
/// freed the object `z` named. `[z n]` then read freed memory: **signal
/// 11**, measured on the host, not inferred.
///
/// The fix retains the bound value, which is what ARC emits for a
/// `__strong` slot and is correct either way -- aliased, the retain
/// covers the release; not aliased, the retain and the scope-exit release
/// cancel on a different object.
///
/// `Thing *s = [[Builder new] result];` is this same shape in ordinary
/// code, which is why it is worth a named test rather than a matrix row.
#[test]
fn a_declaration_over_a_plus_one_argument_does_not_dangle() {
    let src = program(
        "\
int main(void)
{
	Holder *h = [[Holder alloc] init];
	Thing *z = [h take:makeThing(41)];
	printf(\"z=%d\\n\", [z n]);
	return 0;
}
",
    );
    assert_eq!(compile_and_run_strict(&src, "operand_decl_arg"), "z=41\n");
}

/// The same use-after-free in the **receiver** spelling, which segfaulted
/// identically and had to be fixed by the same retain rather than by a
/// second special case: `receiver_owning_value` and
/// `owning_argument_value` are two ways into one hoist.
#[test]
fn a_declaration_over_a_plus_one_receiver_does_not_dangle() {
    let src = program(
        "\
int main(void)
{
	Thing *z = [makeThing(41) itself];
	printf(\"z=%d\\n\", [z n]);
	return 0;
}
",
    );
    assert_eq!(compile_and_run_strict(&src, "operand_decl_recv"), "z=41\n");
}

/// A `+1` handed to a **plain C function**, which asked the ownership
/// question nowhere at all.
///
/// The free-function twin of the send-argument arm, and the same
/// methods-vs-free-functions asymmetry as #326, #336 and #367 in a fifth
/// place: a method's argument list and a function's are two separate
/// walks over one question, and only one was written.
///
/// Three calls on one slab slot. Without the release the second and third
/// get `nil`, which is exactly what was measured before the fix.
#[test]
fn a_plus_one_argument_to_a_plain_c_function_is_released() {
    let src = program(
        "\
int main(void)
{
	keep(makeThing(1));
	keep(makeThing(2));
	keep(makeThing(3));
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "operand_c_arg"),
        "keep ok\nkeep ok\nkeep ok\n",
        "a +1 handed to a borrowing C function must be released at the end of the statement; \
         a `nil` here means the slab slot was never returned"
    );
}

/// An `if` **condition**, and a `switch` **value**, in one program.
///
/// Both are safe to hoist for the reason a `for` *initialiser* is and its
/// condition is not: a controlling expression is evaluated exactly once
/// per execution of the statement, and the group the statement is wrapped
/// in sits inside whatever loop encloses it -- so the allocation still
/// happens once per iteration, and the release with it. The loop here is
/// what pins that down: three iterations on one slot.
#[test]
fn a_controlling_expressions_plus_one_is_released_each_time() {
    let src = program(
        "\
int main(void)
{
	int i;

	for (i = 0; i < 3; i++) {
		if ([makeThing(i) n] > 100) {
			printf(\"unreachable\\n\");
		}
		switch ([makeThing(i) n]) {
		case 99:
			printf(\"unreachable\\n\");
			break;
		default:
			break;
		}
		printf(\"pass %d\\n\", i);
	}
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "operand_condition"),
        "pass 0\npass 1\npass 2\n",
        "an `if` condition and a `switch` value must each release their +1 per evaluation; \
         a missing `pass` line means an allocation returned nil"
    );
}

/// A `return` **out of** an operand group.
///
/// `if ([makeThing() n] > 100) { return; }` is about as ordinary as this
/// construct gets, and the group wrapping the `if` is a plain C block --
/// so a `return` inside it jumped straight past the releases that follow
/// it. The group is registered as an ARC scope for exactly this, which is
/// also what closes the same hole under #341's `for` wrapper.
#[test]
fn a_return_out_of_an_operand_group_releases_the_temporary() {
    let src = program(
        "\
static int probe(int limit)
{
	if ([makeThing(limit) n] >= 0) {
		return 1;
	}
	return 0;
}

int main(void)
{
	printf(\"a=%d b=%d c=%d\\n\", probe(1), probe(2), probe(3));
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "operand_return_out"),
        "a=1 b=1 c=1\n",
        "the early `return` must release the condition's temporary on its way out; without \
         that the second call allocates nothing and `[nil n]` is 0"
    );
}

/// A `return` whose **value** is a borrowed result over a `+1` operand.
///
/// This had no arm at all, so `return [h take:makeThing()];` leaked
/// outright. Fixing the emission alone was not enough and this is the
/// case that proves it: the emitter retains the returned value, so the
/// function hands back a `+1`, and unless
/// `arc::return_hands_back_ownership` reports that too, no caller
/// releases it and the use-after-free has merely become a leak. Both read
/// one predicate (`arc::hoists_owning_operand`) for that reason.
///
/// Three calls, one slot: the caller's release is what makes the third
/// one succeed.
#[test]
fn a_returned_borrowed_result_over_a_plus_one_is_owned_by_the_caller() {
    let src = program(
        "\
Thing *pick(Holder *h, int n)
{
	return [h take:makeThing(n)];
}

int main(void)
{
	Holder *h = [[Holder alloc] init];
	int i;

	for (i = 1; i <= 3; i++) {
		Thing *got = pick(h, i);
		printf(\"got=%d\\n\", [got n]);
	}
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "operand_return_value"),
        "got=1\ngot=2\ngot=3\n",
        "the function must be classified +1 so the caller releases what the return retained; \
         a `got=0` means an allocation found no slot"
    );
}

/// The narrowness of the retain, which is not decoration.
///
/// `int m = [h count:makeThing(1)];` is the same hoist with a result that
/// cannot possibly name the temporary, so it keeps the tight
/// statement-end release and pays nothing. That matters beyond tidiness:
/// one slab slot per allocation site means holding a value past its
/// statement can exhaust a pool that a statement-scoped release would
/// have recycled -- so retaining here would turn a working program into
/// one that allocates `nil`.
///
/// Asserted on the emitted text *and* on the run: the text is what says
/// no retain was added, and three calls on one slot is what says the
/// release still happens where it did.
#[test]
fn an_int_binding_keeps_the_tight_release_and_takes_no_retain() {
    let src = program(
        "\
int main(void)
{
	Holder *h = [[Holder alloc] init];
	int i;

	for (i = 1; i <= 3; i++) {
		int m = [h count:makeThing(i)];
		printf(\"m=%d\\n\", m);
	}
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("oz_static_retain((struct OZObject *)(m))"),
        "an `int` slot cannot name the temporary and must not be retained; got:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "operand_int_binding"), "m=1\nm=2\nm=3\n");
}

/// A `break` out of a `for` whose **header** hoisted an operand must
/// *not* release it.
///
/// The group wrapping the loop begins at the *same* byte as the `for`
/// statement, not inside it, so `releases_up_to_jump_target` stops before
/// reaching it: a `break` releases what the loop body owns and no more.
/// Give the group a start past the loop's and it releases the header's
/// temporary on the way out and then again after it -- a double release,
/// and this is the case that catches it.
#[test]
fn a_break_does_not_release_a_for_headers_operand_temporary() {
    let src = program(
        "\
int main(void)
{
	Holder *h = [[Holder alloc] init];
	int total;

	for (total = [h count:makeThing(5)]; total < 10; total++) {
		if (total == 5) {
			break;
		}
	}
	printf(\"total=%d\\n\", total);
	return 0;
}
",
    );
    assert_eq!(compile_and_run_strict(&src, "operand_break"), "total=5\n");
}

/// The guard on the arm that brought `if` here: a `+1` in a **branch** is
/// that statement's own business and must not be hoisted into the
/// condition's group.
///
/// Hoisting it would evaluate it whether the branch is taken or not,
/// which is the defect `for_header_owning_operands` avoids by the same
/// restriction. Here the branch is never taken, so the allocation must
/// never happen -- and on one slab slot, three passes prove it.
#[test]
fn a_plus_one_in_an_untaken_branch_is_not_hoisted_into_the_condition() {
    let src = program(
        "\
int main(void)
{
	Holder *h = [[Holder alloc] init];
	int i;

	for (i = 0; i < 3; i++) {
		if (i > 100) {
			keep(makeThing(i));
		}
		printf(\"pass %d\\n\", i);
	}
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "operand_untaken_branch"),
        "pass 0\npass 1\npass 2\n"
    );
}

/// `break` inside a **`switch`** must not unwind the enclosing loop.
///
/// A separate defect from the rest of this file, found by it: the case
/// above wraps a `switch` in an operand group, and the group's release
/// ran twice because the `case`'s `break` unwound straight through it.
/// The same flag made this wrong without any of that machinery involved,
/// and it was wrong on `main`:
///
/// ```c
/// struct Thing *t = ...;
/// switch (i) {
/// case 0:
///     oz_static_release(t);   /* break only exits the switch */
///     break;
/// }
/// Thing_n(t);                 /* freed */
/// oz_static_release(t);       /* and again */
/// ```
///
/// A use-after-free *and* a double free, in a `switch` inside a loop with
/// an owned local -- nothing unusual about the shape at all.
///
/// The assertion is on the run and on the text, and the text half matters
/// here: the double release is the kind of thing a slab allocator can
/// absorb silently on a given day, so "it printed 7" is not on its own
/// evidence that the release was not emitted.
#[test]
fn a_break_inside_a_switch_does_not_release_the_loops_local() {
    let src = program(
        "\
int main(void)
{
	int i;

	for (i = 0; i < 2; i++) {
		Thing *t = [[Thing alloc] initWithN:7];
		switch (i) {
		case 0:
			break;
		default:
			break;
		}
		printf(\"t=%d\\n\", [t n]);
	}
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    let releases = out.source_c.matches("oz_static_release((struct OZObject *)(t))").count();
    assert_eq!(
        releases, 1,
        "`t` must be released once, at the end of the iteration -- not by the switch's \
         `break`, which leaves only the switch; got {} releases in:\n{}",
        releases, out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "operand_switch_break"), "t=7\nt=7\n");
}

/// ...and `continue` inside a `switch` must still unwind *past* it,
/// which is the half a "switch is a boundary" flag would have broken.
///
/// `continue` leaves the loop, so the iteration's local dies and has to
/// be released on the way out. Only asking which construct the jump
/// targets gets both of these right at once.
#[test]
fn a_continue_inside_a_switch_does_release_the_loops_local() {
    let src = program(
        "\
int main(void)
{
	int i;

	for (i = 0; i < 3; i++) {
		Thing *t = [[Thing alloc] initWithN:i];
		switch (i) {
		case 0:
			continue;
		default:
			break;
		}
		printf(\"kept %d\\n\", [t n]);
	}
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("oz_static_release((struct OZObject *)(t));\n\tcontinue;")
            || out.source_c.contains("oz_static_release((struct OZObject *)(t));\n\t\t\tcontinue;")
            || out.source_c.contains("oz_static_release((struct OZObject *)(t));")
                && out.source_c.contains("continue;"),
        "`continue` must release the iteration's local on its way out; got:\n{}",
        out.source_c
    );
    /* Three iterations on one slab slot: the `continue` path has to
       return its slot or the second iteration allocates nothing. */
    assert_eq!(compile_and_run_strict(&src, "operand_switch_continue"), "kept 1\nkept 2\n");
}
