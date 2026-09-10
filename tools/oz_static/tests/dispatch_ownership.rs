// SPDX-License-Identifier: Apache-2.0
//
// dispatch_ownership.rs -- what a send decides when the implementation that
// runs is not known until run time (#361, #365).
//
// `emit::dynamic_dispatch_call` routes a send through the `_meta.class_id`
// switch in two situations, and ownership was decided wrongly in both:
//
//   - the receiver pins nothing down -- a bare `id`, or a
//     protocol-qualified one. Ownership was `+0`, which **leaks** whenever
//     the implementation that runs returns `+1` (#361).
//   - the receiver's class is known but a subclass overrides the selector.
//     Ownership came from the **static** class, which **over-releases**
//     whenever the override disagrees (#365) -- a use-after-free, the
//     direction `docs/STATUS.md` forbids.
//
// One question over two different sets, so one answer:
// `arc::dispatch_ownership` polls every implementation the send can reach
// (`Program::reachable_implementors`) and requires them to agree. Unanimous
// `+1` is `+1`; unanimous `+0` is borrowed; a disagreement is a located
// error, because no caller can be correct for both -- a `+1` result must be
// released exactly once and a `+0` one never.
//
// Why not retain-when-unprovable, the trick #351 uses at a `return`: there,
// retaining creates a *new* reference the caller can own, which makes the
// unknown irrelevant. Here the question is whether an *existing* reference
// was handed over, and adding a retain shifts both cases by one, leaving
// the difference exactly where it was. Clang cannot answer it either --
// measured, a protocol send, a `+1` class send and a `+0` class send all
// carry the identical `ARCReclaimReturnedObject`, because ARC's callee
// autoreleases and its caller always reclaims, which needs the pool this
// target does not have.
//
// The two runtime tests take the protocol-typed receiver as a *method*
// parameter rather than a plain C function's. A protocol-qualified `id` is
// lowered to `void *` in a method's signature and copied through verbatim
// in a free function's, where it is not valid C -- a separate defect, of
// the same family as #336, and using the spelling that compiles is what
// keeps these tests about ownership.
//
// The refusal is scoped to object-returning selectors. Ownership is
// meaningless for a `void` or scalar result, and refusing those would
// reject ordinary polymorphism -- a `-poke` overridden by three subclasses
// is what dynamic dispatch is *for*.

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
    format!(
        "/* oz-pool: Thing=6,Maker=2,Sub=2,Sup=2,Caller=2,Keeper=2,Borrower2=2,Mid=2 */\n{}{}\n{}",
        PREAMBLE(),
        THING,
        body
    )
}

