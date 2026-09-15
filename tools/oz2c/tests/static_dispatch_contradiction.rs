// SPDX-License-Identifier: Apache-2.0
//
// static_dispatch_contradiction.rs -- when the emitter resolves a
// receiver's class and `arc` does not, the ownership answer came from a
// poll over classes the send can never reach. That is a **contradiction**,
// not an ambiguity, and it is refused with a located error (#483).
//
// #481 and #502 were two instances of one shape, each closed by teaching
// `arc` one more binding form: a method parameter, then a for-in loop
// variable. #483 is the argument that the *class* is unbounded -- the
// design requires two enumerations to track each other and nothing checks
// that they do -- and Rodrigo chose option 4 from it: keep two resolvers,
// and refuse rather than guess.
//
// **Three instances were live when this landed**, all measured on
// `main` at 03db303 (i.e. with #502's fix in), each reading `deallocs=0`
// against an expected 1:
//
//   * an **ivar** receiver          -- `[_owner build]`
//   * a **file-scope object** receiver -- `[g_owner build]`
//   * a **cast** receiver           -- `[(Owner *)raw build]`
//
// **None of the three is a node kind**, which is why the cheapest option
// #483 lists -- a gate enumerating the kinds in
// `arc::collect_declared_types`'s match -- was measured incapable. An ivar
// is declared in the `@interface` and a file-scope variable at top level,
// both *outside* the `method_definition`/`function_definition` scope
// `arc::declared_class_of`'s walk starts from; and a cast receiver is not
// an `identifier` at all, so `message_target` never asks for a
// declaration. The emitter resolves all three anyway -- ivars and
// file-scope vars are seeded into `ctx.scope`, and a cast carries its own
// type -- and emitted a direct call while carrying a polled answer.
//
// **Deliberately not built on the for-in shape.** That is the trap this
// file exists to avoid: #503 teaches `arc` the for-in binding, so once it
// landed that shape stopped being a contradiction and a guard tested on it
// would have passed while asserting nothing. It appears below only as a
// *control* that it is still accepted, where a silent stop means a
// failure rather than a pass.
//
// The controls matter as much as the refusals: a refusal that fires
// unconditionally is decoration. `an_ivar_receiver_with_one_implementor_*`
// is the load-bearing one -- the identical ivar shape, one implementor, no
// disagreement -- and it must still compile and still release.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// `Thing` counts its own deallocations, which is the oracle for every
/// accepting case here. A grep for `oz_release` in the output is not: this
/// repo has five recorded cases of a text-absence claim holding while the
/// property was gone.
const THING: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag {
	return 7;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end
";

/// `Owner` hands back a `+1`; `Lender` hands back what it keeps. Two
/// classes, one non-create-rule selector, **disagreeing** -- which is what
/// makes the implementor poll ambiguous.
///
/// Neither is a subclass of the other, so `has_overriding_subclass` is
/// false and the emitter still emits a *direct* call: that is the whole
/// reason the existing `Ambiguous` refusal, which guards the `class_id`
/// switch, never saw these sends.
const DISAGREEING: &str = "\
@interface Owner : OZObject
- (Thing *)build;
@end
@implementation Owner
- (Thing *)build {
	return [[Thing alloc] init];
}
@end

@interface Lender : OZObject
{
	Thing *_held;
}
- (Thing *)build;
@end
@implementation Lender
- (Thing *)build {
	return _held;
}
@end
";

/// Only `Owner` declares `-build`, so the poll is unanimous and the
/// receiver's class is never needed.
const AGREEING: &str = "\
@interface Owner : OZObject
- (Thing *)build;
@end
@implementation Owner
- (Thing *)build {
	return [[Thing alloc] init];
}
@end
";

fn program(classes: &str, tail: &str) -> String {
    format!(
        "/* oz-pool: Thing=4,Owner=2,Lender=2,P=1 */\n{}{}{}{}",
        PREAMBLE(),
        THING,
        classes,
        tail
    )
}

/// Every refusal has to name the selector, both sides of the
/// disagreement, and why a poll ran at all.
fn assert_refusal(diags: &str, selector: &str) {
    for needle in [
        selector,
        "dispatched directly here",
        "'Owner' hands back a reference the caller must release",
        "'Lender' hands back one it keeps owning",
        "resolved for dispatch but not for ownership",
    ] {
        assert!(
            diags.contains(needle),
            "refusal should mention {:?}, got:\n{}",
            needle,
            diags
        );
    }
}

// ---------------------------------------------------------------------------
// The three live contradictions, now refused
// ---------------------------------------------------------------------------

/// An **ivar** receiver. `render_method_definition` does
/// `ctx.scope = ivars_scope.clone()`, so the emitter resolves `_owner` and
/// emits `Owner_build(...)`; `arc::declared_class_of` walks up to the
/// enclosing `method_definition` and scans *inside* it, where no
/// declaration of `_owner` exists -- the `@interface` is outside that
/// subtree entirely. Measured at `deallocs=0` before this refusal.
#[test]
fn an_ivar_receiver_is_refused_rather_than_leaked() {
    let src = program(
        DISAGREEING,
        "\
@interface P : OZObject
{
	Owner *_owner;
}
- (void)setup;
- (int)viaIvar;
@end
@implementation P
- (void)setup {
	_owner = [Owner alloc];
}
- (int)viaIvar {
	Thing *t = [_owner build];
	return [t tag];
}
@end

int main(void) {
	return 0;
}
",
    );
    assert_refusal(&expect_reject(&src), "build");
}

/// A **file-scope object** receiver. `emit::file_scope_vars` puts
/// `g_owner` into `ctx.scope` -- that is how a send to one resolves at all
/// -- and a top-level declaration is again outside the method scope
/// `arc::declared_class_of` searches. Measured at `deallocs=0`.
#[test]
fn a_file_scope_object_receiver_is_refused() {
    let src = program(
        DISAGREEING,
        "\
static Owner *g_owner;

@interface P : OZObject
- (int)viaFileScope;
@end
@implementation P
- (int)viaFileScope {
	Thing *t = [g_owner build];
	return [t tag];
}
@end

int main(void) {
	return 0;
}
",
    );
    assert_refusal(&expect_reject(&src), "build");
}

/// A **cast** receiver -- the px-keyboard shape, an object arriving as a
/// `void *` out of a `k_timer` slot and cast back at the send. The cast
/// carries the type, so the emitter resolves it; `message_target` only
/// asks for a declaration when the receiver is an `identifier`, and a
/// `cast_expression` is not one. Measured at `deallocs=0`.
#[test]
fn a_cast_receiver_is_refused() {
    let src = program(
        DISAGREEING,
        "\
@interface P : OZObject
- (int)viaCast:(void *)raw;
@end
@implementation P
- (int)viaCast:(void *)raw {
	Thing *t = [(Owner *)raw build];
	return [t tag];
}
@end

int main(void) {
	return 0;
}
",
    );
    assert_refusal(&expect_reject(&src), "build");
}

/// A **`super`** receiver -- a fourth instance, found by reading
/// `is_super_receiver` and then measured. Pinned deliberately, and it is
/// the one case here where the refusal is **stricter than it needs to
/// be**.
///
/// `super` is an `identifier` node, so `message_target` calls
/// `declared_class_of("super", ..)`, which finds no declaration of that
/// name and answers `None`. The emitter meanwhile keeps the send a direct
/// call by definition -- routing it through the `class_id` switch would
/// re-enter the override that issued it -- so the asymmetry holds and the
/// send was leaking before this.
///
/// **Unlike the three above, a correct answer is trivially available
/// here**: `super` is exactly the superclass of the enclosing
/// `@implementation`, with no cast and no collection element type to lie
/// about, so resolving it in `message_target` (via `enclosing_impl_class`
/// plus `ClassInfo::superclass`) would be sound in a way #502 measured
/// that the for-in binding is not. That is a widening of `arc`'s
/// resolution, which is precisely the direction #483 decided *not* to take
/// as the fix, so it is recorded here rather than done -- and note the
/// first `help` line the diagnostic offers does not apply to a `super`
/// send, only the second one does.
///
/// Nothing in the tree is affected: all 59 `[super ...]` sends in
/// `samples/`, `src/` and `tests/` are `init` (39), `dealloc` (12) or
/// `initWithDTSpec:` (5), and `is_initialiser` answers `Borrowed` for an
/// initialiser before the poll is ever reached.
///
/// **If a later change resolves a `super` receiver, this test fails.**
/// Delete it then, and say so -- do not relax the assertion.
#[test]
fn a_super_receiver_is_refused_although_it_is_resolvable() {
    let src = format!(
        "/* oz-pool: Thing=4,Base=2,Sub=2,Lender=2 */\n{}{}{}",
        PREAMBLE(),
        THING,
        "\
@interface Base : OZObject
- (Thing *)build;
@end
@implementation Base
- (Thing *)build {
	return [[Thing alloc] init];
}
@end

@interface Lender : OZObject
{
	Thing *_held;
}
- (Thing *)build;
@end
@implementation Lender
- (Thing *)build {
	return _held;
}
@end

@interface Sub : Base
- (int)viaSuper;
@end
@implementation Sub
- (int)viaSuper {
	Thing *t = [super build];
	return [t tag];
}
@end

int main(void) {
	return 0;
}
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("is dispatched directly here")
            && diags.contains("'Base' hands back a reference the caller must release"),
        "a `super` receiver is unresolved in `arc`, so the poll runs and disagrees:\n{}",
        diags
    );
}

// ---------------------------------------------------------------------------
// Controls: what the refusal must NOT reach
// ---------------------------------------------------------------------------

/// **The load-bearing control.** The identical ivar receiver, with only
/// one class declaring `-build`: the poll is unanimous, `arc` needs no
/// class, and the send must still compile and still release.
///
/// Without this the refusal could be keyed on the *shape* -- "an ivar
/// receiver is refused" -- which would reject ordinary code. It is keyed
/// on the disagreement.
#[test]
fn an_ivar_receiver_with_one_implementor_is_not_refused() {
    let src = program(
        AGREEING,
        "\
@interface P : OZObject
{
	Owner *_owner;
}
- (void)setup;
- (int)viaIvar;
@end
@implementation P
- (void)setup {
	_owner = [Owner alloc];
}
- (int)viaIvar {
	Thing *t = [_owner build];
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	[p setup];
	int v = [p viaIvar];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_ivar_one_impl"),
        "v=7 deallocs=1\n",
        "one implementor makes the poll unanimous, so the ivar shape alone must not refuse"
    );
}

/// The second control on the same axis: an ivar receiver, both disagreeing
/// implementors present in the program, but the *selector* returns
/// `void`. No implementation of `-poke` is in `OwningMethods` at all
/// (`returns_object_pointer` keeps it out), so the poll answers `Borrowed`
/// rather than `Ambiguous` and ownership is meaningless anyway.
///
/// Refusing here would reject exactly the polymorphism the language is
/// for.
#[test]
fn a_void_selector_on_an_ivar_receiver_is_not_refused() {
    let src = format!(
        "/* oz-pool: Thing=4,Owner=2,Lender=2,P=1 */\n{}{}{}{}",
        PREAMBLE(),
        THING,
        DISAGREEING,
        "\
static int g_pokes = 0;

@interface Owner (Poking)
- (void)poke;
@end
@implementation Owner (Poking)
- (void)poke {
	g_pokes = g_pokes + 1;
}
@end

@interface Lender (Poking)
- (void)poke;
@end
@implementation Lender (Poking)
- (void)poke {
	g_pokes = g_pokes + 100;
}
@end

@interface P : OZObject
{
	Owner *_owner;
}
- (void)setup;
- (void)run;
@end
@implementation P
- (void)setup {
	_owner = [Owner alloc];
}
- (void)run {
	[_owner poke];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	[p setup];
	[p run];
	printf(\"pokes=%d deallocs=%d\\n\", g_pokes, g_deallocs);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_void_selector"),
        "pokes=1 deallocs=0\n",
        "a void selector carries no ownership, so a disagreement cannot exist to refuse"
    );
}

/// A **local** receiver, which `arc` has always resolved. The control that
/// says this is about the resolution asymmetry rather than about the
/// disagreement.
#[test]
fn a_local_receiver_still_compiles_and_releases() {
    let src = program(
        DISAGREEING,
        "\
@interface P : OZObject
- (int)viaLocal;
@end
@implementation P
- (int)viaLocal {
	Owner *o = [Owner alloc];
	Thing *t = [o build];
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	int v = [p viaLocal];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_local"),
        "v=7 deallocs=1\n",
        "a local receiver resolves in both, so the refusal must not reach it"
    );
}

/// A **method parameter** receiver -- #481's shape. `arc` has known
/// `method_parameter` since then, so this resolves in both and must stay
/// accepted.
///
/// A regression guard pointing backwards: if #481's one-token fix were
/// ever reverted, this control turns red rather than the whole shape
/// quietly becoming a build error.
#[test]
fn a_method_parameter_receiver_still_compiles_and_releases() {
    let src = program(
        DISAGREEING,
        "\
@interface P : OZObject
- (int)viaParameter:(Owner *)o;
@end
@implementation P
- (int)viaParameter:(Owner *)o {
	Thing *t = [o build];
	return [t tag];
}
@end

#include <stdio.h>
int main(void) {
	Owner *o = [Owner alloc];
	P *p = [P alloc];
	int v = [p viaParameter:o];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_parameter"),
        "v=7 deallocs=1\n",
        "#481 taught `arc` `method_parameter`; this shape must not become a refusal"
    );
}

/// A **`self`** receiver, which `message_target` resolves through
/// `enclosing_impl_class`. Included because `self` is the most common
/// receiver in the corpus and a refusal reaching it would reject
/// essentially every program.
#[test]
fn a_self_receiver_still_compiles_and_releases() {
    let src = program(
        DISAGREEING,
        "\
@interface Owner (Twice)
- (int)twice;
@end
@implementation Owner (Twice)
- (int)twice {
	Thing *t = [self build];
	return [t tag] + [t tag];
}
@end

#include <stdio.h>
int main(void) {
	Owner *o = [Owner alloc];
	int v = [o twice];
	printf(\"v=%d deallocs=%d\\n\", v, g_deallocs);
	return 0;
}
",
    );
    assert_eq!(
        compile_and_run(&src, "static_contra_self"),
        "v=14 deallocs=1\n",
        "a `self` receiver resolves through `enclosing_impl_class` and must stay accepted"
    );
}
