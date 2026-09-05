// SPDX-License-Identifier: Apache-2.0
//
// reflection.rs -- `@selector`, `SEL`, `-respondsToSelector:` and the
// `-performSelector:` family (#226).
//
// `-performSelector:` was the part the issue expected to be hard, on the
// assumption it needed either a generated switch per (selector, class)
// pair or a variadic trampoline like the retired legacy runtime's
// per-architecture assembly. It needs neither: the per-selector
// `class_id` switch already exists as `OZ_PROTOCOL_SEND_*`
// (`companion::render_protocol_dispatch`), and the only thing missing was
// that `Program::is_dynamically_dispatched` would not generate one for a
// selector with a single implementor. So a `SEL` is a pointer to a small
// `const` record holding a uniform-shape wrapper around that switch, plus
// the responds bitmap the wrapper alone could not answer.
//
// A `SEL` being a real C type is what keeps these tests short: an
// argument that is not one is a C type error, not something the static bar
// has to model.

mod common;
use common::{
    compile_and_run_with_reflection, expect_reject, expect_reject_with_reflection,
    ozobject_src as PREAMBLE,
};

/// `Widget` implements two selectors of different arity; `Plain`
/// implements neither.
///
/// `-poke` returning `void` and `-wrap:` returning an object exercise both
/// wrapper shapes, and `Plain` is what makes the responds bitmap
/// meaningful rather than always-true.
fn classes() -> &'static str {
    "\
@interface Widget : OZObject
- (void)poke;
- (id)wrap:(id)thing;
@end
@implementation Widget
- (void)poke {
}
- (id)wrap:(id)thing {
	return thing;
}
@end

@interface Plain : OZObject
@end
@implementation Plain
@end
"
}

/// `-respondsToSelector:` answers per class, through a `SEL` held in a
/// local as well as a literal, and NO for nil.
#[test]
fn responds_to_selector_answers_per_class() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        classes(),
        "\
#include <stdio.h>
int main(void) {
	Widget *w = [Widget alloc];
	Plain *p = [Plain alloc];
	Widget *none = nil;
	SEL poke = @selector(poke);
	printf(\"wp=%d pp=%d ww=%d nil=%d\\n\",
	       [w respondsToSelector:poke],
	       [p respondsToSelector:poke],
	       [w respondsToSelector:@selector(wrap:)],
	       [none respondsToSelector:poke]);
	return 0;
}
"
    );
    let out = compile_and_run_with_reflection(&src, "responds_to_selector");
    assert_eq!(out, "wp=1 pp=0 ww=1 nil=0\n", "unexpected: {}", out);
}

/// `-performSelector:` reaches the method, including through a `SEL`
/// stored in a local, and returns nil for a nil receiver.
///
/// `-poke` is implemented by exactly one class, which is the case that
/// needed `is_dynamically_dispatched` to grow a reflection clause: without
/// it no `OZ_PROTOCOL_SEND_poke` is generated and the wrapper would
/// reference a function that does not exist.
#[test]
fn perform_selector_reaches_the_method() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        classes(),
        "\
static int g_poked = 0;

@interface Counter : OZObject
- (void)tick;
@end
@implementation Counter
- (void)tick {
	g_poked = g_poked + 1;
}
@end

#include <stdio.h>
int main(void) {
	Widget *w = [Widget alloc];
	Counter *c = [Counter alloc];
	Counter *none = nil;
	SEL tick = @selector(tick);
	[c performSelector:tick];
	[c performSelector:@selector(tick)];
	id back = [w performSelector:@selector(wrap:) withObject:w];
	printf(\"poked=%d same=%d nil=%d\\n\",
	       g_poked, back == (id)w, [none performSelector:tick] == nil);
	return 0;
}
"
    );
    let out = compile_and_run_with_reflection(&src, "perform_selector_reaches");
    assert_eq!(out, "poked=2 same=1 nil=1\n", "unexpected: {}", out);
}

/// A `void`-returning selector performed through a `SEL` hands back nil,
/// not whatever happened to be in the return register.
///
/// Real Objective-C types the whole family `id`-returning and gives a
/// garbage `id` for a `void` method. The wrapper returns NULL instead, so
/// the value is defined rather than merely unlikely to be inspected.
#[test]
fn performing_a_void_selector_yields_nil() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        classes(),
        "\
#include <stdio.h>
int main(void) {
	Widget *w = [Widget alloc];
	printf(\"nil=%d\\n\", [w performSelector:@selector(poke)] == nil);
	return 0;
}
"
    );
    let out = compile_and_run_with_reflection(&src, "perform_void_selector");
    assert_eq!(out, "nil=1\n", "unexpected: {}", out);
}