/// A unanimous `+1` reached through a protocol is released by the caller.
/// Before the fix nothing released it, so every call leaked.
#[test]
fn a_unanimous_owning_protocol_send_is_released() {
    let src = program(
        "\
#include <stdio.h>

@protocol Supplier
- (Thing *)supply;
@end

@interface Maker : OZObject
@end
@implementation Maker
- (Thing *)supply
{
	return [[Thing alloc] initWithTag:7];
}
@end

@interface Caller : OZObject
- (void)useSupplier:(id<Supplier>)s;
@end
@implementation Caller
- (void)useSupplier:(id<Supplier>)s
{
	Thing *t = [s supply];

	printf(\"tag=%d deallocs=%d\\n\", [t tag], g_deallocs);
}
@end

int main(void)
{
	Maker *m = [[Maker alloc] init];
	Caller *c = [[Caller alloc] init];

	[c useSupplier:m];
	printf(\"after the scope deallocs=%d\\n\", g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "dispatch_unanimous_owning");
    assert_eq!(
        stdout, "tag=7 deallocs=0\nafter the scope deallocs=1\n",
        "the caller owns a unanimous +1 and must release it exactly once"
    );
}

/// A unanimous `+0` stays borrowed -- the caller must not release what the
/// implementor keeps owning. This is what every object-returning protocol
/// selector in the tree actually is (`-next`, `-iter`, `+sharedInstance`),
/// so it is the case that must not regress.
#[test]
fn a_unanimous_borrowing_protocol_send_is_left_alone() {
    let src = program(
        "\
#include <stdio.h>

@protocol Lender
- (Thing *)lend;
@end

@interface Keeper : OZObject {
	Thing *_held;
}
- (id)init;
- (int)heldTag;
@end
@implementation Keeper
- (id)init
{
	self = [super init];
	if (self != nil) {
		_held = [[Thing alloc] initWithTag:4];
	}
	return self;
}
- (Thing *)lend
{
	return _held;
}
- (int)heldTag
{
	return [_held tag];
}
@end

@interface Borrower2 : OZObject
- (void)borrow:(id<Lender>)l;
@end
@implementation Borrower2
- (void)borrow:(id<Lender>)l
{
	Thing *t = [l lend];

	printf(\"borrowed=%d deallocs=%d\\n\", [t tag], g_deallocs);
}
@end

int main(void)
{
	Keeper *k = [[Keeper alloc] init];
	Borrower2 *b = [[Borrower2 alloc] init];

	[b borrow:k];
	/* Still the keeper's: releasing it in `borrow` would have freed it. */
	printf(\"still alive=%d deallocs=%d\\n\", [k heldTag], g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "dispatch_unanimous_borrowing");
    assert_eq!(stdout, "borrowed=4 deallocs=0\nstill alive=4 deallocs=0\n");
}

/// #365's shape, and the one that used to corrupt: the receiver's class is
/// known, a subclass overrides the selector, and the two disagree.
#[test]
fn a_subclass_override_that_disagrees_is_refused() {
    let src = program(
        "\
@interface Sup : OZObject
- (Thing *)give;
@end
@implementation Sup
- (Thing *)give
{
	return [[Thing alloc] initWithTag:1];
}
@end

@interface Sub : Sup {
	Thing *_kept;
}
@end
@implementation Sub
- (Thing *)give
{
	return _kept;
}
@end

void take(Sup *s)
{
	Thing *t = [s give];

	[t tag];
}
",
    );

    let diags = common::expect_reject(&src);
    assert!(
        diags.contains("disagree about ownership"),
        "expected the ambiguity refusal, got:\n{}",
        diags
    );
    for class in ["Sup", "Sub"] {
        assert!(diags.contains(class), "the message must name '{}':\n{}", class, diags);
    }
}

/// The same refusal through a protocol, where the disagreeing
/// implementations are unrelated classes rather than a class and its
/// subclass.
#[test]
fn protocol_implementors_that_disagree_are_refused() {
    let src = program(
        "\
@protocol TwoMinded
- (Thing *)supply;
@end

@interface Fresh : OZObject
@end
@implementation Fresh
- (Thing *)supply
{
	return [[Thing alloc] initWithTag:1];
}
@end

@interface Held : OZObject {
	Thing *_held;
}
@end
@implementation Held
- (Thing *)supply
{
	return _held;
}
@end

void consume(id<TwoMinded> s)
{
	Thing *t = [s supply];

	[t tag];
}
",
    );

    let diags = common::expect_reject(&src);
    assert!(
        diags.contains("disagree about ownership"),
        "expected the ambiguity refusal, got:\n{}",
        diags
    );
}

/// A `void`-returning selector overridden by a subclass is ordinary
/// polymorphism and must stay accepted. Ownership is meaningless for it,
/// so the refusal must not reach it -- this is the guard on the refusal's
/// blast radius, and it is the shape most code actually has.
#[test]
fn a_void_selector_overridden_by_a_subclass_is_untouched() {
    let src = program(
        "\
#include <stdio.h>

@interface Base : OZObject
- (void)poke;
@end
@implementation Base
- (void)poke
{
	printf(\"base\\n\");
}
@end

@interface Mid : Base
@end
@implementation Mid
- (void)poke
{
	printf(\"mid\\n\");
}
@end

int main(void)
{
	Base *b = [[Mid alloc] init];

	[b poke];
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "dispatch_void_override");
    assert_eq!(stdout, "mid\n", "a void override must dispatch dynamically and be accepted");
}
