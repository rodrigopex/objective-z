// SPDX-License-Identifier: Apache-2.0
//
// message_receiver_shapes.rs -- a send's selector is the same selector
// whatever shape its receiver has (#435).
//
// `staticbar::message_selector` found the receiver by taking the first
// `identifier` child and skipping it. That is only correct when the
// receiver *is* a bare identifier. In `[self->_ivar foo]`, `[arr[0] foo]`,
// `[[Thing alloc] foo]` and `[(Thing *)t foo]` it is a `field_expression`,
// `subscript_expression`, `message_expression` or `cast_expression`, so the
// skip consumed the *selector's* identifier instead and the function
// returned `""`.
//
// #435 filed this as latent -- "its two live callers happen not to be hurt
// today" -- and enumerating them found that wrong in both directions, which
// is what these two tests are. There are three callers, not two:
//
//   - `collect.rs`'s reflection walk, which is **not** latent. An empty
//     selector is not in `PERFORM_SELECTORS`, so the site does not set
//     `uses_perform_selector`, so `companion` never emits `oz_perform`
//     (`companion.rs:1127`) while `emit` still renders the call to it --
//     the undeclared-helper shape `companion.rs`'s own comment describes.
//   - `staticbar.rs`'s `is_conforms_to_protocol_argument`, also not latent:
//     `@protocol(...)` is accepted *only* as that selector's argument, so
//     an empty selector turns a legal program into a located error.
//   - `pools.rs`'s `class_method_callee`, which genuinely was safe -- and
//     for a reason worth keeping: it had written the correct extraction
//     inline, a third copy of one question, and bailed on a receiver that
//     is not a known class before ever asking for the selector.
//
// Both live symptoms are below. The fix routes every spelling through
// `emit::parse_message`, which is the "one question, one answer" shape --
// and the reason the duplication survived is that two of the three copies
// were right.

mod common;
use common::{compile_and_run_with_reflection, compile_and_run_with_introspection,
             ozobject_src as PREAMBLE};

/// `-performSelector:` on a `self->_ivar` receiver reaches the method.
///
/// The failure this pins is in *generated C*, not in a diagnostic: with the
/// selector coming back `""` the program is accepted, `oz_perform` is never
/// emitted, and the call `emit` rendered to it has no declaration. So a
/// test that only transpiled would pass. This one compiles and runs.
#[test]
fn perform_selector_on_an_ivar_receiver_reaches_the_method() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
static int g_ticks = 0;

@interface Target : OZObject
- (void)tick;
@end
@implementation Target
- (void)tick {
	g_ticks = g_ticks + 1;
}
@end

@interface Driver : OZObject {
	Target *_target;
}
- (id)init;
- (void)drive;
@end
@implementation Driver
- (id)init {
	self = [super init];
	if (self != nil) {
		_target = [Target alloc];
	}
	return self;
}
/* The receiver is a field_expression, which is the whole point. */
- (void)drive {
	[self->_target performSelector:@selector(tick)];
}
@end

#include <stdio.h>
int main(void) {
	Driver *d = [[Driver alloc] init];
	[d drive];
	[d drive];
	printf(\"ticks=%d\\n\", g_ticks);
	return 0;
}
"
    );
    let out = compile_and_run_with_reflection(&src, "perform_on_ivar_receiver");
    assert_eq!(
        out, "ticks=2\n",
        "a performSelector: site with a non-identifier receiver must register as one, so \
         oz_perform is emitted; got:\n{}",
        out
    );
}

/// `-conformsToProtocol:` on a `self->_ivar` receiver still accepts
/// `@protocol(...)` as its argument.
///
/// `@protocol(...)` is legal in exactly one position, and the check that
/// recognises that position asked `message_selector` which selector it was.
/// With the answer `""` the position was unrecognisable and a correct
/// program became: "'@protocol(...)' is accepted only as the argument of
/// '-conformsToProtocol:'" -- pointing at source that *is* that argument.
///
/// `px-keyboard` writes this shape: `PXLEDController` holds its indicator
/// in an ivar and asks whether it is dimmable. It is spelled there without
/// the `self->`, which is an `identifier` and so happened to work.
#[test]
fn conforms_to_protocol_accepts_a_protocol_literal_on_an_ivar_receiver() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@protocol Dimmable <OZObjectProtocol>
- (void)setLevel:(int)level;
@end

@interface Lamp : OZObject <Dimmable>
- (void)setLevel:(int)level;
@end
@implementation Lamp
- (void)setLevel:(int)level {
	(void)level;
}
@end

@interface Holder : OZObject {
	Lamp *_indicator;
}
- (id)init;
- (int)dimmable;
@end
@implementation Holder
- (id)init {
	self = [super init];
	if (self != nil) {
		_indicator = [Lamp alloc];
	}
	return self;
}
/* field_expression receiver, and a protocol literal in the one position
 * that accepts one. */
- (int)dimmable {
	return [self->_indicator conformsToProtocol:@protocol(Dimmable)];
}
@end

#include <stdio.h>
int main(void) {
	Holder *h = [[Holder alloc] init];
	printf(\"dimmable=%d\\n\", [h dimmable]);
	return 0;
}
"
    );
    let out = compile_and_run_with_introspection(&src, "conforms_on_ivar_receiver");
    assert_eq!(
        out, "dimmable=1\n",
        "@protocol(...) is legal as -conformsToProtocol:'s argument however the receiver is \
         spelled; got:\n{}",
        out
    );
}
