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

/// Two differing *arithmetic* returns are left alone, deliberately.
///
/// The shim declares one and returns the other, and C's usual conversions
/// apply. `OZArray` returns `unsigned int` for `-count` where a class of
/// one's own may return `int`; that has always worked, is not what #290 was
/// about, and rejecting it would break working code to no purpose. The
/// first version of this check did reject it and took `behavior_forin` down
/// with it.
#[test]
fn two_classes_with_differing_arithmetic_returns_are_accepted() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Small : OZObject
- (int)count;
@end
@interface Big : OZObject
- (unsigned int)count;
@end

@implementation Small
- (int)count { return 2; }
@end
@implementation Big
- (unsigned int)count { return 3; }
@end

#include <stdio.h>
int main(void) {
	Small *s = [[Small alloc] init];
	Big *b = [[Big alloc] init];
	printf(\"n=%d\\n\", [s count] + (int)[b count]);
	return 0;
}
"
    );
    oz_static::transpile(&src).expect("an arithmetic-return difference is not an error");
    assert_eq!(compile_and_run(&src, "differing_arithmetic_returns"), "n=5\n");
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
