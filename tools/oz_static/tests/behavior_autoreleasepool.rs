// SPDX-License-Identifier: Apache-2.0
//
// behavior_autoreleasepool.rs - @autoreleasepool support.
//
// Not a port -- the oracle's tests/behavior/cases/ has no autoreleasepool
// category at all, so there is nothing to port; this is new coverage.
//
// @autoreleasepool { body } unwraps to a plain compound statement, no
// pool object, no drain -- matching the Python pipeline exactly
// (emit.py accepts it syntactically and unwraps it the same way; there
// is no OZAutoreleasePool class or -autorelease method anywhere in this
// SDK). See emit::render_autoreleasepool_statement for the grammar note
// on why this needs its own detection: tree-sitter-objc gives
// `@autoreleasepool { ... }` no node kind of its own -- it parses as an
// ordinary compound_statement whose first child is the literal token
// `@autoreleasepool`, ahead of the usual `{`.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// The `@autoreleasepool` token itself must not survive into the
/// generated C (it isn't a real C token), and the body's statements --
/// including its `return` -- must behave exactly as if the block markers
/// weren't there.
#[test]
fn autoreleasepool_unwraps_to_plain_block() {
    let src = format!(
        "{}
@interface Foo : OZObject
- (int)compute;
@end

@implementation Foo
- (int)compute {{
	@autoreleasepool {{
		int x = 1;
		int y = 2;
		return x + y;
	}}
}}
@end

#include <stdio.h>

int main(void) {{
	Foo *f = [Foo alloc];
	printf(\"compute=%d\\n\", [f compute]);
	[f release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "autoreleasepool_unwraps_to_plain_block");
    assert_eq!(stdout, "compute=3\n");
}

/// A message send inside the pool block (allocating, using, and
/// releasing a local object) must translate exactly as it would outside
/// one -- the pool boundary carries no retain/release semantics of its
/// own to interact with.
#[test]
fn autoreleasepool_with_message_sends_inside() {
    let src = format!(
        "{}
@interface Bar : OZObject {{
	int _value;
}}
- (void)setValue:(int)v;
- (int)value;
@end

@implementation Bar
- (void)setValue:(int)v {{
	_value = v;
}}
- (int)value {{
	return _value;
}}
@end

@interface Baz : OZObject
- (int)run;
@end

@implementation Baz
- (int)run {{
	int total = 0;
	@autoreleasepool {{
		Bar *b = [Bar alloc];
		[b setValue:5];
		total = [b value] * 2;
		[b release];
	}}
	return total;
}}
@end

#include <stdio.h>

int main(void) {{
	Baz *z = [Baz alloc];
	printf(\"run=%d\\n\", [z run]);
	[z release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "autoreleasepool_with_message_sends_inside");
    assert_eq!(stdout, "run=10\n");
}

/// Two sibling @autoreleasepool blocks in the same method body, each
/// with its own local of the same name -- proof the unwrap doesn't merge
/// scopes or otherwise break C block scoping.
#[test]
fn sibling_autoreleasepool_blocks_each_scope_independently() {
    let src = format!(
        "{}
@interface Seq : OZObject {{
	int _total;
}}
- (void)run;
- (int)total;
@end

@implementation Seq
- (void)run {{
	@autoreleasepool {{
		int n = 1;
		_total = _total + n;
	}}
	@autoreleasepool {{
		int n = 2;
		_total = _total + n;
	}}
}}
- (int)total {{
	return _total;
}}
@end

#include <stdio.h>

int main(void) {{
	Seq *s = [Seq alloc];
	[s run];
	printf(\"total=%d\\n\", [s total]);
	[s release];
	return 0;
}}
",
        PREAMBLE()
    );
    let stdout = compile_and_run(&src, "sibling_autoreleasepool_blocks_each_scope_independently");
    assert_eq!(stdout, "total=3\n");
}

/// A pool block is an ordinary scope as far as ownership goes, so ARC has to
/// release what is declared inside it when it ends.
///
/// It did not. `@autoreleasepool` has its own arm in `emit::render_expr`'s
/// match, sitting *before* the ARC one, so a pool block that declared an
/// owned local got the pool renderer and never the releases -- every object
/// allocated inside one leaked silently. `samples/heap_alloc` is built
/// entirely out of that shape, and states the consequence in its own
/// expected output ("Sensor dealloc", "app heap after free: 0 bytes used"):
/// nothing was released, no `-dealloc` ran, and the heap never came back
/// down. Neither compiling nor linking can see that; only running it can,
/// which is why this test observes `-dealloc` rather than inspecting the C.
#[test]
fn autoreleasepool_releases_what_it_owns_at_scope_exit() {
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
	@autoreleasepool {{
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
    let stdout = compile_and_run(&src, "autoreleasepool_releases_what_it_owns_at_scope_exit");
    // Reverse declaration order, which is what Clang's own ARC does (its
    // scope cleanups run LIFO, like C++ destructors) and what matters when
    // one object's -dealloc touches another. The oracle releases in
    // declaration order instead -- see docs/STATUS.md.
    assert_eq!(stdout, "before\ndealloc 2\ndealloc 1\nafter\n");
}
