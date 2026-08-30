// SPDX-License-Identifier: Apache-2.0
//
// static_bar_rejects.rs - OZ-091 Track B: constructs outside the static
// subset must be a named, located hard error -- never a silent skip.

mod common;
use common::{expect_reject, ozobject_src as PREAMBLE};

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
#[test]
fn weak_ivar_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject {{\n\t__weak id _delegate;\n}}\n@end\n\
         @implementation Foo\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'__weak' ivars are not supported"), "diagnostics: {}", diags);
    assert!(diags.contains("unsafe_unretained"), "diagnostics: {}", diags);
}

#[test]
fn reflection_selector_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    if ([self respondsToSelector:0]) {{\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("respondsToSelector:"), "diagnostics: {}", diags);
    assert!(diags.contains("reflection"), "diagnostics: {}", diags);
}

#[test]
fn is_kind_of_class_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    if ([self isKindOfClass:0]) {{\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("isKindOfClass:"), "diagnostics: {}", diags);
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
    oz_static::transpile(&src).unwrap_or_else(|diags| {
        panic!(
            "expected a non-capturing block to be accepted, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}

#[test]
fn escaping_alloc_in_loop_rejected() {
    let src = format!(
        "{}\n@interface Item : OZObject\n@end\n@implementation Item\n@end\n\
         @interface Foo : OZObject {{\n    Item *_cached;\n}}\n- (void)test;\n@end\n\
         @implementation Foo\n- (void)test {{\n    int i;\n    for (i = 0; i < 3; i++) {{\n\
         \x20       _cached = [Item alloc];\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("Item"), "diagnostics: {}", diags);
    assert!(diags.contains("escapes the iteration"), "diagnostics: {}", diags);
}

#[test]
fn fresh_local_alloc_in_loop_accepted() {
    let src = format!(
        "{}\n@interface Item : OZObject\n- (void)ping;\n@end\n@implementation Item\n\
         - (void)ping {{\n}}\n@end\n\
         @interface Foo : OZObject\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    int i;\n    for (i = 0; i < 3; i++) {{\n\
         \x20       Item *it = [Item alloc];\n        [it ping];\n        [it release];\n    }}\n}}\n@end\n",
        PREAMBLE()
    );
    oz_static::transpile(&src).unwrap_or_else(|diags| {
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
    oz_static::transpile(&src).unwrap_or_else(|diags| {
        panic!(
            "expected satisfied protocol conformance to be accepted, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}

#[test]
fn selector_expression_rejected() {
    // '@selector(...)' is a real `selector_expression` node kind in
    // tree-sitter-objc 3.0.2 (confirmed against its node-types.json) --
    // rejected directly via that node kind.
    let src = format!(
        "{}\n@interface Foo : OZObject\n- (void)run;\n@end\n@implementation Foo\n\
         - (void)run {{\n\tSEL s = @selector(run);\n}}\n@end\n",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("selector_expression"), "diagnostics: {}", diags);
}

#[test]
fn undefined_superclass_rejected() {
    // OZ-093: a class extending a superclass never declared in this
    // translation unit (e.g. a real Foundation class only ever pulled in
    // via `#import <Foundation/Foundation.h>`, which oz_static doesn't
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
