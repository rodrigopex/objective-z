// SPDX-License-Identifier: Apache-2.0
//
// out_parameter_and_arc_attributes.rs -- #461, two declarations oz2c used
// to copy through without asking an ownership question.
//
// 1. A `+1` written through a dereferenced pointer. ARC § 2.6.5 makes
//    `T __autoreleasing *` an out-parameter written by pass-by-writeback,
//    and § 2.7.2 *infers* that qualifier for a plain `T *` -- so `*out =
//    <+1>` stores into an ARC-managed slot. It was the one strong
//    destination nothing walked: the store releases nothing, and the
//    caller's variable is written only through the pointer so it joins no
//    scope. Refused, per #430, because ARC's own answer is writeback
//    through an autoreleased temporary and there is no pool here.
//
// 2. `objc_precise_lifetime` and `objc_externally_retained` reaching the
//    generated C. They change no answer -- every release here is already
//    precise -- but they are not C. Stripped, not refused. The five
//    ownership-carrying attributes are a different case and are *refused*
//    by `staticbar::check_ownership_attributes` (#458); stripping one of
//    those silently would make ARC and the attribute disagree, which is a
//    use-after-free rather than a leak.
//
// The case that matters most here is the negative one: the SDK declares
// `objects:(__unsafe_unretained id *)stackbuf` on
// `countByEnumeratingWithState:objects:count:`, which is structurally the
// same shape and entirely correct. A refusal keyed on "a pointer written
// through" would refuse `OZArray`'s own fast enumeration, so the key is
// the `+1`, not the shape.

mod common;

/// A `+1` through a dereferenced pointer is refused, and the diagnostic
/// says what to do instead.
#[test]
fn a_plus_one_stored_through_a_pointer_is_refused() {
    let diags = common::expect_reject(
        "@interface OZObject\n@end\n@implementation OZObject\n@end\n\
         @interface Thing : OZObject\n@end\n@implementation Thing\n@end\n\
         @interface R : OZObject\n- (void)out:(Thing **)p;\n@end\n\
         @implementation R\n- (void)out:(Thing **)p\n{\n\t*p = [Thing alloc];\n}\n@end\n",
    );
    assert!(
        diags.contains("stored through a dereferenced pointer"),
        "diagnostics: {}",
        diags
    );
    /* The reason, on its own tier (#457): a reader must be told *why* a
     * store that ARC accepts is refused here, or the rule reads arbitrary. */
    assert!(diags.contains("no pool to autorelease into"), "diagnostics: {}", diags);
    /* Both remedies, each its own `help` line. The alternative that works
     * here, and the one for a borrowed buffer -- not ARC's own answer,
     * which needs the pool this target does not have. */
    assert!(diags.contains("return the object instead"), "diagnostics: {}", diags);
    assert!(diags.contains("__unsafe_unretained"), "diagnostics: {}", diags);
}

/// **The negative case, and the reason the refusal is keyed on the `+1`.**
///
/// A borrowed store through the same shape is what fast enumeration does,
/// and it must stay legal. Presence and absence as a pair: the fixture
/// really does contain a `*p = ...` store, and it really is accepted.
#[test]
fn a_borrowed_store_through_a_pointer_is_accepted() {
    let source = "@interface OZObject\n@end\n@implementation OZObject\n@end\n\
                  @interface Thing : OZObject\n@end\n@implementation Thing\n@end\n\
                  @interface R : OZObject\n- (void)fill:(__unsafe_unretained id *)buf \
                  with:(Thing *)t;\n@end\n\
                  @implementation R\n\
                  - (void)fill:(__unsafe_unretained id *)buf with:(Thing *)t\n{\n\
                  \t*buf = t;\n}\n@end\n";
    assert!(source.contains("*buf = t;"), "the fixture must contain the store it is about");
    let out = oz2c::transpile(source).expect("a borrowed store must transpile");
    assert!(
        out.source_c.contains("*buf ="),
        "the borrowed store must survive to the output:\n{}",
        out.source_c
    );
}

/// A `+0` call through a pointer is not a `+1`, so it is not refused
/// either -- the predicate is `arc::binds_ownership`, not "is an
/// assignment through a pointer".
#[test]
fn a_plus_zero_store_through_a_pointer_is_accepted() {
    let source = "@interface OZObject\n@end\n@implementation OZObject\n@end\n\
                  @interface Thing : OZObject\n- (Thing *)peer;\n@end\n\
                  @implementation Thing\n- (Thing *)peer { return self; }\n@end\n\
                  @interface R : OZObject\n- (void)out:(Thing **)p from:(Thing *)t;\n@end\n\
                  @implementation R\n- (void)out:(Thing **)p from:(Thing *)t\n{\n\
                  \t*p = [t peer];\n}\n@end\n";
    let out = oz2c::transpile(source).expect("a +0 store must transpile");
    assert!(out.source_c.contains("*p ="), "a +0 store through a pointer stays:\n{}", out.source_c);
}

/// The two lifetime attributes are stripped from the emitted declaration.
///
/// Asserted against the *code*, not the whole file: oz2c keeps the
/// original source in a `/* original */` provenance comment, so the
/// spelling legitimately survives there and a naive whole-file check
/// would fail for the wrong reason.
#[test]
fn the_two_lifetime_attributes_are_stripped_from_emitted_code() {
    for attr in ["objc_precise_lifetime", "objc_externally_retained"] {
        let source = format!(
            "@interface OZObject\n@end\n@implementation OZObject\n@end\n\
             @interface Thing : OZObject\n- (int)tag;\n@end\n\
             @implementation Thing\n- (int)tag {{ return 1; }}\n@end\n\
             @interface R : OZObject\n- (int)run;\n@end\n\
             @implementation R\n- (int)run\n{{\n\
             \t__attribute__(({attr})) Thing *t = [Thing alloc];\n\
             \treturn [t tag];\n}}\n@end\n",
            attr = attr
        );
        assert!(source.contains(attr), "the fixture must carry the attribute");
        let out = oz2c::transpile(&source).expect("the attribute must not be refused");
        let code: String = out
            .source_c
            .lines()
            .filter(|l| !l.trim_start().starts_with("/*") && !l.trim_start().starts_with("*"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains(attr),
            "'{}' reached the emitted code, not just a provenance comment:\n{}",
            attr,
            code
        );
        /* And the declaration it sat on is still there, so the strip did
         * not take the statement with it. */
        assert!(code.contains("struct Thing *t"), "the declaration survived:\n{}", code);
    }
}

/// The ownership-carrying attributes stay **refused** rather than joining
/// the stripped list. #458 owns that refusal; this asserts the boundary,
/// because silently stripping one is a use-after-free rather than a leak.
#[test]
fn the_ownership_attributes_are_still_refused_not_stripped() {
    for attr in ["ns_returns_retained", "ns_returns_not_retained", "ns_consumed"] {
        let diags = common::expect_reject(&format!(
            "@interface OZObject\n@end\n@implementation OZObject\n@end\n\
             @interface T : OZObject\n- (T *)make __attribute__(({attr}));\n@end\n\
             @implementation T\n- (T *)make __attribute__(({attr})) {{ return [T alloc]; }}\n@end\n",
            attr = attr
        ));
        assert!(diags.contains(attr), "'{}' must still be refused: {}", attr, diags);
    }
}
