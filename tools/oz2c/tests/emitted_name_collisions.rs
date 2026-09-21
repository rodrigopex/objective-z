// SPDX-License-Identifier: Apache-2.0
//
// emitted_name_collisions.rs -- two distinct declarations may never reach one
// C identifier (#605).
//
// `selector_to_c` mapped `:` to `_`, and `_` is a legal selector character,
// so the mapping was not injective. Two selectors, two conflicting
// declarations *and* definitions, oz2c exiting 0, and GCC answering
// `conflicting types` in a file the author never wrote:
//
//     void Clash_a__(struct Clash *self, int x, int y);   /* -a::   */
//     void Clash_a__(struct Clash *self);                 /* -a__   */
//
// **No colon-expansion fixes it.** For any n, `:` -> `_`^n collides on two
// selectors identical except that one has `:` where the other has `_`^n --
// `__` merely moves the collision from `a::`/`a__` to `a:`/`a__`. Measured
// over 448 declarations, `_` -> `__` collides 56 times.
//
// So the escape marks the literal underscore instead: `_5F_`, `_`'s code
// point, the convention JNI, Swift and Rust v0 use. **The marker must be
// digit-led**, because no selector piece may begin with a digit
// (`collect::selector_literal_name`) -- a *letter* cannot serve, and `_u_`,
// the obvious mnemonic, collides 234 times in 2,574 because it is
// indistinguishable from "colon, piece `u`, colon".
//
// Seven families reached one identifier from two declarations. Five are
// closed by the escape. The last two are the *struct tag* -- the bare,
// uncomposed class name -- which no escape over composed names can reach, so
// the `oz_`/`OZ_` prefix is reserved on class names instead
// (`staticbar::check_generated_namespace`).

mod common;
use common::{expect_reject, ozobject_src as PREAMBLE};

fn program(body: &str) -> String {
    format!("{}{}", PREAMBLE(), body)
}

/// Every emitted byte from one transpile, so the test sees what the C
/// compiler sees.
///
/// All three outputs, not just `source_c`: the slab, the allocators and the
/// class-id macro are the *companion*'s, and families 6 and 7 live there.
fn header_of(body: &str) -> String {
    let src = program(body);
    let out = oz2c::transpile(&src).expect("probe must transpile");
    format!("{}\n{}\n{}", out.source_c, out.companion_h, out.companion_c)
}

/* ---------------------------------------------------------------- family 1 */

/// `-a::` and `-a__` are different selectors and must stay different names.
#[test]
fn family_1_an_empty_piece_and_an_underscore_name_differ() {
    let c = header_of(
        "\
@interface Clash : OZObject {
	int _sum;
	int _flag;
}
- (void)a:(int)x :(int)y;
- (void)a__;
@end

@implementation Clash
- (void)a:(int)x :(int)y { _sum = x + y; }
- (void)a__ { _flag = 99; }
@end
",
    );
    assert!(c.contains("Clash_a__("), "the colon form keeps today's name:\n{}", c);
    assert!(c.contains("Clash_a_5F__5F_("), "the underscore form must be escaped:\n{}", c);
}

/// The same defect without any exotic syntax -- an ordinary two-part
/// selector against a name that happens to spell its mangling.
#[test]
fn family_1_a_two_part_selector_and_its_spelling_differ() {
    let c = header_of(
        "\
@interface Sib : OZObject {
	int _a;
	int _b;
}
- (void)put:(int)x to:(int)y;
- (void)put_to_;
@end

@implementation Sib
- (void)put:(int)x to:(int)y { _a = x + y; }
- (void)put_to_ { _b = 1; }
@end
",
    );
    assert!(c.contains("Sib_put_to_("), "{}", c);
    assert!(c.contains("Sib_put_5F_to_5F_("), "{}", c);
}

/* ---------------------------------------------------------------- family 2 */

/// `+foo` and `-foo_cls` both reached `F_foo_cls`. The `_cls` suffix
/// separates `+foo` from `-foo` and nothing more, which `method_fn_name`'s
/// doc comment claimed until #605.
#[test]
fn family_2_the_cls_suffix_does_not_collide_with_a_selector_spelling_it() {
    let c = header_of(
        "\
@interface F : OZObject {
	int _n;
}
+ (int)foo;
- (void)foo_cls;
@end

@implementation F
+ (int)foo { return 7; }
- (void)foo_cls { _n = 1; }
@end
",
    );
    assert!(c.contains("F_foo_cls("), "the class method keeps today's name:\n{}", c);
    assert!(c.contains("F_foo_5F_cls("), "the instance selector must be escaped:\n{}", c);
}

