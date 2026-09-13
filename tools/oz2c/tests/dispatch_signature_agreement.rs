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
//
// *Located* is half the contract, so every rejection here asserts the line
// as well as the message. The first cut asserted only the text, and a
// property-declared selector -- which is what the reported collision
// actually was -- reported `1:1` for four commits without a test noticing
// (#297).

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// 1-based line of the first source line containing `needle`.
///
/// The preamble `PREAMBLE()` prepends is over a hundred lines and grows, so
/// an expected line is looked up in the assembled source rather than
/// written down as a number.
fn line_of(src: &str, needle: &str) -> usize {
    src.lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line of the source contains {:?}", needle))
        + 1
}

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
    assert_eq!(
        diags[0].line,
        line_of(&src, "- (const struct beta_spec *)spec;"),
        "the error belongs on the second declaration, the one that \
         introduced the disagreement, got:\n{}",
        diags[0]
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
    assert_eq!(
        diags[0].line,
        line_of(&src, "- (unsigned int)tally;"),
        "the error belongs on the second declaration, got:\n{}",
        diags[0]
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

/// The shape the check was actually filed for: the colliding selector is
/// declared as a `@property`, not as a method (#297).
///
/// `locate_method` walks for a `method_declaration` or `method_definition`,
/// and a property writes neither -- so the search returned `None` and the
/// error landed at `1:1`. The verdict was always right; only the position
/// was lost, and lost for the one case that motivated the rule.
#[test]
fn a_property_declared_collision_is_located_at_the_property() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
struct alpha_spec { int a; };
struct beta_spec { long b; };

@interface Alpha : OZObject
@property(nonatomic, readonly, unsafe_unretained) const struct alpha_spec *spec;
@end
@interface Beta : OZObject
@property(nonatomic, readonly, unsafe_unretained) const struct beta_spec *spec;
@end

@implementation Alpha
@synthesize spec = _spec;
@end
@implementation Beta
@synthesize spec = _spec;
@end
int main(void) { return 0; }
"
    );
    let diags = match oz_static::transpile(&src) {
        Err(diags) => diags,
        Ok(_) => panic!("a property-declared collision must be rejected too"),
    };
    let text = diags.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n");
    assert!(
        text.contains("alpha_spec") && text.contains("beta_spec"),
        "the diagnostic should name both return types, got:\n{}",
        text
    );
    assert_eq!(
        diags[0].line,
        line_of(&src, "const struct beta_spec *spec;"),
        "a property-declared selector must be located at its @property, \
         not at the top of the file, got:\n{}",
        diags[0]
    );
}

/// A property's selector is not its spelling. `getter=probe` declares
/// `probe`, which is what dispatch is keyed on and what
/// `collect::extract_property` records -- so matching the property by name
/// would have missed this and located it at `1:1` again, while matching two
/// differently-spelled properties that share a getter is exactly right.
#[test]
fn a_renamed_getter_is_located_at_its_property() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
struct alpha_spec { int a; };
struct beta_spec { long b; };

@interface Alpha : OZObject
@property(nonatomic, readonly, getter=probe, unsafe_unretained) const struct alpha_spec *spec;
@end
@interface Beta : OZObject
@property(nonatomic, readonly, getter=probe, unsafe_unretained) const struct beta_spec *cfg;
@end

@implementation Alpha
@synthesize spec = _spec;
@end
@implementation Beta
@synthesize cfg = _cfg;
@end
int main(void) { return 0; }
"
    );
    let diags = match oz_static::transpile(&src) {
        Err(diags) => diags,
        Ok(_) => panic!("two properties sharing a renamed getter must be rejected"),
    };
    assert!(
        diags[0].message.contains("'probe'"),
        "the collision is on the getter selector, not the property name, got:\n{}",
        diags[0]
    );
    assert_eq!(
        diags[0].line,
        line_of(&src, "const struct beta_spec *cfg;"),
        "got:\n{}",
        diags[0]
    );
}

/// A synthesized *setter* is located the same way. It returns `void`, so it
/// can only disagree with a hand-written method of that name -- rarer than
/// the getter case, and the same lookup answers it.
#[test]
fn a_synthesized_setter_is_located_at_its_property() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Beta : OZObject
- (int)setTally:(int)v;
@end
@interface Alpha : OZObject
@property(nonatomic) int tally;
@end

@implementation Beta
- (int)setTally:(int)v { return v; }
@end
@implementation Alpha
@synthesize tally = _tally;
@end
int main(void) { return 0; }
"
    );
    let diags = match oz_static::transpile(&src) {
        Err(diags) => diags,
        Ok(_) => panic!("a setter disagreeing with a method of that name must be rejected"),
    };
    assert!(
        diags[0].message.contains("'setTally:'"),
        "got:\n{}",
        diags[0]
    );
    assert_eq!(
        diags[0].line,
        line_of(&src, "@property(nonatomic) int tally;"),
        "the synthesized setter belongs to the @property, so that is where \
         the error goes, got:\n{}",
        diags[0]
    );
}

/// A category is a declaration site too.
///
/// `collect` pushes a category's `method_declaration`s onto the class, so a
/// selector declared only in `@interface Beta (Extra)` really does take
/// part in a collision -- but `locate_method` skipped any node with a
/// category name and reported `1:1`, the same lost position as the
/// property case and the same cause (#297).
#[test]
fn a_category_declared_collision_is_located_in_the_category() {
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
@end
@interface Beta (Extra)
- (const struct beta_spec *)spec;
@end

@implementation Alpha
- (const struct alpha_spec *)spec { return (const struct alpha_spec *)0; }
@end
@implementation Beta
@end
@implementation Beta (Extra)
- (const struct beta_spec *)spec { return (const struct beta_spec *)0; }
@end
int main(void) { return 0; }
"
    );
    let diags = match oz_static::transpile(&src) {
        Err(diags) => diags,
        Ok(_) => panic!("a category-declared collision must be rejected too"),
    };
    assert_eq!(
        diags[0].line,
        line_of(&src, "- (const struct beta_spec *)spec;"),
        "got:\n{}",
        diags[0]
    );
}
