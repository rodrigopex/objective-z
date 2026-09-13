// SPDX-License-Identifier: Apache-2.0
//
// call_result_receiver.rs -- a message sent to the result of a call
// resolves against the callee's declared return type (#355).
//
// `render_expr` had no `call_expression` arm at all, so a call fell to
// the default one and was typed `id` -- which no receiver resolution can
// use. Binding the result to a local first worked, and sending to a
// *method* result worked, so the information was plainly available; the
// callee's own signature said `Thing *` the whole time and nothing read
// it.
//
// Two diagnostics came out of the one cause, which is worth recording
// because the issue quotes the first and the second is stranger:
//
//   - on its own, "cannot statically resolve the receiver type for
//     selector 'poke' (receiver type is 'id')";
//   - in a statement that *also* hoists a `+1` operand, "class 'OZObject'
//     has no method matching 'poke'" -- naming a class the source never
//     mentions, because `render_owning_operand_statement` reads a
//     non-pointer type as "some object, cast it to the root pointer" and
//     the send then resolves against the root class.
//
// So the missing type did not merely refuse the code, it refused it while
// pointing somewhere else entirely.
//
// The fix is `collect::function_return_types`, which records prototypes
// as well as definitions -- a function declared in a header and defined
// in a plain `.c` compiled alongside has no definition in this
// translation unit and its declared return type is no less definite for
// that.

mod common;
use common::{compile_and_run_strict, expect_reject, ozobject_src};

const PRELUDE: &str = "\
#include <stdio.h>

@interface Thing : OZObject {
	int _n;
}
- (id)initWithN:(int)n;
- (int)n;
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
@end

Thing *makeThing(int n)
{
	return [[Thing alloc] initWithN:n];
}

Thing *borrowThing(Thing *t)
{
	return t;
}
";

fn program(body: &str) -> String {
    format!("{}{}\n{}", ozobject_src(), PRELUDE, body)
}

/// Same, with room for `slots` live `Thing`s at once.
///
/// A slab holds one slot per *allocation site* by default (see `pools`),
/// and `makeThing` contains exactly one -- so a program that legitimately
/// holds two `Thing`s at the same time has to say so. Worth spelling out
/// because the first version of the test below did not, and its loop
/// allocations came back `nil` while a long-lived local sat on the only
/// slot: correct behaviour, misread as a failure.
fn program_with_pool(slots: usize, body: &str) -> String {
    format!("/* oz-pool: Thing={} */\n{}{}\n{}", slots, ozobject_src(), PRELUDE, body)
}

/// The case the issue was filed on, in both ownership directions.
///
/// `makeThing()` hands back `+1`, so the send's receiver is a temporary
/// that has to be released once; `borrowThing()` hands back a reference
/// it does not own, so nothing must be released. Both are the same
/// receiver *shape* and they must not get the same ownership answer,
/// which is why they are asserted together -- one of them passing tells
/// you nothing about the other.
///
/// Three `makeThing` calls on one slab slot, so the run is also the leak
/// check: the release is what makes the third succeed.
#[test]
fn a_message_to_a_call_result_resolves_and_is_released_once() {
    let src = program_with_pool(
        2,
        "\
int main(void)
{
	Thing *held = makeThing(5);
	int i;

	for (i = 1; i <= 3; i++) {
		printf(\"owning=%d\\n\", [makeThing(i) n]);
	}
	printf(\"borrowed=%d\\n\", [borrowThing(held) n]);
	return 0;
}
",
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("Thing_n((struct Thing *)(borrowThing(held)))"),
        "a borrowed call result must be sent to directly, with no temporary and no release; \
         got:\n{}",
        out.source_c
    );
    assert_eq!(
        compile_and_run_strict(&src, "call_result_receiver"),
        "owning=1\nowning=2\nowning=3\nborrowed=5\n"
    );
}

/// A **prototype** is as good as a definition.
///
/// The function is declared here and defined below `main`, so at the send
/// the only thing in scope is the prototype. Recording definitions alone
/// would have left this refused, and it is the ordinary arrangement for
/// anything declared in a header.
#[test]
fn a_prototype_is_enough_to_resolve_the_receiver() {
    let src = program(
        "\
Thing *later(int n);

int main(void)
{
	printf(\"n=%d\\n\", [later(9) n]);
	return 0;
}

Thing *later(int n)
{
	return [[Thing alloc] initWithN:n];
}
",
    );
    assert_eq!(compile_and_run_strict(&src, "call_result_prototype"), "n=9\n");
}

/// A call through something that is **not** a plain function name keeps
/// the old `id`, and so is still refused rather than guessed at.
///
/// A function pointer's result has no declared return type this pass can
/// read -- `collect::declares_function` deliberately stops at the
/// `parenthesized_declarator` that makes `Thing *(*fp)(void)` a variable
/// -- and inventing one would be worse than the refusal. The diagnostic
/// is the honest one, and the message is checked because the point of the
/// case is *which* refusal it gets.
#[test]
fn a_call_through_a_function_pointer_is_still_refused() {
    let src = program(
        "\
int main(void)
{
	Thing *(*fp)(int) = makeThing;

	return [fp(1) n];
}
",
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("cannot statically resolve the receiver type"),
        "expected the unresolvable-receiver diagnostic; got:\n{}",
        err
    );
}

/// A local shadows a file-scope function of the same name in C, so its
/// call is not that function's call.
///
/// Cheap, and it keeps the lookup from ever being the *wrong* answer
/// rather than merely a missing one -- the failure mode that matters,
/// since a wrong receiver type resolves to a real class and emits a real
/// call to the wrong function. Here the shadowing local returns an `int`,
/// so without the `ctx.locals` check the send would resolve against
/// `makeThing`'s `Thing *` and hand `Thing_n` an integer.
///
/// The assertion is that it is **refused**, not which refusal it gets,
/// and that is deliberate. `arc`'s owning-function lookup is by *name*
/// and knows nothing about shadowing, so it still treats `makeThing(1)`
/// as `+1` and hoists it -- which types the temporary from the root class
/// and produces the root-class diagnostic instead of the receiver one.
/// Both are hard errors and the program is rejected either way. Worth
/// recording rather than fixing: a local shadowing a function name with a
/// different signature is pathological C, and making `arc` scope-aware
/// for it would mean threading locals through an analysis that
/// deliberately has none.
#[test]
fn a_local_shadowing_a_function_does_not_borrow_its_return_type() {
    let src = program(
        "\
int main(void)
{
	int (*makeThing)(int) = 0;

	return [makeThing(1) n];
}
",
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("cannot statically resolve the receiver type")
            || err.contains("has no method matching 'n'"),
        "a local shadowing the function must not take its return type -- the program has to \
         be refused, either way round; got:\n{}",
        err
    );
}
