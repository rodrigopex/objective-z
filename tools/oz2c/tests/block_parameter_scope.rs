// SPDX-License-Identifier: Apache-2.0
//
// block_parameter_scope.rs -- a block literal's declared parameter types
// are in scope for its body, so a message send to one resolves (#537).
//
// Before this, `emit::render_block` found its parameter list only to
// render the hoisted *signature text* and rendered the body with
// `collect_local_decls` alone -- which covers a body's `declaration`
// nodes and returns immediately on a `block_literal`. Nothing ever put a
// parameter in `ctx.scope`, so inside the body every parameter fell to
// `render_expr`'s `unwrap_or("id")` and a send to one was refused:
// "cannot statically resolve the receiver type for selector 'hasPrefix:'
// (receiver type is 'id')". The diagnostic's remedy -- cast it to that
// class at the send -- was right and told a reader who *had* declared the
// parameter to declare it.
//
// **This is the same omission as #250, one position over.** There the
// path that got a reduced version of what a method body gets was the
// plain C function; here it is the block literal. `free_function_params.rs`
// is that test file, and the seeding is now literally shared code
// (`seed_parameter_list`), so the two cannot drift again.
//
// The one thing a block parameter needs that a function's does not: it
// **shadows**, and then un-shadows. A literal is rendered on the
// *enclosing* body's `EmitCtx`, so the enclosing binding has to come back
// afterwards -- the same save/restore discipline `method_return_type`
// (#339) and `sync_cleanups` (#342) already use there.
// `an_enclosing_name_is_shadowed_and_restored` is what pins it, and it has
// teeth in both directions: without the seeding the block's send is
// refused, and without the restore the *enclosing* send after the literal
// dispatches to a function belonging to a class the object is not.
//
// Every case compiles and runs the generated C -- the #367 lesson, and
// the reason a text match is not the assertion here either.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src};

/// `Widget` and `Gadget` are siblings under `OZObject`, and each selector
/// is declared by exactly one of them -- so neither is
/// protocol-dispatchable and a send to an `id` receiver has nothing to
/// resolve against. That is what makes the refusal reproducible, and what
/// makes a mis-resolved receiver a *wrong call* rather than a harmless
/// one.
const CLASSES: &str = "\
@protocol Marker
- (int)marked;
@end

@interface Widget : OZObject {
	int _n;
}
- (id)initWithN:(int)n;
- (int)n;
- (void)bump;
@end
@implementation Widget
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
- (void)bump
{
	_n = _n + 1;
}
@end

@interface Gadget : OZObject <Marker> {
	int _g;
}
- (id)initWithG:(int)g;
- (int)g;
- (int)marked;
@end
@implementation Gadget
- (id)initWithG:(int)g
{
	self = [super init];
	if (self != nil) {
		_g = g;
	}
	return self;
}
- (int)g
{
	return _g;
}
- (int)marked
{
	return _g * 2;
}
@end
";

fn program(body: &str) -> String {
    format!(
        "/* oz-pool: Widget=4,Gadget=4 */\n{}{}\n{}",
        ozobject_src(),
        CLASSES,
        body
    )
}

/// The case #537 was filed on, in the shape px-app hit it
/// (`src/challenges/PXCaseBlocks.m:80`, WA-020): an object-taking block
/// sending an ordinary -- not protocol-declared -- selector to its own
/// parameter.
#[test]
fn a_send_to_a_class_typed_block_parameter_resolves() {
    let src = program(
        "\
#include <stdio.h>

int main(void)
{
	int (^read)(Widget *) = ^int(Widget *w) {
		return [w n];
	};
	Widget *w = [[Widget alloc] initWithN:21];

	printf(\"n=%d\\n\", read(w));
	return 0;
}
",
    );

    let out = oz2c::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("Widget_n("),
        "expected the send statically dispatched to Widget, got:\n{}",
        out.source_c.lines().filter(|l| l.contains("_n(")).collect::<Vec<_>>().join("\n")
    );

    let stdout = compile_and_run(&src, "a_send_to_a_class_typed_block_parameter_resolves");
    assert_eq!(stdout, "n=21\n");
}

