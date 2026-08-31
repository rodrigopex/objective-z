// SPDX-License-Identifier: Apache-2.0
//
// static_bar_rejects.rs - OZ-091 Track B: constructs outside the static
// subset must be a named, located hard error -- never a silent skip.

mod common;
use common::{
    compile_and_run, expect_reject, ozarray_src, ozobject_src as PREAMBLE, ozq31_src,
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

/// Releasing an owned object ivar by hand inside `-dealloc` is rejected,
/// because the release is already emitted automatically
/// (`companion::render_release_ivars`) and running both is a double free.
///
/// A deliberate divergence from the oracle rather than a port of it:
/// `emit.py::_emit_user_dealloc` appends the automatic releases *after* the
/// user's body, so ordinary manual-retain/release teardown has every owned
/// ivar released twice, silently. Real ARC does not compensate for that
/// either -- it makes the explicit `release` a compile error, which is the
/// rule taken here.
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
    assert!(diags.contains("released automatically"), "diagnostics: {}", diags);
    assert!(diags.contains("_held"), "diagnostics: {}", diags);
}

/// The contrast: an `__unsafe_unretained` ivar is *not* released
/// automatically, so releasing it by hand stays legal. The rejection is
/// scoped to ivars the class actually owns.
#[test]
fn releasing_unretained_ivar_in_dealloc_accepted() {
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
    assert!(
        oz_static::transpile(&src).is_ok(),
        "releasing an __unsafe_unretained ivar should be accepted"
    );
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
        ozq31_src(),
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
    assert!(diags.contains("escapes the iteration"), "diagnostics: {}", diags);
}

/// The dictionary counterpart, which names itself distinctly so the
/// message points at the construct actually written.
#[test]
fn dictionary_literal_accumulated_in_a_loop_rejected() {
    let src = format!(
        "{}{}{}{}",
        PREAMBLE(),
        ozq31_src(),
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
    assert!(diags.contains("escapes the iteration"), "diagnostics: {}", diags);
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
        ozq31_src(),
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
