// SPDX-License-Identifier: Apache-2.0
//
// class_objects.rs -- `Class` as the `class_id` integer, and the two
// constructs that need no table: `+class`/`-class` and `-isMemberOfClass:`
// (#226).
//
// These are the always-available half of #226: `CONFIG_OBJZ_INTROSPECTION`
// gates `-isKindOfClass:` and `-conformsToProtocol:`, which generate
// tables, but a class *identity* costs nothing -- `[Foo class]` is a
// compile-time constant and `[obj class]` is a read of a bitfield every
// object already carries -- so there is nothing to switch off.
//
// The first test is also a regression test. `+class` is declared once on
// the root class, so before the fix `find_defining_class` routed
// `[Widget class]` to `OZObject_class_cls()`: the receiver's class was
// dropped, making `[Widget class]` and `[Gadget class]` the same
// expression, and no such function is generated anywhere, so a program
// calling it failed at *link* time with an undefined symbol instead of at
// transpile time with a located message. Nothing under `samples/`,
// `tests/behavior/cases/` or `tests/adapted/` writes `[X class]`, which is
// why it survived. `compile_and_run` links, so any test here would have
// caught it.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// A three-class hierarchy: `Gadget` inherits from `Widget`, so it is a
/// *kind of* Widget but not a *member* of Widget.
fn hierarchy() -> &'static str {
    "\
@interface Widget : OZObject
@end
@implementation Widget
@end

@interface Gadget : Widget
@end
@implementation Gadget
@end
"
}

/// `[Foo class]` is a distinct compile-time constant per class, and
/// `[obj class]` reports the receiver's own runtime class.
#[test]
fn class_of_a_literal_and_of_an_instance_agree_and_distinguish() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
#include <stdio.h>
int main(void) {
	Widget *w = [Widget alloc];
	Gadget *g = [Gadget alloc];
	printf(\"distinct=%d w=%d g=%d\\n\",
	       [Widget class] != [Gadget class],
	       [w class] == [Widget class],
	       [g class] == [Gadget class]);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "class_literal_and_instance");
    assert_eq!(out, "distinct=1 w=1 g=1\n", "unexpected: {}", out);
}

/// The generated C must name neither the dropped-receiver call nor any
/// class-object pointer.
///
/// Pins the fix at the level the defect lived at: `[Widget class]` has to
/// become `OZ_STATIC_CLASS_Widget`, not a call to a function that exists
/// nowhere. Without the fix the previous test fails too, but as a linker
/// error whose message names no source line -- this one says why.
#[test]
fn class_emits_a_constant_rather_than_a_call() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
@interface Driver : OZObject
- (Class)which;
@end
@implementation Driver
- (Class)which {
	return [Widget class];
}
@end
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.source_c.contains("return OZ_STATIC_CLASS_Widget;"),
        "[Widget class] must emit the class constant:\n{}",
        out.source_c
    );
    assert!(
        !out.source_c.contains("OZObject_class_cls"),
        "the dropped-receiver call must be gone from the output:\n{}",
        out.source_c
    );
    assert!(
        out.companion_h.contains("typedef uint16_t Class;"),
        "Class must be the class_id integer:\n{}",
        out.companion_h
    );
}

/// `-isMemberOfClass:` is exact class equality, not an ancestry test: a
/// `Gadget` is not a member of `Widget` however much it inherits from it.
/// That distinction is the whole reason it is a separate selector from
/// `-isKindOfClass:`, so a test that used only one class would pass with
/// either semantics.
#[test]
fn is_member_of_class_is_exact_not_ancestral() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
#include <stdio.h>
int main(void) {
	Widget *w = [Widget alloc];
	Gadget *g = [Gadget alloc];
	printf(\"ww=%d gw=%d gg=%d\\n\",
	       [w isMemberOfClass:[Widget class]],
	       [g isMemberOfClass:[Widget class]],
	       [g isMemberOfClass:[Gadget class]]);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "is_member_of_class_exact");
    assert_eq!(out, "ww=1 gw=0 gg=1\n", "unexpected: {}", out);
}

/// A message to nil answers the way Objective-C does -- `Nil` and `NO` --
/// rather than dereferencing a null receiver.
///
/// `oz_class_of` carries the guard, so every construct built on it
/// inherits the behaviour instead of each needing its own check. `Nil` is
/// 0xFFFF, which a 10-bit `class_id` can never hold, so it collides with
/// no real class.
#[test]
fn a_nil_receiver_has_no_class_and_is_a_member_of_nothing() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
#include <stdio.h>
int main(void) {
	Widget *w = nil;
	printf(\"nil_class=%d member=%d\\n\", [w class] == Nil, [w isMemberOfClass:[Widget class]]);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "nil_receiver_class");
    assert_eq!(out, "nil_class=1 member=0\n", "unexpected: {}", out);
}

/// Defining `-class` or `-isMemberOfClass:` is a located error, not a
/// silently ignored body.
///
/// The emitter answers both at the call site, so an implementation would
/// never be reached -- every caller keeps the compile-time constant. A
/// method that looks implemented and never runs is worse than one that is
/// refused, so `check_method_body` refuses it.
#[test]
fn overriding_an_intrinsic_selector_is_rejected() {
    for selector in ["class", "isMemberOfClass:"] {
        let body = if selector == "class" {
            "- (Class)class {\n\treturn OZ_STATIC_CLASS_Widget;\n}"
        } else {
            "- (BOOL)isMemberOfClass:(Class)aClass {\n\treturn NO;\n}"
        };
        let src = format!(
            "{}@interface Widget : OZObject\n@end\n@implementation Widget\n{}\n@end\n",
            PREAMBLE(),
            body
        );
        let diags = common::expect_reject(&src);
        assert!(
            diags.contains(selector) && diags.contains("cannot be overridden"),
            "overriding '{}' must be refused, got:\n{}",
            selector,
            diags
        );
    }
}
