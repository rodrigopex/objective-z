// SPDX-License-Identifier: Apache-2.0
//
// behavior_autoreleasepool.rs - `@autoreleasepool` is refused (#430).
//
// **This file asserted the opposite until #430, and the reversal is the
// record worth keeping.** It had four tests proving the construct worked:
// the token did not survive into the generated C, sends inside it
// translated as they would outside, sibling blocks scoped independently,
// and ARC released what the block owned at its closing brace. All four
// passed, and the behaviour really was correct -- `emit` dropped the token
// and ran the same `arc_enter`/`arc_exit` bookkeeping as
// `render_scoped_block`, so the block was an ordinary ARC scope.
//
// Correct behaviour was never the question. The keyword promises a
// mechanism that does not exist here and cannot:
//
//   - **There is no `-autorelease`.** It is in no SDK header, and a send of
//     it is one of the five hard located errors ARC owns
//     (`ARC_FORBIDDEN_SELECTORS`, #428/#436). With no way to make a
//     reference *pending*, a drain has nothing to drain.
//   - **There is no pool object.** `OZAutoreleasePool` exists only under
//     `src/runtime_legacy/`, which no CMake file references.
//
// So the construct was accepted for its syntax alone, and a reader porting
// Cocoa code got immediate reclaim at scope exit where the keyword promises
// deferred reclaim at a drain point. In Cocoa those differ observably -- an
// object handed back by a factory outlives the statement that made it --
// and here the difference is unreachable, because the mechanism that
// creates it is refused. Accepting a keyword whose meaning is a mechanism
// the backend does not have is precisely what the never-silently-degrade
// rule forbids, and "it happens to behave correctly" is the argument that
// kept `__objc_refcount_get` public for a year (#418).
//
// The grammar note that used to live here still matters to the checker:
// tree-sitter-objc gives `@autoreleasepool { ... }` no node kind of its own
// -- it parses as an ordinary `compound_statement` whose first child is the
// literal token `@autoreleasepool`, ahead of the usual `{`. That test moved
// from `emit::is_autoreleasepool_shape`, now deleted, to
// `staticbar::check_autoreleasepool`.
//
// What the old tests covered is not lost. The pool block compiled to
// exactly the plain braced scope that replaces it, so the shapes moved
// rather than went: `selector_ownership_matrix::the_nested_scope_construct`
// carries the two ownership cells, `behavior_foundation_heap`'s fixtures
// carry the heap ones, and the second test below is the leak regression
// this file existed for, respelled.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// The construct is refused, and the diagnostic names the replacement.
///
/// Asserting the *remedy* and not merely the refusal: the migration is
/// deleting one token, and a diagnostic that does not say so leaves a
/// reader guessing at a mechanism that is not there.
#[test]
fn autoreleasepool_is_rejected_and_the_diagnostic_names_the_replacement() {
    let src = format!(
        "{}
int main(void) {{
	@autoreleasepool {{
		int x = 1;
		(void)x;
	}}
	return 0;
}}
",
        PREAMBLE()
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("@autoreleasepool") && diags.contains("no meaning"),
        "the rejection must name the construct; got:\n{}",
        diags
    );
    assert!(
        diags.contains("-autorelease") && diags.contains("no pool object"),
        "and say why it can never mean more -- nothing can be pending, nothing to drain; \
         got:\n{}",
        diags
    );
    assert!(
        diags.contains("Delete the keyword and keep the braces"),
        "and name the one-token fix, which is what makes this migration mechanical; got:\n{}",
        diags
    );
}

/// The replacement behaves exactly as the pool block did -- which is what
/// makes deleting the token a safe migration rather than a hopeful one.
///
/// This is the old `autoreleasepool_releases_what_it_owns_at_scope_exit`
/// with the keyword removed and nothing else changed, and it still asserts
/// the same output. That test existed because the pool block *did* leak
/// once: `@autoreleasepool` had its own arm in `emit::render_expr`'s match,
/// sitting before the ARC one, so a pool block declaring an owned local got
/// the pool renderer and never the releases. `samples/heap_alloc` was built
/// entirely of that shape and stated the consequence in its own expected
/// output; neither compiling nor linking could see it, which is why this
/// observes `-dealloc` rather than inspecting the C.
///
/// Reverse declaration order, which is what Clang's own ARC does -- its
/// scope cleanups run LIFO, like C++ destructors -- and what matters when
/// one object's `-dealloc` touches another.
#[test]
fn a_plain_braced_scope_releases_what_it_owns_at_its_closing_brace() {
    let src = format!(
        "{}
#include <stdio.h>

@interface Tracked : OZObject {{
	int _tag;
}}
- (void)setTag:(int)t;
@end

@implementation Tracked
- (void)setTag:(int)t {{
	_tag = t;
}}
- (void)dealloc {{
	printf(\"dealloc %d\\n\", _tag);
}}
@end

int main(void) {{
	printf(\"before\\n\");
	{{
		Tracked *first = [Tracked alloc];
		[first setTag:1];
		Tracked *second = [Tracked alloc];
		[second setTag:2];
	}}
	printf(\"after\\n\");
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "plain_scope_releases_what_it_owns");
    assert_eq!(stdout, "before\ndealloc 2\ndealloc 1\nafter\n");
}

/// A bare nested block is still an ordinary block.
///
/// The checker keys on a `compound_statement` whose *first child* is the
/// `@autoreleasepool` token, so nothing about an ordinary `{ ... }` should
/// change -- but a rejection that over-reaches to every nested block would
/// break most of the corpus, so it is worth one test of its own rather than
/// left to be noticed by the sweep.
#[test]
fn an_ordinary_nested_block_is_not_mistaken_for_a_pool() {
    let src = format!(
        "{}
#include <stdio.h>
int main(void) {{
	int total = 0;
	{{
		int inner = 2;
		total = total + inner;
	}}
	{{
		int inner = 3;
		total = total + inner;
	}}
	printf(\"total=%d\\n\", total);
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "ordinary_nested_block_accepted");
    assert_eq!(stdout, "total=5\n", "sibling blocks each scope independently, as in C");
}
