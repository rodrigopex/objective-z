// SPDX-License-Identifier: Apache-2.0
//
// static_bar_rejects.rs - OZ-091 Track B: constructs outside the static
// subset must be a named, located hard error -- never a silent skip.

mod common;
use common::{
    compile_and_run, expect_reject, ozarray_src, ozobject_src as PREAMBLE, oznumber_src,
};

#[test]
fn try_catch_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    @try {{\n        int x = 1;\n    }} @catch (id e) {{\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@try/@catch"), "diagnostics: {}", diags);
}

// @synchronized used to be rejected outright; it is supported now (see
// `emit::render_synchronized_statement`). What remains rejected is a jump
// that would escape the body and leak the lock -- covered by
// `behavior_synchronized::break_escaping_synchronized_rejected`, next to
// the accepted cases it contrasts with.

#[test]
fn weak_property_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n@property (weak) id delegate;\n@end\n\
         @implementation Foo\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'weak' property 'delegate'"), "diagnostics: {}", diags);
    assert!(diags.contains("unsafe_unretained"), "diagnostics: {}", diags);
}

/// The ivar-level counterpart of `weak_property_rejected`. `__strong` and
/// `__unsafe_unretained` are stripped on the way into the generated
/// struct (see `emit::lower_ivar_decl`), but `__weak` is rejected: with
/// no runtime to zero the reference it would silently behave as an
/// unretained strong ivar.
///
/// **The ivar was the only position that worked**, and since #448 the
/// refusal is one whole-tree walk over `type_qualifier` nodes
/// (`staticbar::check_refused_qualifiers`), so the general rule and all ten
/// positions live in `weak_every_position.rs`. This case stays as the
/// control for the position that never regressed, and its wording
/// assertion moved with the message: the diagnostic no longer says
/// "ivars", because saying so is what let a reader conclude the
/// prohibition was ivar-shaped -- the misreading that had #448 recording
/// two covered positions when there was one.
#[test]
fn weak_ivar_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject {{\n\t__weak id _delegate;\n}}\n@end\n\
         @implementation Foo\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'__weak' is not supported"), "diagnostics: {}", diags);
    assert!(diags.contains("unsafe_unretained"), "diagnostics: {}", diags);
}

/// Reflection and introspection are supported since #226, each behind its
/// own Kconfig option, so these two no longer assert that the construct
/// has no lowering -- they assert the *option being off* is what refuses
/// it, and that the refusal names the option so the message is actionable.
///
/// Both are kept here rather than folded into `tests/reflection.rs` and
/// `tests/introspection.rs` because this file's subject is the bar's
/// reach: the diagnostic has to come from inside an `@implementation`
/// method body, which is the path these exercise.
#[test]
fn reflection_selector_rejected_when_the_option_is_off() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    if ([self respondsToSelector:0]) {{\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("respondsToSelector:"), "diagnostics: {}", diags);
    assert!(diags.contains("CONFIG_OBJZ_REFLECTION"), "diagnostics: {}", diags);
}

#[test]
fn is_kind_of_class_rejected_when_the_option_is_off() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    if ([self isKindOfClass:0]) {{\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("isKindOfClass:"), "diagnostics: {}", diags);
    assert!(diags.contains("CONFIG_OBJZ_INTROSPECTION"), "diagnostics: {}", diags);
}

#[test]
fn capturing_block_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    int local = 5;\n    void (^blk)(void) = ^{{\n        local;\n    }};\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("captures 'local'"), "diagnostics: {}", diags);
}

#[test]
fn self_capturing_block_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject {{\n    int _x;\n}}\n- (void)test;\n@end\n\
         @implementation Foo\n- (void)test {{\n    void (^blk)(void) = ^{{\n        _x;\n    }};\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("captures"), "diagnostics: {}", diags);
}