/* ---------------------------------------------------------------- family 3 */

/// The class/selector joiner: `-[X y:z:]` and `-[X_y z:]` both reached
/// `X_y_z_`. This is the family that needs the *class* name escaped, which is
/// why `method_fn_name` escapes it rather than only `selector_to_c`.
#[test]
fn family_3_the_joiner_separates_a_class_with_an_underscore() {
    let a = header_of(
        "\
@interface X : OZObject { int _v; }
- (void)y:(int)p z:(int)q;
@end
@implementation X
- (void)y:(int)p z:(int)q { _v = p + q; }
@end
",
    );
    let b = header_of(
        "\
@interface X_y : OZObject { int _v; }
- (void)z:(int)q;
@end
@implementation X_y
- (void)z:(int)q { _v = q; }
@end
",
    );
    assert!(a.contains("X_y_z_("), "the two-part selector keeps today's name:\n{}", a);
    assert!(b.contains("X_5F_y_z_("), "the underscored class must be escaped:\n{}", b);
}

/* ---------------------------------------------------------------- family 4 */

/// A selector spelling a synthesized helper: `-oz_alloc` reached
/// `{Class}_oz_alloc`, the allocator's own name.
#[test]
fn family_4_a_selector_cannot_spell_a_synthesized_helper() {
    let c = header_of(
        "\
@interface F : OZObject { int _n; }
- (void)oz_alloc;
@end
@implementation F
- (void)oz_alloc { _n = 1; }
@end
",
    );
    assert!(c.contains("F_oz_5F_alloc("), "the selector must be escaped:\n{}", c);
    assert!(
        c.contains("F_oz_alloc(void)"),
        "and the real allocator keeps its own name:\n{}",
        c
    );
}

/* ------------------------------------------------------------ families 6, 7 */

/// The struct tag is the bare class name, so a class named like a generated
/// helper cannot be separated by any escape over *composed* names. Reserved
/// instead.
#[test]
fn families_6_and_7_the_generated_namespace_is_reserved_on_class_names() {
    for name in ["oz_slab_Foo", "oz_alloc_Foo", "OZ_CLASS_Fan"] {
        let src = program(&format!(
            "@interface {name} : OZObject\n- (int)v;\n@end\n\
             @implementation {name}\n- (int)v {{ return 1; }}\n@end\n"
        ));
        let err = expect_reject(&src);
        assert!(
            err.contains("is in the generated namespace"),
            "'{}' must be refused; got:\n{}",
            name,
            err
        );
        assert!(
            err.contains("rename it"),
            "the diagnostic must name a remedy; got:\n{}",
            err
        );
        /* One mistake, one diagnostic -- `@interface` and `@implementation`
         * are two nodes naming the same class. */
        assert_eq!(
            err.matches("is in the generated namespace").count(),
            1,
            "one diagnostic per class, not per declaration; got:\n{}",
            err
        );
    }
}

/// The reservation is narrow on purpose: the separator is part of it, so the
/// fifteen `OZ`-prefixed SDK classes are untouched.
#[test]
fn the_reservation_does_not_reach_oz_prefixed_sdk_class_names() {
    let src = program(
        "\
@interface OZWidget : OZObject { int _v; }
- (int)v;
@end
@implementation OZWidget
- (int)v { return 1; }
@end
",
    );
    oz2c::transpile(&src)
        .expect("'OZWidget' has no separator and must stay legal -- OZObject, OZString et al.");
}

/* ------------------------------------------------------------- the property */

