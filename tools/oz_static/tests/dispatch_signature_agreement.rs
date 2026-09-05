// SPDX-License-Identifier: Apache-2.0
//
// dispatch_signature_agreement.rs -- two classes cannot share a
// dynamically-dispatched selector name with incompatible return types
// (#290).
//
// `companion::render_protocol_dispatch` emits one `OZ_PROTOCOL_SEND_<sel>`
// per selector *name*, taking its signature from whichever implementor was
// declared first. Its own doc stated the assumption -- "every implementor of
// a given selector is expected to match it" -- and nothing checked it, so
// two classes sharing a name but not a return type produced a shim whose
// `case` arms disagreed with its own return type:
//
//     const struct alpha_spec* OZ_PROTOCOL_SEND_spec(struct OZObject *self)
//     {
//             switch (self->_meta.class_id) {
//             case OZ_STATIC_CLASS_Alpha: return Alpha_spec(...);
//             case OZ_STATIC_CLASS_Beta:  return Beta_spec(...);   /* wrong type */
//             }
//     }
//
// The only complaint came from GCC, about generated code the author never
// wrote. It is a located error now.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// The shape as filed: two unrelated classes, one selector name, two
/// pointer return types.
#[test]
fn two_classes_with_one_selector_and_different_pointer_returns_are_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
struct alpha_spec { int a; };
struct beta_spec { long b; };

@interface Alpha : OZObject
- (const struct alpha_spec *)spec;
@end
@interface Beta : OZObject
- (const struct beta_spec *)spec;
@end

@implementation Alpha
- (const struct alpha_spec *)spec { return (const struct alpha_spec *)0; }
@end
@implementation Beta
- (const struct beta_spec *)spec { return (const struct beta_spec *)0; }
@end
int main(void) { return 0; }
"
    );
    let diags = match oz_static::transpile(&src) {
        Err(diags) => diags,
        Ok(_) => panic!("a pointer-return collision must be rejected"),
    };
    let text = diags.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n");
    assert!(
        text.contains("alpha_spec") && text.contains("beta_spec"),
        "the diagnostic should name both return types, got:\n{}",
        text
    );
    assert!(
        text.contains("keyed on the selector name"),
        "the diagnostic should say why they cannot share one, got:\n{}",
        text
    );
}

/// Two differing *arithmetic* returns are rejected too.
///
/// #290 first shipped accepting them, because the SDK itself disagreed --
/// `-count` was `unsigned int` on `OZArray` where other classes wrote
/// `int` -- and rejecting would have broken working code. With the SDK's
/// size APIs typed `size_t`/`ptrdiff_t` that disagreement is gone and the
/// rule says what it means: one shim declares one return type, so any
/// disagreement is wrong for at least one implementor.
///
/// Having the shim declare the type C's usual arithmetic conversions would
/// give was the other option, and it is not implementable from the
/// spelling: whether `size_t` is wider than `unsigned int` is
/// target-dependent, so ranking them textually would be a guess.
#[test]
fn two_classes_with_differing_arithmetic_returns_are_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Small : OZObject
- (int)tally;
@end
@interface Big : OZObject
- (unsigned int)tally;
@end

@implementation Small
- (int)tally { return 2; }
@end
@implementation Big
- (unsigned int)tally { return 3; }
@end
int main(void) { return 0; }
"
    );
    let diags = match oz_static::transpile(&src) {
        Err(diags) => diags,
        Ok(_) => panic!("an arithmetic-return disagreement must be rejected"),
    };
    let text = diags.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n");
    assert!(
        text.contains("'int'") && text.contains("'unsigned int'"),
        "the diagnostic should name both types, got:\n{}",
        text
    );
}

/// `instancetype` is exempt and has to be: every implementor's resolved
/// return type is its own class, and the shim already collapses them to
/// `void *` for that reason. Comparing the resolved types would reject
/// `-init` in any program with two classes.
#[test]
fn instancetype_returns_are_not_a_collision() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Left : OZObject
- (instancetype)configure;
@end
@interface Right : OZObject
- (instancetype)configure;
@end

@implementation Left
- (instancetype)configure { return self; }
@end
@implementation Right
- (instancetype)configure { return self; }
@end

#include <stdio.h>
int main(void) {
	Left *l = [[Left alloc] init];
	Right *r = [[Right alloc] init];
	printf(\"ok=%d\\n\", ([l configure] != 0) + ([r configure] != 0));
	return 0;
}
"
    );
    oz_static::transpile(&src).expect("instancetype covaries by design and must not collide");
    assert_eq!(compile_and_run(&src, "instancetype_is_not_a_collision"), "ok=2\n");
}

/// One class implementing a selector is not a collision with itself, and a
/// subclass inheriting it is not a second implementor.
#[test]
fn a_single_implementor_and_an_inheriting_subclass_are_fine() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
struct thing { int v; };

@interface Base : OZObject
- (const struct thing *)spec;
@end
@interface Derived : Base
@end

@implementation Base
- (const struct thing *)spec { return (const struct thing *)0; }
@end
@implementation Derived
@end
int main(void) { return 0; }
"
    );
    oz_static::transpile(&src).expect("inheritance is not a second implementor");
}
