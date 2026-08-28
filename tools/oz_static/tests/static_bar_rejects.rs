// SPDX-License-Identifier: Apache-2.0
//
// static_bar_rejects.rs - OZ-091 Track B: constructs outside the static
// subset must be a named, located hard error -- never a silent skip.

mod common;
use common::expect_reject;

const PREAMBLE: &str = "\
@interface OZSRoot
- (void)dealloc;
@end
@implementation OZSRoot
- (void)dealloc {
}
@end
";

#[test]
fn try_catch_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZSRoot\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    @try {{\n        int x = 1;\n    }} @catch (id e) {{\n    }}\n}}\n@end\n",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@try/@catch"), "diagnostics: {}", diags);
}

#[test]
fn synchronized_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZSRoot\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    @synchronized(self) {{\n    }}\n}}\n@end\n",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@synchronized"), "diagnostics: {}", diags);
}

#[test]
fn property_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZSRoot\n@property (nonatomic) int count;\n@end\n\
         @implementation Foo\n@synthesize count = _count;\n@end\n",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@property") || diags.contains("@synthesize"), "diagnostics: {}", diags);
}

#[test]
fn reflection_selector_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZSRoot\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    if ([self respondsToSelector:0]) {{\n    }}\n}}\n@end\n",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("respondsToSelector:"), "diagnostics: {}", diags);
    assert!(diags.contains("reflection"), "diagnostics: {}", diags);
}

#[test]
fn is_kind_of_class_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZSRoot\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    if ([self isKindOfClass:0]) {{\n    }}\n}}\n@end\n",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("isKindOfClass:"), "diagnostics: {}", diags);
}

#[test]
fn capturing_block_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZSRoot\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    int local = 5;\n    void (^blk)(void) = ^{{\n        local;\n    }};\n}}\n@end\n",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("captures 'local'"), "diagnostics: {}", diags);
}

#[test]
fn self_capturing_block_rejected() {
    let src = format!(
        "{}\n@interface Foo : OZSRoot {{\n    int _x;\n}}\n- (void)test;\n@end\n\
         @implementation Foo\n- (void)test {{\n    void (^blk)(void) = ^{{\n        _x;\n    }};\n}}\n@end\n",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("captures"), "diagnostics: {}", diags);
}

#[test]
fn non_capturing_block_accepted() {
    let src = format!(
        "{}\n@interface Foo : OZSRoot\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    void (^blk)(void) = ^{{\n        int y = 1;\n    }};\n}}\n@end\n",
        PREAMBLE
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
        "{}\n@interface Item : OZSRoot\n@end\n@implementation Item\n@end\n\
         @interface Foo : OZSRoot {{\n    Item *_cached;\n}}\n- (void)test;\n@end\n\
         @implementation Foo\n- (void)test {{\n    int i;\n    for (i = 0; i < 3; i++) {{\n\
         \x20       _cached = [Item alloc];\n    }}\n}}\n@end\n",
        PREAMBLE
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("Item"), "diagnostics: {}", diags);
    assert!(diags.contains("escapes the iteration"), "diagnostics: {}", diags);
}

#[test]
fn fresh_local_alloc_in_loop_accepted() {
    let src = format!(
        "{}\n@interface Item : OZSRoot\n- (void)ping;\n@end\n@implementation Item\n\
         - (void)ping {{\n}}\n@end\n\
         @interface Foo : OZSRoot\n- (void)test;\n@end\n@implementation Foo\n\
         - (void)test {{\n    int i;\n    for (i = 0; i < 3; i++) {{\n\
         \x20       Item *it = [Item alloc];\n        [it ping];\n        [it release];\n    }}\n}}\n@end\n",
        PREAMBLE
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
        "{}\n@interface Foo : OZSRoot\n- (void)test:(id)obj;\n@end\n@implementation Foo\n\
         - (void)test:(id)obj {{\n    [obj ping];\n}}\n@end\n",
        PREAMBLE
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
         @interface Foo : OZSRoot <Greeter>\n@end\n@implementation Foo\n@end\n",
        PREAMBLE
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
         @interface Foo : OZSRoot <Greeter>\n@end\n@implementation Foo\n- (void)greet {{\n}}\n@end\n",
        PREAMBLE
    );
    oz_static::transpile(&src).unwrap_or_else(|diags| {
        panic!(
            "expected satisfied protocol conformance to be accepted, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}
