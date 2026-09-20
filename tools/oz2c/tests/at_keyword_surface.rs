// SPDX-License-Identifier: Apache-2.0
//
// at_keyword_surface.rs - which of Objective-C's `@`-keywords the static
// subset accepts, refuses, or delegates (#563, #564).
//
// The static subset accepted a *positive list* of `@`-keywords and refused
// a few more by name. A keyword in neither was **neither accepted nor
// refused**: tree-sitter parsed it, no pass had a case for it, and emit's
// catch-all copied the source text through -- so the first thing in the
// toolchain that understood it was GCC, complaining about a line the author
// wrote in a file they never saw.
//
// The three-way split is the point of this file, and it is not derivable
// from "is this keyword implemented":
//
//   REFUSED    `@encode`, `@throw`, `@available`, `@defs`, a handler-less
//              `@try` -- no meaning in this backend to lower to (#563).
//   SUPPORTED  `@class` -- a forward declaration is not an operation, so
//              there was nothing to lower in the first place (#564).
//   DELEGATED  `@import` -- a front-end feature. Clang's own dump rejects
//              it, `oz2c-challenges/MUTATIONS.md` grades it `CLANG`, and
//              that grade is correct. A check that refused "every
//              unrecognised `@`-keyword" would take this with it, which is
//              why #563 is five names and not a category.
//
// Node kinds below were read out of a tree dump, not out of the grammar's
// type list. Three of the five are not what their names suggest --
// `@defs` has no rule at all and arrives as a generic `at_expression`, and
// a handler-less `@try` arrives as an `ERROR` -- which is the fifth and
// sixth time in this tree that the obvious node was the wrong one.

mod common;
use common::{compile_and_run, expect_reject, oznumber_src, ozobject_src as PREAMBLE};

/* --------------------------------------------------------- #563 refusals */

#[test]
fn encode_operator_refused() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	const char *c = @encode(int);
	(void)c;
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'@encode' is not in the static subset"), "{}", diags);
    assert!(diags.contains("no runtime type strings"), "{}", diags);
}

#[test]
fn throw_statement_refused() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	@throw nil;
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'@throw' is not in the static subset"), "{}", diags);
    assert!(diags.contains("unwinding"), "{}", diags);
}

#[test]
fn available_check_refused() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	if (@available(macos 10.12, *)) { return 67; }
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'@available' is not in the static subset"), "{}", diags);
}

/// `@defs` at **file scope**, which is where the corpus fixture puts it and
/// where nothing caught it. The body position was already refused -- by the
/// generic `at_expression` catch-all, with a generic message -- so the two
/// halves of one construct disagreed, and a body-only check would have
/// looked like a fix while leaving the reported case untouched.
#[test]
fn defs_operator_at_file_scope_refused() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@defs(P);

@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'@defs' is not in the static subset"), "{}", diags);
    assert!(diags.contains("ivar-layout"), "{}", diags);
}

/// The same construct in a body: one diagnostic, and the *named* one.
///
/// Before #563 this position was refused by the generic message ("this
/// '@'-boxed expression is not in the static subset's accepted construct
/// set"), because `@defs(P)` is an `at_expression` that is neither
/// numeric-boxed nor `@protocol`-shaped. Both firing would be two
/// diagnostics for one mistake, so `walk_for_reject` now defers this shape.
#[test]
fn defs_operator_in_a_body_is_named_once() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	@defs(P);
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'@defs' is not in the static subset"), "{}", diags);
    /* The generic message must not also fire. Narrow needle: the phrase
     * below belongs to that one diagnostic and to no other. */
    assert!(
        !diags.contains("accepted construct set"),
        "the generic at_expression refusal fired as well:\n{}",
        diags
    );
}

/// A `@try` with no handler. tree-sitter builds a `try_statement` only once
/// a `@catch`/`@finally` follows, so the existing refusal never saw this --
/// the parser hands back an `ERROR` whose first child is the `@try` token.
#[test]
fn handlerless_try_refused_as_exceptions_not_as_syntax() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	@try { return 70; }
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'@try' is not in the static subset"), "{}", diags);
    assert!(diags.contains("unwinding"), "{}", diags);
    /* The distinction #563 asks for: this is the exceptions refusal, not a
     * complaint about the parse. */
    assert!(diags.contains("no '@catch' or '@finally'"), "{}", diags);
}

/// The handled form still takes the **existing** `try_statement` arm, with
/// its own wording. Two refusals of one feature would be a regression in
/// the other direction.
#[test]
fn handled_try_keeps_its_original_refusal() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	@try { return 1; } @catch (id e) { (void)e; }
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("@try/@catch is not supported"), "{}", diags);
}

/* ------------------------------------------------------ #563 delegations */

