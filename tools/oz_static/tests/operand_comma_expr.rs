// SPDX-License-Identifier: Apache-2.0
//
// operand_comma_expr.rs -- a `+1` operand in a position that is evaluated
// conditionally or repeatedly carries its release *inside* the expression
// (#376).
//
// These are the shapes the statement-level hoist cannot serve, and the
// reason is not that it releases in the wrong place -- it is that hoisting
// **moves the allocation**. Lifting a temporary above a `while` allocates
// once where the source allocates per iteration; lifting it out of an
// `&&` allocates whether or not the left operand permits it. So the fix
// is not a different release site but a different *shape*: a comma
// expression over a declaration-only temporary, which evaluates exactly
// where the source does.
//
//     (tmp = makeThing(), v = Thing_n(tmp), oz_static_release(tmp), v)
//
// The declaration is hoisted through `ctx.pre_stmts` and only the
// assignment and release stay inside. A declaration with no initialiser
// evaluates nothing, so lifting it above a loop costs nothing and
// reorders nothing -- which is why this needed no new hoisting machinery.
//
// Two kinds of assertion here, and both are necessary:
//
//   - **a leak is asserted through slab exhaustion.** A slab holds one
//     slot per *allocation site*, so a one-site factory called three
//     times proves the release: if the reference is dropped, calls two
//     and three get `nil`. This is the measured signature of the original
//     defect and what a device would actually do.
//   - **eager allocation is asserted by counting evaluations.** The
//     factory prints, so a test can say outright that the untaken branch
//     never ran. Slab counting cannot see this: before this change
//     `if (x && [makeThing() n])` was *balanced*, allocating and
//     releasing on a path the source never takes. Nothing but observing
//     the side effect catches it.

mod common;
use common::{compile_and_run_strict, ozobject_src};

/// `makeThing` announces itself, so a test can count evaluations rather
/// than infer them. `borrow` is a plain C function taking a borrowed
/// reference -- the consumer that has no send at all.
const PRELUDE: &str = "\
#include <stdio.h>