/// A *mutating* send, so the parameter is not merely read through and the
/// hoisted function is acting on the caller's object.
#[test]
fn a_mutating_send_to_a_block_parameter_resolves() {
    let src = program(
        "\
#include <stdio.h>

int main(void)
{
	void (^bumpTwice)(Widget *) = ^(Widget *w) {
		[w bump];
		[w bump];
	};
	Widget *w = [[Widget alloc] initWithN:5];

	bumpTwice(w);
	printf(\"n=%d\\n\", [w n]);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "a_mutating_send_to_a_block_parameter_resolves");
    assert_eq!(stdout, "n=7\n");
}

/// The shadowing half, and the reason the seeding is saved and restored
/// rather than simply inserted.
///
/// `thing` is a `Gadget *` in `main` and a `Widget *` inside the literal,
/// and there is a send to each. Both directions have teeth:
///
///   - without the seeding, `[thing n]` inside the body is refused as an
///     `id` receiver -- the #537 symptom;
///   - without the restore, `[thing g]` *after* the literal is resolved
///     against `struct Widget *` and lowers to `Widget_g(...)`, a static
///     call into a class the object is not. That is silent wrong code, not
///     a diagnostic, which is the same hazard #505 recorded for a for-in
///     header.
#[test]
fn an_enclosing_name_is_shadowed_and_restored() {
    let src = program(
        "\
#include <stdio.h>

int main(void)
{
	Gadget *thing = [[Gadget alloc] initWithG:3];
	int (^read)(Widget *) = ^int(Widget *thing) {
		return [thing n];
	};
	Widget *w = [[Widget alloc] initWithN:4];

	printf(\"block=%d after=%d\\n\", read(w), [thing g]);
	return 0;
}
",
    );

    let out = oz2c::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("Gadget_g("),
        "the enclosing `thing` must resolve as a Gadget after the literal, got:\n{}",
        out.source_c.lines().filter(|l| l.contains("_g(")).collect::<Vec<_>>().join("\n")
    );
    assert!(
        !out.source_c.contains("Widget_g("),
        "the block's parameter type outlived the literal:\n{}",
        out.source_c.lines().filter(|l| l.contains("_g(")).collect::<Vec<_>>().join("\n")
    );

    let stdout = compile_and_run(&src, "an_enclosing_name_is_shadowed_and_restored");
    assert_eq!(stdout, "block=4 after=3\n");
}

/// A block parameter spelled `id<Proto>`, which is where #531 and #537
/// meet: the seeding routes the declared type through
/// `collect::render_type`, so without #531's normalization this would have
/// put the literal string `"id<Marker>"` into `ctx.scope` and a send to
/// the parameter would still have been unresolvable -- the symptom moved,
/// not fixed.
///
/// Lowered to `void *`, exactly as a bare `id` and as a method's
/// `id<Proto>` parameter are, so the send goes out through
/// `OZ_PROTOCOL_SEND_*` as a protocol-typed receiver should.
#[test]
fn a_protocol_qualified_block_parameter_dispatches_through_the_protocol() {
    let src = program(
        "\
#include <stdio.h>

int main(void)
{
	int (^read)(id<Marker>) = ^int(id<Marker> m) {
		return [m marked];
	};
	Gadget *g = [[Gadget alloc] initWithG:6];

	printf(\"marked=%d\\n\", read(g));
	return 0;
}
",
    );

    let out = oz2c::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("OZ_PROTOCOL_SEND_marked("),
        "expected the protocol-typed parameter dispatched dynamically, got:\n{}",
        out.source_c.lines().filter(|l| l.contains("marked")).collect::<Vec<_>>().join("\n")
    );

    let stdout =
        compile_and_run(&src, "a_protocol_qualified_block_parameter_dispatches_through_the_protocol");
    assert_eq!(stdout, "marked=12\n");
}

/// A **bare `id`** block parameter is still refused for an ordinary
/// selector, which is the property the fix must not trade away.
///
/// `id` carries no class, so there is nothing to resolve `-n` against and
/// nothing to fall back to -- `-n` is declared by `Widget` alone, so it is
/// not protocol-dispatchable. Refusing it is the standing rule ("it never
/// silently degrades"), and the author's remedy is to write the type,
/// which now works.
///
/// The refusal's *wording* moved, and towards parity rather than away
/// from it. It used to read "receiver type is 'id'" -- but only because
/// nothing had put the parameter in scope at all and `render_expr` fell to
/// its `unwrap_or("id")`. Now the parameter is in scope, lowered to
/// `void *` exactly as `collect_function_params` lowers a free function's
/// `id` and `render_method_definition` lowers a method's, both of which
/// have always reported `'void *'` here (measured, not assumed). The
/// block path was the odd one out; it now matches, and it picks up the
/// third `help:` line that the `void *` arm carries about an untyped
/// pointer.
#[test]
fn a_bare_id_block_parameter_is_still_refused_for_an_ordinary_selector() {
    let src = program(
        "\
int main(void)
{
	int (^read)(id) = ^int(id thing) {
		return [thing n];
	};
	Widget *w = [[Widget alloc] initWithN:1];

	return read(w);
}
",
    );

    let err = expect_reject(&src);
    assert!(
        err.contains("cannot statically resolve the receiver type")
            && err.contains("receiver type is 'void *'"),
        "expected the unresolvable-receiver refusal, got:\n{}",
        err
    );
}

/// A parameter name that was **not** bound before the literal must not
/// still be bound after it -- the `None` arm of the restore, which is a
/// removal and not a no-op.
///
/// `thing` exists only as the block's parameter. After the literal, a send
/// to that name resolves against nothing, so it is refused as an `id`
/// receiver: the same verdict a reference to an undeclared name has always
/// got here, and the evidence that the block's binding did not leak into
/// the enclosing body's scope map.
#[test]
fn a_block_parameter_does_not_leak_into_the_enclosing_scope() {
    let src = program(
        "\
int main(void)
{
	int (^read)(Widget *) = ^int(Widget *thing) {
		return [thing n];
	};
	Widget *made = [[Widget alloc] initWithN:2];
	id thing = made;

	return read(made) + [thing n];
}
",
    );

    let err = expect_reject(&src);
    assert!(
        err.contains("cannot statically resolve the receiver type"),
        "the block's parameter type outlived the literal -- `[thing n]` after it \
         resolved instead of being refused:\n{}",
        err
    );
}

/// Two literals in one body, each with a parameter of the same name and a
/// different type. Restoring per literal rather than once at the end is
/// what keeps the second from inheriting the first.
#[test]
fn two_literals_each_see_only_their_own_parameter() {
    let src = program(
        "\
#include <stdio.h>

int main(void)
{
	int (^readWidget)(Widget *) = ^int(Widget *x) {
		return [x n];
	};
	int (^readGadget)(Gadget *) = ^int(Gadget *x) {
		return [x g];
	};
	Widget *w = [[Widget alloc] initWithN:11];
	Gadget *g = [[Gadget alloc] initWithG:13];

	printf(\"w=%d g=%d\\n\", readWidget(w), readGadget(g));
	return 0;
}
",
    );

    let out = oz2c::transpile(&src).expect("should transpile");
    for needle in ["Widget_n(", "Gadget_g("] {
        assert!(
            out.source_c.contains(needle),
            "expected `{}`, so each literal saw its own `x`, got:\n{}",
            needle,
            out.source_c
                .lines()
                .filter(|l| l.contains("_n(") || l.contains("_g("))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    let stdout = compile_and_run(&src, "two_literals_each_see_only_their_own_parameter");
    assert_eq!(stdout, "w=11 g=13\n");
}

/// A literal inside a **method** body, not `main` -- the position that
/// already had ivars and method parameters in scope, so the parameter has
/// to be added to a populated scope rather than an empty one, and an ivar
/// of the same name has to come back afterwards.
#[test]
fn a_literal_in_a_method_body_shadows_an_ivar_and_restores_it() {
    let src = program(
        "\
#include <stdio.h>

@interface Runner : OZObject {
	Gadget *_held;
}
- (id)initWithHeld:(Gadget *)held;
- (int)run:(Widget *)w;
@end
@implementation Runner
- (id)initWithHeld:(Gadget *)held
{
	self = [super init];
	if (self != nil) {
		_held = held;
	}
	return self;
}
- (int)run:(Widget *)w
{
	int (^read)(Widget *) = ^int(Widget *_held) {
		return [_held n];
	};

	return read(w) + [_held g];
}
@end

int main(void)
{
	Gadget *g = [[Gadget alloc] initWithG:8];
	Runner *r = [[Runner alloc] initWithHeld:g];
	Widget *w = [[Widget alloc] initWithN:9];

	printf(\"run=%d\\n\", [r run:w]);
	return 0;
}
",
    );

    let out = oz2c::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("Gadget_g(") && out.source_c.contains("Widget_n("),
        "expected the parameter inside the literal and the ivar after it, got:\n{}",
        out.source_c
            .lines()
            .filter(|l| l.contains("_g(") || l.contains("_n("))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let stdout = compile_and_run(&src, "a_literal_in_a_method_body_shadows_an_ivar_and_restores_it");
    assert_eq!(stdout, "run=17\n");
}