/// A single-implementor selector named by a `@selector(...)` gets a
/// dispatch function it would not otherwise have.
///
/// Pins the `is_dynamically_dispatched` clause directly: `-tick` is
/// implemented once and is not protocol-declared, so the ordinary rule
/// (more than one implementor) excludes it.
#[test]
fn a_reflected_single_implementor_selector_gets_a_dispatch_function() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        classes(),
        "\
@interface Counter : OZObject
- (void)tick;
@end
@implementation Counter
- (void)tick {
}
@end

int main(void) {
	Counter *c = [Counter alloc];
	[c performSelector:@selector(tick)];
	return 0;
}
"
    );
    let options = oz_static::Options { reflection: true, ..Default::default() };
    let out = oz_static::transpile_with_options(&src, &options).expect("should transpile");
    // Asserted on the *declaration*, not on any mention: the wrapper in
    // the companion source calls `OZ_PROTOCOL_SEND_tick`, so a substring
    // check against that file passes whether or not the function was ever
    // generated -- as removing the `is_dynamically_dispatched` clause and
    // watching this test still pass demonstrated. Only the header carries
    // the prototype, and only when the dispatch function really exists.
    assert!(
        out.companion_h.contains("OZ_PROTOCOL_SEND_tick"),
        "a performed selector needs its dispatch function generated:\n{}",
        out.companion_h
    );
    assert!(
        out.companion_c.contains("oz_perform_tick") && out.companion_c.contains("oz_sel_tick"),
        "and its wrapper and record:\n{}",
        out.companion_c
    );
}

/// With the option off, all four constructs are located errors naming it.
#[test]
fn the_option_being_off_is_a_located_error_naming_it() {
    let cases = [
        ("@selector(...)", "SEL s = @selector(poke); (void)s;"),
        ("-respondsToSelector:", "int r = [w respondsToSelector:@selector(poke)]; (void)r;"),
        ("-performSelector:", "[w performSelector:@selector(poke)];"),
        (
            "-performSelector:withObject:",
            "id b = [w performSelector:@selector(wrap:) withObject:w]; (void)b;",
        ),
    ];
    for (what, stmt) in cases {
        let src = format!(
            "{}{}int main(void) {{\n\tWidget *w = [Widget alloc];\n\t(void)w;\n\t{}\n\treturn 0;\n}}\n",
            PREAMBLE(),
            classes(),
            stmt
        );
        let diags = expect_reject(&src);
        assert!(
            diags.contains("CONFIG_OBJZ_REFLECTION"),
            "'{}' with the option off must name the option, got:\n{}",
            what,
            diags
        );
    }
}

/// A program that enables reflection and names no selector gets no record,
/// no wrapper and no helper.
#[test]
fn enabling_the_option_alone_generates_nothing() {
    let src = format!("{}{}", PREAMBLE(), classes());
    let options = oz_static::Options { reflection: true, ..Default::default() };
    let out = oz_static::transpile_with_options(&src, &options).expect("should transpile");
    for absent in ["oz_sel_", "oz_responds", "oz_perform"] {
        assert!(
            !out.companion_c.contains(absent),
            "'{}' must not be emitted for a program that names no selector",
            absent
        );
    }
}

/// The two halves are independent: asking about responding emits no
/// wrapper, and performing emits no bitmap.
#[test]
fn responds_and_perform_are_gated_separately() {
    let options = oz_static::Options { reflection: true, ..Default::default() };

    let responds_only = format!(
        "{}{}int main(void) {{\n\tWidget *w = [Widget alloc];\n\tint r = [w respondsToSelector:@selector(poke)];\n\treturn r;\n}}\n",
        PREAMBLE(),
        classes()
    );
    let out = oz_static::transpile_with_options(&responds_only, &options).expect("should transpile");
    assert!(
        out.companion_c.contains("oz_responds_poke") && out.companion_c.contains("BOOL oz_responds"),
        "the bitmap and its reader must be emitted:\n{}",
        out.companion_c
    );
    assert!(
        !out.companion_c.contains("oz_perform"),
        "nothing performs, so no wrapper belongs here:\n{}",
        out.companion_c
    );

    let perform_only = format!(
        "{}{}int main(void) {{\n\tWidget *w = [Widget alloc];\n\t[w performSelector:@selector(poke)];\n\treturn 0;\n}}\n",
        PREAMBLE(),
        classes()
    );
    let out = oz_static::transpile_with_options(&perform_only, &options).expect("should transpile");
    assert!(
        out.companion_c.contains("oz_perform_poke"),
        "the wrapper must be emitted:\n{}",
        out.companion_c
    );
    assert!(
        !out.companion_c.contains("oz_responds"),
        "nothing asks about responding, so no bitmap belongs here:\n{}",
        out.companion_c
    );
}