@interface Thing : OZObject {
	int _n;
}
- (id)initWithN:(int)n;
- (int)n;
- (void)poke;
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
- (void)poke
{
	printf(\"poke %d\\n\", _n);
}
@end

Thing *makeThing(int n)
{
	printf(\"made %d\\n\", n);
	return [[Thing alloc] initWithN:n];
}

int borrow(Thing *t)
{
	return [t n];
}
";

fn program(body: &str) -> String {
    format!("{}{}\n{}", ozobject_src(), PRELUDE, body)
}

/// Every loop position, in one program, on **one slab slot**.
///
/// `while`, `do`-`while`, a `for` condition and a `for` update each
/// created a `+1` per iteration and abandoned it. Three iterations of each
/// with a single slot is what proves the release: a dropped reference
/// makes the second iteration allocate nothing, and `[nil n]` is 0, so
/// the loop's own bound changes and the output does with it.
#[test]
fn every_loop_position_releases_its_plus_one_each_iteration() {
    let src = program(
        "\
int main(void)
{
	int i;
	int seen = 0;

	i = 0;
	while (i < 3 && [makeThing(i) n] >= 0) {
		seen++;
		i++;
	}
	i = 0;
	do {
		seen++;
		i++;
	} while (i < 3 && [makeThing(i + 10) n] >= 0);
	for (i = 0; i < 3 && [makeThing(i + 20) n] >= 0; i++) {
		seen++;
	}
	for (i = 0; i < 3; i++, [makeThing(i + 30) n]) {
		seen++;
	}
	printf(\"seen=%d\\n\", seen);
	return 0;
}
",
    );
    let out = compile_and_run_strict(&src, "comma_loops");
    /* Each loop runs three times, so twelve allocations must all have
       found the one slot free. `seen` counting 12 is the whole claim. */
    assert!(
        out.ends_with("seen=12\n"),
        "every iteration must get a fresh object from the single slab slot; got:\n{}",
        out
    );
    /* **Eleven**, not twelve, and the arithmetic is the point. Each loop
       runs its body three times, but the `do`-`while` evaluates its
       condition only twice: on the third pass `i < 3` is false and the
       `&&` short-circuits before the operand, so no allocation happens.
       3 + 2 + 3 + 3. A count of twelve would mean the operand was being
       evaluated where the source says it is not -- which is exactly the
       eager-hoist defect -- so pinning the exact number is worth more
       here than pinning a round one. */
    assert_eq!(
        out.matches("made ").count(),
        11,
        "one evaluation per iteration, and none where the left operand short-circuits \
         first; got:\n{}",
        out
    );
}

/// The eager-allocation regression, and the case slab counting cannot see.
///
/// `if (x && [makeThing() n] > 0)` was a leak until the `if`-condition arm
/// reached it, and then became *balanced but eager*: the allocation was
/// hoisted above the statement, so it ran whether `x` was true or not. On
/// a one-slot pool that can exhaust the slab from a branch the source
/// never takes, and it is observable outright whenever the factory has a
/// side effect -- which is exactly what this asserts. `||` is the mirror
/// image: its right operand runs only when the left is false.
#[test]
fn a_short_circuited_operand_is_not_evaluated_when_the_left_decides() {
    let src = program(
        "\
int main(void)
{
	int taken = 0;

	/* `&&`: the right operand must not run, because 0 decides it. */
	if (0 && [makeThing(1) n] > 0) {
		taken++;
	}
	/* `||`: the right operand must not run, because 1 decides it. */
	if (1 || [makeThing(2) n] > 0) {
		taken++;
	}
	printf(\"taken=%d\\n\", taken);
	return 0;
}
",
    );
    let out = compile_and_run_strict(&src, "comma_short_circuit");
    assert_eq!(
        out, "taken=1\n",
        "neither short-circuited operand may be evaluated -- a `made` line here means the \
         allocation was hoisted out of the operand it belongs to; got:\n{}",
        out
    );
}

/// The same question for a ternary: the arm not taken must not allocate.
///
/// This one was never a leak -- it was hoisted and released, so the
/// refcounts balanced -- which is why it survived an audit that counted
/// them. It still allocated on a branch the source does not take.
#[test]
fn an_untaken_ternary_arm_does_not_allocate() {
    let src = program(
        "\
int main(void)
{
	int x = 0;
	int n = x ? [makeThing(7) n] : 99;

	printf(\"n=%d\\n\", n);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "comma_ternary"),
        "n=99\n",
        "the untaken arm must not allocate"
    );
}

/// ...and the arm that *is* taken allocates exactly once and releases it.
#[test]
fn a_taken_ternary_arm_allocates_once_and_releases() {
    let src = program(
        "\
int main(void)
{
	int i;

	for (i = 0; i < 3; i++) {
		int n = (i >= 0) ? [makeThing(i) n] : 99;
		printf(\"n=%d\\n\", n);
	}
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run_strict(&src, "comma_ternary_taken"),
        "made 0\nn=0\nmade 1\nn=1\nmade 2\nn=2\n",
        "one allocation per iteration, each released so the next finds the slot free"
    );
}

/// A plain C call as the consumer, with no send anywhere.
///
/// `while (borrow(makeThing()) > 100)` had no temporary at all: the
/// C-call argument arm is reached only from the statement positions, so a
/// call in a loop condition asked nobody. The comma form covers it
/// because it keys on the *consumer*, not on whether the consumer is a
/// send.
#[test]
fn a_plus_one_in_a_c_call_inside_a_loop_condition_is_released() {
    let src = program(
        "\
int main(void)
{
	int i = 0;

	while (i < 3 && borrow(makeThing(i)) >= 0) {
		i++;
	}
	printf(\"i=%d\\n\", i);
	return 0;
}
",
    );
    /* Three evaluations, not four: `i < 3` decides the fourth pass and
       the `&&` short-circuits before the call, so the argument is never
       built. The loop still ends at i=3 -- proof the left operand ended
       it rather than the object being nil. */
    assert_eq!(
        compile_and_run_strict(&src, "comma_c_call"),
        "made 0\nmade 1\nmade 2\ni=3\n",
        "the call's argument must be released per evaluation, and not evaluated at all on \
         the pass the left operand decides"
    );
}

/// A `void`-valued send in a conditionally-evaluated position, where the
/// comma expression has no value temporary and yields the release itself.
///
/// Reachable only in a ternary arm -- `x && [t poke]` is not valid C,
/// since `void` has no truth value -- and worth a case because the
/// value-temporary path would declare a `void` variable if it were taken
/// here.
#[test]
fn a_void_send_needs_no_value_temporary() {
    let src = program(
        "\
int main(void)
{
	int x = 1;

	x ? [makeThing(5) poke] : (void)0;
	printf(\"done\\n\");
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("void _oz_cv_"),
        "a void-valued send must synthesize no value temporary; got:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "comma_void"), "made 5\npoke 5\ndone\n");
}

/// The boundary on the other side: a `for` **initialiser** runs exactly
/// once, so it must keep the statement-level hoist and must *not* get the
/// comma form.
///
/// `for_header_owning_operands` exists precisely because that position is
/// safe to lift, and `conditionally_evaluated` names it as the one part of
/// a `for` header it does not claim. Asserted on the emitted text, because
/// both shapes behave identically here -- only the emission differs, and a
/// predicate that over-claimed would be invisible in a run.
#[test]
fn a_for_initialisers_operand_is_still_hoisted_not_inlined() {
    let src = program(
        "\
int main(void)
{
	int i;

	for (i = borrow(makeThing(1)); i < 2; i++) {
		printf(\"i=%d\\n\", i);
	}
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("_oz_ce_"),
        "a `for` initialiser runs once and keeps the hoist; got:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "comma_for_init"), "made 1\ni=1\n");
}

/// And an ordinary statement position keeps the statement hoist, with the
/// retain that shape needs.
///
/// This is the #375 shape, which must be untouched: the two mechanisms are
/// exclusive, and `collect_owning_operands_in` declining an operand is
/// what makes them so. A predicate that claimed too much would move this
/// case to the comma form and lose the retain, putting the use-after-free
/// back.
#[test]
fn a_statement_position_keeps_its_hoist_and_its_retain() {
    let src = program(
        "\
@interface Holder : OZObject
- (Thing *)take:(Thing *)t;
@end
@implementation Holder
- (Thing *)take:(Thing *)t
{
	return t;
}
@end

int main(void)
{
	Holder *h = [[Holder alloc] init];
	Thing *z = [h take:makeThing(41)];

	printf(\"z=%d\\n\", [z n]);
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        !out.source_c.contains("_oz_ce_"),
        "an ordinary statement must keep the statement-level hoist; got:\n{}",
        out.source_c
    );
    assert!(
        out.source_c.contains("oz_static_retain((struct OZObject *)(z))"),
        "and the retain that keeps #375 fixed; got:\n{}",
        out.source_c
    );
    assert_eq!(compile_and_run_strict(&src, "comma_stmt_untouched"), "made 41\nz=41\n");
}

/// Two `+1` operands in one conditionally-evaluated send, released in
/// reverse so a nested one outlives what was built from it.
#[test]
fn two_operands_in_one_expression_are_both_released() {
    let src = program(
        "\
@interface Adder : OZObject
- (int)add:(Thing *)a to:(Thing *)b;
@end
@implementation Adder
- (int)add:(Thing *)a to:(Thing *)b
{
	return [a n] + [b n];
}
@end

int main(void)
{
	Adder *s = [[Adder alloc] init];
	int i;

	for (i = 0; i < 3; i++) {
		if (i >= 0 && [s add:makeThing(1) to:makeThing(2)] == 3) {
			printf(\"sum ok\\n\");
		}
	}
	return 0;
}
",
    );
    let src = format!("/* oz-pool: Thing=2 */\n{}", src);
    assert_eq!(
        compile_and_run_strict(&src, "comma_two_operands"),
        "made 1\nmade 2\nsum ok\nmade 1\nmade 2\nsum ok\nmade 1\nmade 2\nsum ok\n",
        "both operands must be released each iteration, or the second pass finds no slot"
    );
}