/// Round-trip: the escape is injective because it is decodable.
///
/// This is the whole claim, and it is the reason the escape can be trusted
/// over a collision *check*: there is no enumerator to drift. Brute-forced
/// over the alphabet where the ambiguity lives -- `_`, `:` and a plain letter
/// -- rather than over realistic names, which would never reach the edge.
#[test]
fn the_escape_round_trips_for_every_shape_underscores_and_colons_can_take() {
    fn esc(s: &str) -> String {
        s.replace('_', "_5F_")
    }
    fn encode(class: &str, selector: &str, is_class_method: bool) -> String {
        let name = format!("{}_{}", esc(class), esc(selector).replace(':', "_"));
        if is_class_method {
            format!("{}_cls", name)
        } else {
            name
        }
    }
    /// The inverse. A `_` followed by `5F_` is a literal underscore; any
    /// other `_` is a separator. Nothing else can produce that sequence,
    /// because no piece may begin with a digit.
    fn decode(mut name: &str) -> (String, String, bool) {
        let is_class_method = name.ends_with("_cls");
        if is_class_method {
            name = &name[..name.len() - 4];
        }
        let bytes: Vec<char> = name.chars().collect();
        let mut parts: Vec<String> = Vec::new();
        let mut buf = String::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == '_' {
                if bytes[i + 1..].starts_with(&['5', 'F', '_']) {
                    buf.push('_');
                    i += 4;
                } else {
                    parts.push(std::mem::take(&mut buf));
                    i += 1;
                }
            } else {
                buf.push(bytes[i]);
                i += 1;
            }
        }
        parts.push(buf);
        let class = parts.remove(0);
        let selector = if parts.len() == 1 {
            parts[0].clone()
        } else {
            format!("{}:", parts[..parts.len() - 1].join(":"))
        };
        (class, selector, is_class_method)
    }

    let alpha = ['a', '_'];
    let mut pieces: Vec<String> = Vec::new();
    for a in alpha {
        pieces.push(a.to_string());
        for b in alpha {
            pieces.push(format!("{}{}", a, b));
        }
    }
    let mut selectors: Vec<String> = pieces.clone();
    for p in &pieces {
        selectors.push(format!("{}:", p));
    }
    for p in &pieces {
        for q in &pieces {
            selectors.push(format!("{}:{}:", p, q));
        }
    }

    let mut checked = 0usize;
    for class in &pieces {
        for selector in &selectors {
            for is_class_method in [false, true] {
                let encoded = encode(class, selector, is_class_method);
                let back = decode(&encoded);
                assert_eq!(
                    back,
                    (class.clone(), selector.clone(), is_class_method),
                    "round trip failed for {:?} / {:?} / cls={} -> {}",
                    class,
                    selector,
                    is_class_method,
                    encoded
                );
                checked += 1;
            }
        }
    }
    /* Presence paired with the absence check above: a generator that produced
     * nothing would pass every assertion. */
    assert!(checked > 500, "expected a real search space, probed only {}", checked);
}

/// Nothing without an underscore moves, which is what makes this safe.
///
/// The families above prove the escape fires; this proves it fires *only*
/// there. Measured across both corpora when the change landed: 0 of 118 cases
/// produced different C, and no selector or class name in any compiled source
/// contains `_` -- so the escape is latent until someone writes one. An
/// escape that quietly respelled ordinary names would have been the same
/// change as the length-prefixed scheme this one was chosen over.
#[test]
fn an_underscore_free_program_keeps_every_name_it_has_today() {
    let c = header_of(
        "\
@interface Widget : OZObject {
	int _tag;
}
- (int)tag;
- (void)setTag:(int)t;
- (void)setOriginX:(int)x y:(int)y;
+ (int)make;
@end

@implementation Widget
- (int)tag { return _tag; }
- (void)setTag:(int)t { _tag = t; }
- (void)setOriginX:(int)x y:(int)y { _tag = x + y; }
+ (int)make
{
	/* Allocates, so the slab and the class-id macro are emitted too --
	 * `pools` sizes a slab from allocation sites, and a class that is
	 * never allocated has none. */
	Widget *w = [Widget alloc];

	return w != nil;
}
@end
",
    );
    for name in [
        "Widget_tag(",
        "Widget_setTag_(",
        "Widget_setOriginX_y_(",
        "Widget_make_cls(",
        "Widget_oz_alloc(",
        "oz_slab_Widget",
        "OZ_CLASS_Widget",
        "struct Widget",
    ] {
        assert!(c.contains(name), "'{}' must be unchanged; got:\n{}", name, c);
    }
    /* And the escape appears nowhere at all, or something moved that should
     * not have -- the ivar `_tag` is a struct member, not a name the scheme
     * touches. */
    assert!(
        !c.contains("_5F_"),
        "no name in an underscore-free program may carry the escape:\n{}",
        c
    );
}