/// `@selector(...)` naming a selector no class implements is a located
/// error.
///
/// There is no record to point at, so passing it through would emit a
/// reference to a symbol that is never generated -- an undefined symbol at
/// link time with no source location, which is exactly the `[X class]`
/// failure this issue started from.
#[test]
fn a_selector_nothing_implements_is_rejected() {
    let src = format!(
        "{}{}int main(void) {{\n\tSEL s = @selector(nonexistentThing);\n\t(void)s;\n\treturn 0;\n}}\n",
        PREAMBLE(),
        classes()
    );
    let diags = expect_reject_with_reflection(&src);
    assert!(
        diags.contains("nonexistentThing") && diags.contains("names no instance method"),
        "an unimplemented selector must be refused, got:\n{}",
        diags
    );
}

/// In a program that performs, a selector with no uniform-shape wrapper is
/// refused where it is named.
///
/// Performability is a whole-program property because a `SEL` is a value:
/// nothing can prove which selector reaches which call site, so every
/// named selector has to be performable or the wrapper's types would not
/// hold. Refusing at the `@selector(...)` is what keeps this from becoming
/// a null `perform` pointer discovered at run time.
#[test]
fn an_unperformable_selector_is_rejected_where_the_program_performs() {
    let cases = [
        ("- (void)setCount:(int)n {\n}\n", "setCount:", "not an object type"),
        ("- (size_t)count {\n\treturn 0;\n}\n", "count", "neither void nor an object type"),
        (
            "- (void)a:(id)x b:(id)y c:(id)z {\n}\n",
            "a:b:c:",
            "passes at most two",
        ),
    ];
    for (method, selector, why) in cases {
        let src = format!(
            "{}{}@interface Odd : OZObject\n@end\n@implementation Odd\n{}@end\n\nint main(void) {{\n\tWidget *w = [Widget alloc];\n\t[w performSelector:@selector({})];\n\treturn 0;\n}}\n",
            PREAMBLE(),
            classes(),
            method,
            selector
        );
        let diags = expect_reject_with_reflection(&src);
        assert!(
            diags.contains("cannot be performed") && diags.contains(why),
            "'{}' should be refused as unperformable ({}), got:\n{}",
            selector,
            why,
            diags
        );
    }
}

/// The same selector is fine for `-respondsToSelector:` alone: without a
/// `-performSelector:` anywhere, no wrapper is needed and the signature
/// never has to fit one.
///
/// This is the control for the test above -- it is what shows the
/// rejection is driven by the program performing, not by the signature on
/// its own.
#[test]
fn an_unperformable_selector_is_fine_when_nothing_performs() {
    let src = format!(
        "{}{}\
@interface Odd : OZObject
- (size_t)count;
@end
@implementation Odd
- (size_t)count {{
	return 7;
}}
@end

#include <stdio.h>
int main(void) {{
	Odd *o = [Odd alloc];
	printf(\"r=%d\\n\", [o respondsToSelector:@selector(count)]);
	return 0;
}}
",
        PREAMBLE(),
        classes()
    );
    let out = compile_and_run_with_reflection(&src, "unperformable_but_only_queried");
    assert_eq!(out, "r=1\n", "unexpected: {}", out);
}

/// A keyword selector's name survives extraction.
///
/// tree-sitter-objc 3.0.2 exposes no children at all inside
/// `@selector(wrap:)` -- only the `@selector`, `(` and `)` tokens -- so an
/// implementation that walked children found the name for `@selector(poke)`
/// and nothing for every selector taking an argument.
/// `collect::selector_literal_name` reads the node's text instead.
#[test]
fn a_keyword_selector_name_is_extracted() {
    let src = format!(
        "{}{}\
@interface Multi : OZObject
- (id)a:(id)x b:(id)y;
@end
@implementation Multi
- (id)a:(id)x b:(id)y {{
	(void)y;
	return x;
}}
@end

#include <stdio.h>
int main(void) {{
	Multi *m = [Multi alloc];
	id got = [m performSelector:@selector(a:b:) withObject:m withObject:m];
	printf(\"same=%d\\n\", got == (id)m);
	return 0;
}}
",
        PREAMBLE(),
        classes()
    );
    let out = compile_and_run_with_reflection(&src, "keyword_selector_name");
    assert_eq!(out, "same=1\n", "unexpected: {}", out);
}

/// A null `SEL` answers NO and performs nothing, rather than
/// dereferencing.
///
/// `SEL` is a plain pointer, so nothing stops a caller writing
/// `[obj respondsToSelector:0]` -- `tests/static_bar_rejects.rs` had one
/// doing exactly that, from back when the selector was refused outright
/// and the argument never reached any generated code. The guard is in the
/// helpers rather than at the call site so it holds however the SEL got
/// there.
#[test]
fn a_null_selector_is_answered_not_dereferenced() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        classes(),
        "\
#include <stdio.h>
int main(void) {
	Widget *w = [Widget alloc];
	SEL none = (SEL)0;
	printf(\"responds=%d perform=%d\\n\",
	       [w respondsToSelector:none], [w performSelector:none] == nil);
	return 0;
}
"
    );
    let out = compile_and_run_with_reflection(&src, "null_selector");
    assert_eq!(out, "responds=0 perform=1\n", "unexpected: {}", out);
}
