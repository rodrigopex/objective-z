//! `OZObjectProtocol` -- oz_sdk's `<NSObject>` (#307).
//!
//! Clang resolves a message sent to `id<P>` against `P` and its
//! super-protocols and nowhere else, so
//! `[someIdProto conformsToProtocol:...]` was
//! `error: no known instance method for selector 'conformsToProtocol:'`
//! even though `OZObject` declares it. The answer is real Objective-C's:
//! a base protocol carrying the introspection methods, which every other
//! protocol adopts.
//!
//! Two halves, and each of these tests fails without one of them:
//!
//!   * the header half -- `OZObjectProtocol.h` declaring all ten methods
//!     (it declared two), imported by `OZObject.h`, and adopted by
//!     `OZObject` and by both SDK protocols;
//!   * the transpiler half -- `Program::implements_selector`, so an
//!     *inherited* implementation satisfies a protocol requirement.
//!     Without it every one of these programs is rejected with
//!     "declares conformance to 'X' but doesn't implement 'isEqual:'"
//!     for nine methods no conforming class ever restates.
//!
//! The Clang diagnostic that started it is not visible from here: this
//! harness compiles the *generated C* with `cc` and never parses the
//! Objective-C. What guards that end is `oz_static.cmake`'s AST-dump
//! check, which since #307 fails the build on an ordinary error and not
//! only a fatal one.

mod common;

use oz_static::collect;

/// Every method `OZObject` declares, `OZObjectProtocol` declares too.
///
/// The list is not restated here: it is read out of `OZObject`'s own
/// interface, so a method added to the root class and forgotten in the
/// protocol fails this rather than passing quietly. `-init`, `-dealloc`,
/// `+alloc` and friends are excluded -- lifecycle, not introspection, and
/// a protocol requiring `-init` would demand every conforming class
/// restate it.
#[test]
fn object_protocol_declares_what_the_root_class_does() {
    let (program, diags) = collect::collect(&common::ozobject_src());
    assert!(diags.is_empty(), "OZObject should collect cleanly: {:?}", diags);

    let declared: Vec<String> = program
        .protocol_methods("OZObjectProtocol")
        .into_iter()
        .map(|m| m.selector)
        .collect();

    const LIFECYCLE: &[&str] = &["init", "dealloc", "alloc", "allocWithHeap:"];
    let root = &program.classes["OZObject"];
    let expected: Vec<&str> = root
        .methods
        .iter()
        .filter(|m| !m.is_class_method && !LIFECYCLE.contains(&m.selector.as_str()))
        .map(|m| m.selector.as_str())
        .collect();

    assert!(!expected.is_empty(), "OZObject should declare instance methods");
    for selector in &expected {
        assert!(
            declared.contains(&selector.to_string()),
            "OZObjectProtocol is missing '{}', which OZObject declares -- \
             a protocol-qualified receiver cannot be sent it",
            selector
        );
    }
    assert!(
        declared.contains(&"conformsToProtocol:".to_string()),
        "the selector #307 was filed about"
    );
}

/// `OZObject` adopts it, so `conformsToProtocol:@protocol(OZObjectProtocol)`
/// answers YES rather than silently NO -- `class_conforms_to` reads the
/// declared conformance list, not the method table, so declaring the
/// methods is not on its own enough.
#[test]
fn the_root_class_adopts_it() {
    let (program, _) = collect::collect(&common::ozobject_src());
    assert!(
        program.class_conforms_to("OZObject", "OZObjectProtocol"),
        "OZObject must declare <OZObjectProtocol>, not merely implement its methods"
    );
}

/// Both SDK protocols adopt it, so an `id<OZSingletonProtocol>` or
/// `id<OZIteratorProtocol>` receiver can be introspected too.
#[test]
fn the_sdk_protocols_adopt_it() {
    let src = format!(
        "{}\n{}\n{}\n",
        common::ozobject_src(),
        common::iterator_protocol_src(),
        common::singleton_protocol_src()
    );
    let (program, diags) = collect::collect(&src);
    assert!(diags.is_empty(), "should collect cleanly: {:?}", diags);

    for protocol in ["OZIteratorProtocol", "OZSingletonProtocol"] {
        let inherited: Vec<String> = program
            .protocol_methods(protocol)
            .into_iter()
            .map(|m| m.selector)
            .collect();
        assert!(
            inherited.contains(&"respondsToSelector:".to_string()),
            "{} should adopt <OZObjectProtocol> and so inherit its methods; got {:?}",
            protocol,
            inherited
        );
    }
}

/// An inherited implementation satisfies a protocol requirement.
///
/// This is the half that makes the header change usable at all. A class
/// adopting a protocol that adopts `<OZObjectProtocol>` implements none of
/// the ten methods itself -- `OZObject` does, once -- and before #307 the
/// conformance check read only the class's own method list and rejected
/// the program ten times over.
#[test]
fn conformance_counts_inherited_implementations() {
    let source = format!(
        r#"{}
@protocol Toggleable <OZObjectProtocol>
- (void)toggle;
@end

@interface Lamp : OZObject <Toggleable>
{{
	int _on;
}}
- (void)toggle;
- (int)isOn;
@end

@implementation Lamp
- (void)toggle
{{
	_on = !_on;
}}
- (int)isOn
{{
	return _on;
}}
@end

int main(void)
{{
	Lamp *lamp = [[Lamp alloc] init];
	[lamp toggle];
	printf("on=%d\n", [lamp isOn]);
	printf("kind=%d\n", (int)[lamp isKindOfClass:[OZObject class]]);
	return 0;
}}
"#,
        common::ozobject_src()
    );

    let out = common::compile_and_run_with_introspection(&source, "conformance_inherited");
    assert!(out.contains("on=1"), "unexpected output: {}", out);
    assert!(out.contains("kind=1"), "unexpected output: {}", out);
}

/// The runtime answer through a protocol-qualified receiver, which is the
/// shape #307 was filed about: a variable typed `id<Toggleable>` asked
/// whether it conforms to a *refinement* of that protocol.
///
/// Both answers matter. `Dimmable` extends `Toggleable`, so a `Lamp`
/// declaring only `<Toggleable>` must answer NO for `Dimmable` and YES
/// for `Toggleable` -- conformance follows the declared list and protocol
/// inheritance, not "does it happen to implement the methods".
#[test]
fn conforms_to_protocol_through_a_protocol_qualified_receiver() {
    let source = format!(
        r#"{}
@protocol Toggleable <OZObjectProtocol>
- (void)toggle;
@end

@protocol Dimmable <Toggleable>
- (void)setLevel:(int)level;
@end

@interface Lamp : OZObject <Toggleable>
{{
	int _on;
}}
- (void)toggle;
@end

@implementation Lamp
- (void)toggle
{{
	_on = !_on;
}}
@end

int main(void)
{{
	Lamp *lamp = [[Lamp alloc] init];
	printf("toggleable=%d\n", (int)[lamp conformsToProtocol:@protocol(Toggleable)]);
	printf("dimmable=%d\n", (int)[lamp conformsToProtocol:@protocol(Dimmable)]);
	return 0;
}}
"#,
        common::ozobject_src()
    );

    let out = common::compile_and_run_with_introspection(&source, "conforms_via_id_proto");
    assert!(out.contains("toggleable=1"), "unexpected output: {}", out);
    assert!(out.contains("dimmable=0"), "unexpected output: {}", out);
}
