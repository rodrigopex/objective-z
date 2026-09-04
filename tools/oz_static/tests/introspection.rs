// SPDX-License-Identifier: Apache-2.0
//
// introspection.rs -- `-isKindOfClass:` and `-conformsToProtocol:`, the two
// introspection selectors that generate a table, and the
// `CONFIG_OBJZ_INTROSPECTION` option that gates them (#226).
//
// Both answer a question about the receiver's *actual* class, which is why
// they need a runtime table at all: a declared type is only an upper bound
// (`Base *b = (Base *)[Sub alloc];` is still a `Sub`), so
// `Program::is_descendant_of` cannot be folded away at the call site the
// way `[Foo class]` can. The relation it walks is instead emitted as
// `oz_superclass_of`, indexed by class id.
//
// The tables are gated on *use*, not on the option: that is what keeps the
// promise that a program enabling introspection and never introspecting
// pays nothing, so several tests here assert on absence.

mod common;
use common::{
    compile_and_run_with_introspection, expect_reject, expect_reject_with_introspection,
    ozobject_src as PREAMBLE,
};

/// `Gadget : Widget : OZObject`, `Plain : OZObject`, and a `Togglable`
/// protocol that `Widget` declares and `Gadget` inherits.
///
/// Inheritance on both axes is the point: a one-class program would pass
/// with an `==` in place of the ancestry walk, and a protocol declared
/// directly by every conformer would pass without
/// `class_conforms_to`'s superclass chain.
fn hierarchy() -> &'static str {
    "\
@protocol Togglable
- (int)toggle;
@end

@interface Widget : OZObject <Togglable>
- (int)toggle;
@end
@implementation Widget
- (int)toggle {
	return 1;
}
@end

@interface Gadget : Widget
@end
@implementation Gadget
@end

@interface Plain : OZObject
@end
@implementation Plain
@end
"
}

/// `-isKindOfClass:` walks the real superclass chain, and answers NO for
/// nil rather than dereferencing it.
#[test]
fn is_kind_of_class_follows_the_superclass_chain() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
#include <stdio.h>
int main(void) {
	Gadget *g = [Gadget alloc];
	Plain *p = [Plain alloc];
	Widget *none = nil;
	printf(\"gw=%d gg=%d go=%d pw=%d nil=%d\\n\",
	       [g isKindOfClass:[Widget class]],
	       [g isKindOfClass:[Gadget class]],
	       [g isKindOfClass:[OZObject class]],
	       [p isKindOfClass:[Widget class]],
	       [none isKindOfClass:[Widget class]]);
	return 0;
}
"
    );
    let out = compile_and_run_with_introspection(&src, "is_kind_of_class_chain");
    assert_eq!(out, "gw=1 gg=1 go=1 pw=0 nil=0\n", "unexpected: {}", out);
}

/// `-conformsToProtocol:` sees a protocol a class inherits its conformance
/// to, not only one it declares itself.
#[test]
fn conforms_to_protocol_is_inherited() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
#include <stdio.h>
int main(void) {
	Widget *w = [Widget alloc];
	Gadget *g = [Gadget alloc];
	Plain *p = [Plain alloc];
	Widget *none = nil;
	printf(\"w=%d g=%d p=%d nil=%d\\n\",
	       [w conformsToProtocol:@protocol(Togglable)],
	       [g conformsToProtocol:@protocol(Togglable)],
	       [p conformsToProtocol:@protocol(Togglable)],
	       [none conformsToProtocol:@protocol(Togglable)]);
	return 0;
}
"
    );
    let out = compile_and_run_with_introspection(&src, "conforms_to_protocol_inherited");
    assert_eq!(out, "w=1 g=1 p=0 nil=0\n", "unexpected: {}", out);
}

/// With the option off, both selectors are located errors that name it --
/// never quietly unavailable, and never silently degraded to something
/// weaker.
#[test]
fn the_option_being_off_is_a_located_error_naming_it() {
    for (selector, call) in [
        ("-isKindOfClass:", "[g isKindOfClass:[Widget class]]"),
        ("-conformsToProtocol:", "[g conformsToProtocol:@protocol(Togglable)]"),
    ] {
        let src = format!(
            "{}{}#include <stdio.h>\nint main(void) {{\n\tGadget *g = [Gadget alloc];\n\tprintf(\"%d\\n\", {});\n\treturn 0;\n}}\n",
            PREAMBLE(),
            hierarchy(),
            call
        );
        let diags = expect_reject(&src);
        assert!(
            diags.contains(selector) && diags.contains("CONFIG_OBJZ_INTROSPECTION"),
            "'{}' with the option off must name the option, got:\n{}",
            selector,
            diags
        );
    }
}