/// **Refusing, and this test used to assert the opposite (#582).**
///
/// It was `module_import_is_left_to_clang`, and it asserted the passthrough
/// as a feature -- `out.source_c.contains("@import Foundation;")` -- on the
/// reasoning that modules are a front-end concern, that Clang's dump
/// rejects them, and that `MUTATIONS.md` grades M68 `CLANG`, delegated
/// correctly.
///
/// That reasoning was sound wherever Clang runs, and Clang does refuse it:
/// `error: use of '@import' when modules are disabled`, checked against
/// this harness's own flags. **Clang does not always run.**
/// `lib::check_ast_present` returns early when `program.classes.is_empty()`,
/// so a source declaring no class needs no dump, and `--allow-missing-ast`
/// skips the requirement outright. In both cases `@import` rode the
/// passthrough into generated C and GCC met a stray `@` in a file the
/// author never wrote -- which is how `outputbar` found it.
///
/// So the delegation is withdrawn and the disposition is oz2c's, per #582's
/// rule that every node kind is lowered or refused and never merely
/// unnamed. M68's grade is oz2c's to answer now. The message is also the
/// better one to receive: it names `#import`, the form this backend
/// resolves, where Clang can only say that modules are off.
#[test]
fn module_import_is_refused() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@import Foundation;

@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	return 0;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("'@import' is not in the static subset"),
        "expected the located refusal:\n{}",
        diags
    );
    assert!(diags.contains("#import"), "the remedy is the header form:\n{}", diags);
}

/* ------------------------------------------------ #563 accepted keywords */

/// **Accepting.** The positive list is unchanged. `@42`/`@YES`/`@(expr)`
/// all parse as the same generic `at_expression` that `@defs` does, so a
/// check keyed on the node kind rather than on the callee identifier would
/// have refused every one of them.
#[test]
fn boxed_literals_still_accepted() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        oznumber_src(),
        "\
@interface P : OZObject
- (int)run;
- (int)five;
@end
@implementation P
- (int)five { return 5; }
- (int)run
{
	OZNumber *a = @42;
	OZNumber *b = @YES;
	OZNumber *c = @([self five]);
	return (a != nil) + (b != nil) + (c != nil);
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	printf(\"%d\\n\", [p run]);
	return 0;
}
"
    );
    assert_eq!(compile_and_run(&src, "boxed_literals_still_accepted"), "3\n");
}

/* --------------------------------------------------- #564 @class support */

/// `@class` is consumed and replaced by the C spelling of the same
/// statement -- a tag declaration, which is legal with nothing ever
/// defining the struct. That is exactly the property `@class` has.
#[test]
fn class_forward_declaration_supported() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@class Other;

@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	Other *o = nil;

	(void)o;
	return 65;
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("a forward declaration is not an operation to lower");
    /* Consumed, not forwarded -- the `stray '@' in program` is gone. */
    assert!(
        !out.source_c.contains("@class Other;"),
        "the '@class' line was copied into the generated C:\n{}",
        out.source_c
    );
    /* Paired with a presence check, so the absence above cannot pass
     * vacuously if the declaration stopped being emitted at all. */
    assert!(out.source_c.contains("struct Other;"), "source_c:\n{}", out.source_c);
    /* And the use site gets the tag, which is what makes the C valid. */
    assert!(out.source_c.contains("struct Other *o"), "source_c:\n{}", out.source_c);
}

/// `@class A, B;` is a *single* `class_declaration` carrying every name, so
/// the list needs one tag declaration per name and not one per statement.
#[test]
fn class_forward_declaration_list_supported() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@class Alpha, Beta;

@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	Alpha *a = nil;
	Beta *b = nil;

	return (a == nil) && (b == nil);
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("a list of forward declarations is still declarations");
    assert!(out.source_c.contains("struct Alpha;"), "source_c:\n{}", out.source_c);
    assert!(out.source_c.contains("struct Beta;"), "source_c:\n{}", out.source_c);
    assert!(out.source_c.contains("struct Alpha *a"), "source_c:\n{}", out.source_c);
    assert!(out.source_c.contains("struct Beta *b"), "source_c:\n{}", out.source_c);
}

/// **Accepting, and the boundary `spells_with_struct_tag` draws.**
///
/// A name that is *both* forward-declared and really declared is an
/// ordinary class -- the forward declaration adds nothing. It must keep
/// every synthesized member, which is what would break if the spelling
/// predicate had been folded into `is_class` in the other direction.
#[test]
fn a_forward_declaration_beside_a_real_interface_is_still_a_class() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@class P;

@interface P : OZObject
- (int)run;
@end
@implementation P
- (int)run
{
	return 42;
}
@end

#include <stdio.h>
int main(void) {
	P *p = [P alloc];
	printf(\"%d\\n\", [p run]);
	return 0;
}
"
    );
    /* It runs, which means the allocator, the slab and the dispatch slot
     * all still exist for a name the forward declaration also mentions. */
    assert_eq!(compile_and_run(&src, "fwd_beside_real_interface"), "42\n");
}