#[test]
fn non_capturing_block_accepted() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    void (^blk)(void) = ^{{\n        int y = 1;\n    }};\n}}\n@end\n",
        PREAMBLE()
    );
    oz2c::transpile(&src).unwrap_or_else(|diags| {
        panic!(
            "expected a non-capturing block to be accepted, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}

/// `_cached = [Item alloc]` in a loop: accepted since #405.
///
/// It was refused because an ivar store evaluated the new value before
/// releasing the old, so two were briefly live and one slab slot could not
/// serve it. `render_strong_ivar_assign` now releases first where the store
/// cannot read the ivar -- as `render_strong_local_assign` has since #234 --
/// so the shape needs one slot and the bar asks the emitter rather than
/// assuming. The behavioural proof that it really runs on one slot is
/// `loop_allocation_bounds::an_ivar_store_released_first_needs_only_one_slot`.
#[test]
fn alloc_into_an_ivar_in_a_loop_accepted() {
    let src = format!(
        "{}\n@interface Item : OZObject\n@end\n@implementation Item\n@end\n\
         @interface Foo : OZObject {{\n    Item *_cached;\n}}\n- (void)test;\n@end\n\
         @implementation Foo\n- (void)test {{\n    int i;\n    for (i = 0; i < 3; i++) {{\n\
         \x20       _cached = [Item alloc];\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    oz2c::transpile(&src).unwrap_or_else(|diags| {
        panic!(
            "expected an ivar store whose value cannot read it to be accepted, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}

/// The half that is still refused, and why the narrowing is not a blanket
/// acceptance: when the store *can* read the ivar the new value has to exist
/// before the old one goes, so two are briefly live. The emitter keeps a
/// hoisted temporary for that shape, and a loop lifts the temporary out of
/// itself -- so accepting this would miscompile, not merely exhaust.
#[test]
fn alloc_into_an_ivar_the_store_reads_is_rejected() {
    let src = format!(
        "{}\n@interface Item : OZObject\n@end\n@implementation Item\n@end\n\
         @interface Foo : OZObject {{\n    Item *_cached;\n}}\n- (void)test;\n@end\n\
         @implementation Foo\n- (void)test {{\n    int i;\n    for (i = 0; i < 3; i++) {{\n\
         \x20       _cached = i > 0 ? [Item alloc] : _cached;\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("Item"), "diagnostics: {}", diags);
    /* #345 replaced the old "escapes the iteration" wording with one that
       names the destination; #405 narrowed which ivar stores reach it. */
    assert!(diags.contains("an ivar"), "diagnostics: {}", diags);
}

#[test]
fn fresh_local_alloc_in_loop_accepted() {
    let src = format!(
        "{}\n@interface Item : OZObject\n- (void)ping;\n@end\n@implementation Item\n\
         - (void)ping {{\n}}\n@end\n\
         @interface Foo : OZObject\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    int i;\n    for (i = 0; i < 3; i++) {{\n\
         \x20       Item *it = [Item alloc];\n        [it ping];\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    oz2c::transpile(&src).unwrap_or_else(|diags| {
        panic!(
            "expected a fresh per-iteration local alloc to be accepted, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}

#[test]
fn unresolvable_receiver_type_rejected() {
    // Sending a message to something whose static type the transpiler
    // cannot determine (here, a param typed `id`) must be a hard error,
    // not a best-effort guess.
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)test:(id)obj;\n@end\n@implementation Foo\n\
         - (void)test:(id)obj {{\n    [obj ping];\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("cannot statically resolve"), "diagnostics: {}", diags);
}

#[test]
fn protocol_conformance_missing_method_rejected() {
    // A class declaring conformance to a protocol must actually implement
    // every method that protocol (transitively, through protocol
    // inheritance) requires -- a compile-time contract, same as real
    // Objective-C, checked here instead of left to silently produce a
    // dispatch function with a hole in it.
    let src = format!(
        "{}\n@protocol Greeter\n- (void)greet;\n@end\n\
         @interface Foo : OZObject <Greeter>\n@end\n@implementation Foo\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("Foo"), "diagnostics: {}", diags);
    assert!(diags.contains("Greeter"), "diagnostics: {}", diags);
    assert!(diags.contains("greet"), "diagnostics: {}", diags);
}

#[test]
fn protocol_conformance_satisfied_accepted() {
    let src = format!(
        "{}\n@protocol Greeter\n- (void)greet;\n@end\n\
         @interface Foo : OZObject <Greeter>\n@end\n@implementation Foo\n- (void)greet {{\n}}\n@end\n",
        PREAMBLE()
    );
    oz2c::transpile(&src).unwrap_or_else(|diags| {
        panic!(
            "expected satisfied protocol conformance to be accepted, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}

#[test]
fn selector_expression_rejected_when_the_option_is_off() {
    // '@selector(...)' is a real `selector_expression` node kind in
    // tree-sitter-objc 3.0.2 (confirmed against its node-types.json).
    // Since #226 that node is accepted and resolved to the selector's
    // generated record, so what is asserted here is the option-off
    // refusal reaching a method body -- see
    // `tests/reflection.rs` for the supported behaviour.
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)run;\n@end\n@implementation Foo\n\
         - (void)run {{\n\tSEL s = @selector(run);\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("CONFIG_OBJZ_REFLECTION"), "diagnostics: {}", diags);
}

#[test]
fn undefined_superclass_rejected() {
    // OZ-093: a class extending a superclass never declared in this
    // translation unit (e.g. a real Foundation class only ever pulled in
    // via `#import <Foundation/Foundation.h>`, which oz2c doesn't
    // resolve) must be a named, located diagnostic -- not the raw panic
    // this used to produce in `companion::topological_order`. Deliberately
    // doesn't use `PREAMBLE`: the whole point is that `OZObject` is never
    // declared anywhere in this source.
    let src = "@interface MyFirstObject : OZObject\n- (void)greet;\n@end\n\
               @implementation MyFirstObject\n- (void)greet {\n}\n@end\n";
    let diags = expect_reject(src);
    assert!(diags.contains("MyFirstObject"), "diagnostics: {}", diags);
    assert!(diags.contains("no class 'OZObject' is defined"), "diagnostics: {}", diags);
}

#[test]
fn protocol_literal_expression_rejected() {
    // '@protocol(Name)' has no dedicated `protocol_expression` node kind
    // in this grammar version -- unlike `@selector(...)`, it parses as a
    // generic `at_expression` wrapping what looks like a call to a
    // function named `protocol` (same class of bug already found and
    // fixed for `boxed_expression` in #191: a reject check that matched
    // a node kind the parser never actually emits). Still correctly
    // rejected either way (see `emit::is_protocol_literal_shape`, which
    // gives this specific shape its own clear message instead of
    // falling through to the generic boxed-literal one).
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)run;\n@end\n@implementation Foo\n\
         - (void)run {{\n\tid p = @protocol(NSObject);\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@protocol(...)"), "diagnostics: {}", diags);
}

/// Releasing an owned object ivar by hand inside `-dealloc` is rejected.
///
/// It used to be rejected for a narrower reason -- the release is already
/// emitted automatically (`companion::render_release_ivars`) and running
/// both is a double free -- by a check scoped to `-dealloc` and to ivars the
/// class owns. It is now rejected by the general rule (#428): ARC is always
/// enabled, so `-release` cannot be sent anywhere, whatever the receiver.
#[test]
fn releasing_owned_ivar_in_dealloc_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Held : OZObject
@end
@implementation Held
@end

@interface Owner : OZObject {
	Held *_held;
}
- (void)dealloc;
@end
@implementation Owner
- (void)dealloc {
	[_held release];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'-release' cannot be sent"), "diagnostics: {}", diags);
    assert!(diags.contains("ARC is always enabled"), "diagnostics: {}", diags);
}

/// **Reversal (#428).** This case used to assert the opposite: that
/// releasing an `__unsafe_unretained` ivar by hand was *accepted*, on the
/// grounds that nothing releases such an ivar automatically so the author
/// must. That reasoning does not survive ARC being unconditional -- the
/// qualifier says "this slot does not participate in ARC's retain/release",
/// not "manual sends are legal on it", and under `-fobjc-arc` Clang refuses
/// `[_seen release]` regardless of how `_seen` is qualified.
///
/// Kept as a rejection rather than deleted, because the shape is the one a
/// reader is most likely to believe is still allowed.
#[test]
fn releasing_unretained_ivar_in_dealloc_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Held : OZObject
@end
@implementation Held
@end

@interface Watcher : OZObject {
	__unsafe_unretained Held *_seen;
}
- (void)dealloc;
@end
@implementation Watcher
- (void)dealloc {
	[_seen release];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'-release' cannot be sent"), "diagnostics: {}", diags);
    assert!(diags.contains("__unsafe_unretained"), "diagnostics: {}", diags);
}

// ---------------------------------------------------------------------
// Manual retain/release/autorelease/dealloc (#428)
//
// ARC is always enabled, so these four selectors are the runtime's to
// emit and not the author's to send. The rejection is keyed on the
// *selector*, never on the receiver's shape, so the matrix below is the
// standing record that no spelling of a receiver escapes it -- a plain
// local, `self`, `super`, a bare ivar, `self->_ivar`, a cross-instance
// ivar, a property through dot syntax, an array element, a chained send,
// a parenthesized receiver, a cast receiver and an `id`-typed one. Miss
// one and the second ownership model comes back through it.
// ---------------------------------------------------------------------

/// Every receiver spelling, one row each, as `(label, receiver text)`.
const RECEIVER_SPELLINGS: &[(&str, &str)] = &[
    ("plain local", "t"),
    ("parenthesized local", "(t)"),
    ("cast local", "(Thing *)t"),
    ("id-typed local", "u"),
    ("self", "self"),
    ("super", "super"),
    ("bare ivar", "_kid"),
    ("ivar through self", "self->_kid"),
    ("ivar of another instance", "t->_kid"),
    ("property through dot syntax", "self.kid"),
    ("array element", "_arr[0]"),
    ("chained send", "[Thing alloc]"),
];

fn manual_send_source(receiver: &str, selector: &str) -> String {
    format!(
        "{}{}",
        PREAMBLE(),
        format!(
            "\
@interface Thing : OZObject {{
	Thing *_kid;
	Thing *_arr[2];
}}
@property (nonatomic) Thing *kid;
- (void)go;
@end
@implementation Thing
@synthesize kid = _kid;
- (void)go
{{
	Thing *t = [Thing alloc];
	id u = [Thing alloc];
	[{receiver} {selector}];
	(void)t;
	(void)u;
}}
@end
",
            receiver = receiver,
            selector = selector
        )
    )
}

#[test]
fn every_receiver_spelling_of_a_manual_send_is_rejected() {
    for selector in ["retain", "release", "autorelease", "dealloc"] {
        for (label, receiver) in RECEIVER_SPELLINGS {
            let diags = expect_reject(&manual_send_source(receiver, selector));
            assert!(
                diags.contains(&format!("'-{}' cannot be sent", selector)),
                "[{} {}] ({}) was not rejected by the ARC rule; diagnostics: {}",
                receiver,
                selector,
                label,
                diags
            );
        }
    }
}

/// A send nested inside a construct the bar's body scan treats as opaque or
/// never enters at all. `staticbar::walk_for_reject` returns at a
/// `block_literal`, which is why the rejection is a whole-root walk driven
/// from `collect` rather than an arm in that scan.
#[test]
fn manual_send_inside_a_block_or_nested_scope_is_rejected() {
    let bodies = [
        ("block literal", "\tvoid (^b)(void) = ^{ [t release]; };\n\t(void)b;"),
        ("synchronized body", "\t@synchronized(self) { [t release]; }"),
        ("if body", "\tif (t != 0) { [t release]; }"),
        ("for body", "\tfor (int i = 0; i < 1; i++) { [t release]; }"),
        ("ternary operand", "\tint n = (t != 0) ? ([t release], 1) : 0;\n\t(void)n;"),
    ];
    for (label, body) in bodies {
        let src = format!(
            "{}{}",
            PREAMBLE(),
            format!(
                "\
@interface Thing : OZObject
- (void)go;
@end
@implementation Thing
- (void)go
{{
	Thing *t = [Thing alloc];
{body}
	(void)t;
}}
@end
",
                body = body.replace("\\t", "\t")
            )
        );
        let diags = expect_reject(&src);
        assert!(
            diags.contains("'-release' cannot be sent"),
            "a release in a {} was not rejected; diagnostics: {}",
            label,
            diags
        );
    }
}

/// A send from a **free function** rather than a method body, which the bar
/// enters through a different function (`check_function_body`) -- and which
/// the whole-root walk does not have to be entered twice for.
#[test]
fn manual_send_in_a_free_function_is_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Thing : OZObject
@end
@implementation Thing
@end

void tick(void)
{
	Thing *t = [Thing alloc];
	[t release];
}
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'-release' cannot be sent"), "diagnostics: {}", diags);
}

/// The defect that surfaced #428: a hand release into a `static` slot.
///
/// `emit::managed_object_locals` consulted `released_by_hand` and so left
/// the slot alone; `static_object_locals` and `is_file_scope_object` did
/// not, so both releases were emitted for the *same* reference:
///
/// ```c
/// if (cached != nil) { oz_release((struct OZObject *)(cached)); }
/// (oz_release((struct OZObject *)(cached)), cached = Thing_oz_alloc());
/// ```
///
/// Two releases of one reference -- a segfault on the host, not a leak.
/// Adding the missing filter to both slot paths would have silenced it while
/// leaving the second ownership model in place, and a third slot kind added
/// later would have reintroduced it. Rejecting the input removes the
/// question: there is no longer a program whose double release has to be
/// avoided.
#[test]
fn hand_release_into_a_static_slot_is_rejected_not_miscompiled() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Thing : OZObject
@end
@implementation Thing
@end

void tick(void)
{
	static Thing *cached;

	[cached release];
	cached = [Thing alloc];
}
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'-release' cannot be sent"), "diagnostics: {}", diags);
}

/// The same shape at **file scope**, the other slot path `released_by_hand`
/// never reached (`emit::is_file_scope_object`).
#[test]
fn hand_release_into_a_file_scope_slot_is_rejected_not_miscompiled() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Thing : OZObject
@end
@implementation Thing
@end

static Thing *g_cached;

void tock(void)
{
	[g_cached release];
	g_cached = [Thing alloc];
}
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'-release' cannot be sent"), "diagnostics: {}", diags);
}

/// Declaring or defining `-retain`, `-release` or `-autorelease` is
/// refused as well, for the reason `INTRINSIC_SELECTORS` gives: with every
/// send of them a located error, such a body could never run, and silently
/// ignoring a method someone wrote is the degradation this module exists to
/// prevent. Real ARC refuses the override too.
///
/// Found by the fixtures rather than reasoned about: `selector_ownership_matrix`
/// carried `- (id)autorelease { return self; }`, which is what made its
/// `[[[Thing alloc] init] autorelease]` row work at all. Rejecting only the
/// send would have left that definition accepted and uncallable.
#[test]
fn declaring_a_selector_arc_owns_is_rejected() {
    for (kind, decl) in [
        ("declaration", "- (id)retain;\n- (void)release;\n- (id)autorelease;"),
        (
            "definition",
            "- (id)retain { return self; }\n- (void)release { }\n- (id)autorelease { return self; }",
        ),
    ] {
        let (interface_extra, impl_extra) = if kind == "declaration" {
            (decl, "")
        } else {
            ("", decl)
        };
        let src = format!(
            "{}@interface Thing : OZObject\n{}\n@end\n@implementation Thing\n{}\n@end\n",
            PREAMBLE(),
            interface_extra,
            impl_extra
        );
        let diags = expect_reject(&src);
        for selector in ["retain", "release", "autorelease"] {
            assert!(
                diags.contains(&format!("'-{}' cannot be declared or defined", selector)),
                "the {} of '-{}' was not rejected; diagnostics: {}",
                kind,
                selector,
                diags
            );
        }
    }
}

/// The contrast, and the one exception: a `-dealloc` override is supported.
/// It is the cleanup hook, the deallocation path calls it, and the chain
/// above it is called automatically (`companion::dealloc_chain`) -- so a
/// body that does real cleanup still works, without `[super dealloc]`.
#[test]
fn a_dealloc_override_is_still_supported_and_chains_automatically() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Base : OZObject
- (void)dealloc;
@end
@implementation Base
- (void)dealloc {
\tprintf(\"base cleanup\\n\");
}
@end

@interface Derived : Base
- (void)dealloc;
@end
@implementation Derived
- (void)dealloc {
\tprintf(\"derived cleanup\\n\");
}
@end

#include <stdio.h>
int main(void)
{
\t{
\t\tDerived *d = [Derived alloc];
\t\tprintf(\"alive=%d\\n\", d != 0);
\t}
\tprintf(\"done\\n\");
\treturn 0;
}
"
    );
    let stdout = compile_and_run(&src, "a_dealloc_override_is_still_supported_and_chains_automatically");
    /* Most-derived first, then the chain above it, then the root's -- the
     * order `[super dealloc]` at the end of each body produced, now
     * synthesized. `OZObject`'s own `-dealloc` is empty and prints
     * nothing. */
    assert_eq!(stdout, "alive=1\nderived cleanup\nbase cleanup\ndone\n");
}

/// `-retainCount` is rejected too -- the fifth selector, added in #436.
///
/// This test asserted the opposite until then, and the reversal is the
/// record worth keeping. #428 ruled on four selectors and left this one out
/// on the grounds that reading a count takes and gives no ownership, so it
/// is not a second ownership model. That reasoning is sound and answers a
/// different question: ARC forbids the *send* regardless of ownership, and a
/// probe settles it where an argument could not -- declared or undeclared,
/// Clang under `-fobjc-arc` answers `ARC forbids explicit message send of
/// 'retainCount'`. The rule the list now follows is one line, with nothing
/// weighed per selector: **exactly what Clang refuses.**
///
/// Reading a refcount is not lost, and never depended on the message
/// spelling. `include/oz_sdk/Foundation/OZObject.h` has called
/// `oz_retain_count()` "the only refcount entry point Objective-C
/// source may spell" since #418 -- a sentence true of the design and false
/// of the implementation until this change. The second half of this test is
/// that claim, compiled.
#[test]
fn sending_retain_count_is_rejected_and_the_c_call_replaces_it() {
    let body = "\
@interface Thing : OZObject
@end
@implementation Thing
@end
";
    let rejected = format!(
        "{}{}{}",
        PREAMBLE(),
        body,
        "\
#include <stdio.h>
int main(void)
{
	Thing *t = [Thing alloc];
	printf(\"rc=%d\\n\", [t retainCount]);
	return 0;
}
"
    );
    let diags = common::expect_reject(&rejected);
    assert!(
        diags.contains("retainCount") && diags.contains("oz_retain_count"),
        "the rejection must name the selector and the call that replaces it, got:\n{}",
        diags
    );

    /* The positive control: the same program, the sanctioned spelling. */
    let accepted = format!(
        "{}{}{}",
        PREAMBLE(),
        body,
        "\
#include <stdio.h>
int main(void)
{
	Thing *t = [Thing alloc];
	printf(\"rc=%d\\n\", oz_retain_count(t));
	return 0;
}
"
    );
    let stdout = compile_and_run(&accepted, "retain_count_via_c_call");
    assert_eq!(stdout, "rc=1\n");
}

// ---------------------------------------------------------------------
// Collection literals that escape a loop iteration (OZ-098)
//
// Pool sizing counts an allocation *site* once, however many times it
// runs (`pools::count_sites`). That is a sound floor only while each
// iteration's instance dies before the next begins, which scope-based ARC
// guarantees for a fresh per-iteration local and cannot guarantee for
// anything else. An explicit `[X alloc]` has been held to this rule all
// along; `@[...]`/`@{...}` allocate too -- both a collection object and a
// run of element slots -- so they are now held to it as well.
//
// What "escape" means here narrowed with #234, and the reason is worth
// stating: reassigning a *strong local* is not an escape. ARC releases the
// previous object before allocating the next
// (`emit::render_strong_local_assign`), so the slot goes straight back to
// the slab and one slot serves the whole loop -- measured, not argued:
// `arc_strong_locals::reassigned_local_needs_only_one_slab_slot` runs 100
// iterations on `OZArray=1` with a 2-slot item pool. What stays rejected is
// *accumulation*, where each iteration's object is still live when the next
// begins and nothing bounds the total.
//
// The cases below therefore test the accumulating shape. The
// reassign-into-a-local shape they used to test is now accepted, and is
// covered as an accepted case in `arc_strong_locals`.
//
// (These also used to note that a loop in a plain C function was not
// examined at all, `staticbar::check_method_body` being reachable only from
// the method-body renderer. #234 closed that: `check_function_body` runs the
// same scan over a free function's body.)
// ---------------------------------------------------------------------

/// Stored into a C array of pointers, one element per iteration, so every
/// array the loop builds is still live when it ends. Nothing releases them
/// and the counted single site is not a bound.
///
/// The destination is what makes this an escape: a plain local would be
/// released on each overwrite and need one slot. Only a store the emitter
/// cannot bound -- anything but a strong local it manages -- is rejected.
#[test]
fn array_literal_accumulated_in_a_loop_rejected() {
    let src = format!(
        "{}{}{}{}",
        PREAMBLE(),
        oznumber_src(),
        ozarray_src(),
        "\
@interface Keeper : OZObject
- (BOOL)run;
@end
@implementation Keeper
- (BOOL)run {
	OZArray *kept[3];
	for (int i = 0; i < 3; i++) {
		kept[i] = @[@(1), @(2)];
	}
	return kept[0] != 0;
}
@end

int main(void) { return 0; }
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("boxed array literal"), "diagnostics: {}", diags);
    assert!(
        diags.contains("an array element chosen per iteration"),
        "diagnostics: {}",
        diags
    );
}

/// The dictionary counterpart, which names itself distinctly so the
/// message points at the construct actually written.
#[test]
fn dictionary_literal_accumulated_in_a_loop_rejected() {
    let src = format!(
        "{}{}{}{}",
        PREAMBLE(),
        oznumber_src(),
        common::ozdictionary_src(),
        "\
@interface Keeper : OZObject
- (BOOL)run;
@end
@implementation Keeper
- (BOOL)run {
	OZDictionary *kept[3];
	for (int i = 0; i < 3; i++) {
		kept[i] = @{@(1): @(2)};
	}
	return kept[0] != 0;
}
@end

int main(void) { return 0; }
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("boxed dictionary literal"), "diagnostics: {}", diags);
    assert!(
        diags.contains("an array element chosen per iteration"),
        "diagnostics: {}",
        diags
    );
}

/// The contrast that keeps the rule from being over-broad: bound to a
/// fresh local declared inside the loop, the array is released at the end
/// of each iteration and one site really is one slot. Compiled and run,
/// not merely accepted, so the recycling is demonstrated rather than
/// assumed.
#[test]
fn array_literal_in_a_loop_bound_to_a_fresh_local_accepted() {
    let src = format!(
        "/* oz-item-pool: 2 */\n{}{}{}{}",
        PREAMBLE(),
        oznumber_src(),
        ozarray_src(),
        "\
@interface Keeper : OZObject
- (int)run;
@end
@implementation Keeper
- (int)run {
	int seen = 0;
	for (int i = 0; i < 4; i++) {
		OZArray *arr = @[@(1), @(2)];
		seen += (arr != 0);
	}
	return seen;
}
@end

#include <stdio.h>
int main(void) {
	Keeper *k = [Keeper alloc];
	printf(\"seen=%d\\n\", [k run]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "array_literal_in_a_loop_bound_to_a_fresh_local_accepted");
    assert_eq!(stdout, "seen=4\n");
}

/// A `+1` stored into a **parameter** is refused, in both spellings (#477 M2).
///
/// `emit::is_slot` enumerates three strong destinations -- a managed local, a
/// `static` local, a file-scope object -- and a parameter is in none of them,
/// so the store lowered to a plain C assignment that released nothing.
/// Measured in both spellings before the refusal: no release, no retain.
///
/// **Two rows because the CST spells them differently.** A free function's
/// parameter is a `parameter_declaration`; a method's is a
/// `method_parameter`, and `staticbar.rs`'s own arms note that the first
/// never sees the second. A refusal written against one spelling and
/// asserted against one spelling is the shape that reads as covering both.
///
/// Refused rather than managed, and the measurement is the argument.
/// `clang -fobjc-arc` at `-O0` copies the parameter into a strong slot and
/// retains on entry -- for *every* object parameter, assigned or not -- which
/// is what makes release-old-then-assign sound. At `-O2` the optimiser proves
/// it `readnone captures(none)` and deletes the pair. oz2c decides elision
/// statically at emit time and has no such pass, so it would pay that
/// permanently. Reusing the managed-slot store without the entry retain would
/// release the *caller's* object: a leak turned into an over-release, which is
/// the direction `ARC.md` § 1.3.1 refuses `ns_consumed` for.
#[test]
fn a_plus_one_stored_into_a_parameter_is_refused_in_both_spellings() {
	for (what, body) in [
		(
			"parameter_declaration (a free function)",
			"\
int freeFnParam(Thing *p)
{
\tp = [[Thing alloc] init];
\treturn [p tag];
}
",
		),
		(
			"method_parameter (a method)",
			"\
@interface Holder : OZObject
- (int)take:(Thing *)p;
@end
@implementation Holder
- (int)take:(Thing *)p {
\tp = [[Thing alloc] init];
\treturn [p tag];
}
@end
",
		),
	] {
		let src = format!(
			"{}{}{}",
			PREAMBLE(),
			"\
@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag { return 1; }
@end
",
			body
		);
		let diags = expect_reject(&src);
		assert!(
			diags.contains("parameter 'p'"),
			"{what}: the refusal must name the parameter, got:\n{diags}"
		);
		assert!(
			diags.contains("caller's reference"),
			"{what}: the note must say why a parameter is not ours to release, got:\n{diags}"
		);
		assert!(
			diags.contains("declare a local"),
			"{what}: the help must name the thing to write instead (#430's rule), got:\n{diags}"
		);
	}
}

/// A `+1` into an array element that is **not** an ivar is refused (#477 M4).
///
/// Reachable through exactly one spelling, and not the one the issue
/// described. Measured before the refusal:
///
/// | shape | before |
/// |---|---|
/// | ivar array, `_arr[0]` and `self->_arr[0]` | correct, release-first |
/// | local `Thing *arr[2]` | already refused -- read as ObjC subscripting |
/// | file-scope `Thing *g_arr[2]` | already refused, same |
/// | a local shadowing an ivar array | already refused, same |
/// | local `id a[2]` | **2 allocations, 0 releases** |
///
/// Every class-typed spelling is blocked upstream, because `arr[0]` on a
/// `Thing` is read as a subscript *message* and `Thing` implements neither
/// subscript selector. `id` slips through because `class_name_from_type`
/// answers `None` for it, so nothing reads `a[0]` as a send -- making `id` the
/// escape hatch for a third ownership defect after #400 and #429.
///
/// **So this test uses `id`, deliberately.** A row written against
/// `Thing *arr[2]` would pass on the pre-existing subscripting refusal while
/// testing nothing about ownership -- the vacuous shape this file's header
/// warns about.
///
/// The second row is the control: an ivar array is the supported spelling and
/// must keep its release-first store. A refusal that also caught the ivar case
/// would read as passing here while breaking the only form that works.
#[test]
fn a_plus_one_into_a_non_ivar_array_element_is_refused() {
	let leaking = format!(
		"{}{}",
		PREAMBLE(),
		"\
@interface Thing : OZObject
@end
@implementation Thing
@end
void intoIdArray(void)
{
\tid a[2];
\ta[0] = [[Thing alloc] init];
\t(void)a;
}
"
	);
	let diags = expect_reject(&leaking);
	assert!(
		diags.contains("element of 'a'"),
		"the refusal must name the array, got:\n{diags}"
	);
	assert!(
		diags.contains("only an array ivar is a strong slot"),
		"the note must say why a non-ivar array cannot be managed, got:\n{diags}"
	);
	assert!(
		diags.contains("declare the array as an ivar"),
		"the help must name the supported spelling (#430's rule), got:\n{diags}"
	);
}