/// A program that enables introspection and never introspects gets no
/// table and no helper.
///
/// This is the footprint promise, asserted rather than described: gating
/// on the option instead of on use would have added the superclass chain
/// and both helpers -- measured at 94 bytes of flash for a 13-class
/// program on Cortex-M3 at `-Os` -- to every build that merely left the
/// default alone.
#[test]
fn enabling_the_option_alone_generates_no_table() {
    let src = format!("{}{}", PREAMBLE(), hierarchy());
    let options = oz_static::Options { introspection: true, ..Default::default() };
    let out = oz_static::transpile_with_options(&src, &options).expect("should transpile");
    for absent in ["oz_superclass_of", "oz_is_kind_of", "oz_conforms", "oz_proto_Togglable"] {
        assert!(
            !out.companion_c.contains(absent) && !out.companion_h.contains(absent),
            "'{}' must not be emitted for a program that never introspects",
            absent
        );
    }
}

/// Only the protocols a call site actually named get a bitmap, and the
/// ancestry table appears only when `-isKindOfClass:` does.
#[test]
fn only_the_constructs_used_are_emitted() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
#include <stdio.h>
int main(void) {
	Gadget *g = [Gadget alloc];
	printf(\"%d\\n\", [g conformsToProtocol:@protocol(Togglable)]);
	return 0;
}
"
    );
    let options = oz_static::Options { introspection: true, ..Default::default() };
    let out = oz_static::transpile_with_options(&src, &options).expect("should transpile");
    assert!(
        out.companion_c.contains("oz_proto_Togglable") && out.companion_c.contains("oz_conforms"),
        "the named protocol's bitmap and its reader must be emitted:\n{}",
        out.companion_c
    );
    assert!(
        !out.companion_c.contains("oz_superclass_of") && !out.companion_c.contains("oz_is_kind_of"),
        "nothing used -isKindOfClass:, so the ancestry table must be absent:\n{}",
        out.companion_c
    );
}

/// The conformance bitmap is keyed by class id and set for exactly the
/// conforming classes.
///
/// Pins the table's contents, not just its presence: `Widget` (id 1) and
/// `Gadget` (id 2) conform, `OZObject` (0) and `Plain` (3) do not, so the
/// word is 0b0110.
#[test]
fn the_conformance_bitmap_is_keyed_by_class_id() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
#include <stdio.h>
int main(void) {
	Gadget *g = [Gadget alloc];
	printf(\"%d\\n\", [g conformsToProtocol:@protocol(Togglable)]);
	return 0;
}
"
    );
    let options = oz_static::Options { introspection: true, ..Default::default() };
    let out = oz_static::transpile_with_options(&src, &options).expect("should transpile");
    assert!(
        out.companion_c.contains("oz_proto_Togglable[1] = { 0x00000006u }"),
        "expected bits for Widget (1) and Gadget (2) only:\n{}",
        out.companion_c
    );
}

/// `@protocol(...)` is accepted only as `-conformsToProtocol:`'s argument.
///
/// It resolves to a `const uint32_t *` bitmap, which is meaningful only to
/// `oz_conforms`. Letting it escape into a variable would hand source that
/// thinks it holds a protocol a pointer to a bitmap, so the position is
/// part of the contract.
#[test]
fn a_protocol_literal_is_rejected_outside_conforms_to_protocol() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
int main(void) {
	id p = @protocol(Togglable);
	(void)p;
	return 0;
}
"
    );
    let diags = expect_reject_with_introspection(&src);
    assert!(
        diags.contains("@protocol(...)") && diags.contains("conformsToProtocol:"),
        "a stray protocol literal must be refused, got:\n{}",
        diags
    );
}

/// A `@protocol(...)` naming something the program never declared is a
/// located error, not a bitmap of zeros.
///
/// Emitting an all-zero bitmap would make `-conformsToProtocol:` answer
/// NO for every class, which is indistinguishable from a correct answer
/// and hides the typo.
#[test]
fn an_unknown_protocol_name_is_rejected() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        hierarchy(),
        "\
#include <stdio.h>
int main(void) {
	Gadget *g = [Gadget alloc];
	printf(\"%d\\n\", [g conformsToProtocol:@protocol(Nonexistent)]);
	return 0;
}
"
    );
    let diags = expect_reject_with_introspection(&src);
    assert!(
        diags.contains("Nonexistent") && diags.contains("names no protocol"),
        "an undeclared protocol must be refused, got:\n{}",
        diags
    );
}
