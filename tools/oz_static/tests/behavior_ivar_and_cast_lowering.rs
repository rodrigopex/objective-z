// SPDX-License-Identifier: Apache-2.0
//
// behavior_ivar_and_cast_lowering.rs - two ObjC-only spellings that used
// to reach the C compiler untranslated, both found while porting OZHeap
// and the since-retired OZTimer (whose real sources used them).
//
//   - a bare class name as an ivar type (`OZHeap *_heap;`). The generated
//     struct for a class is `struct Name`, never a typedef, so the
//     untagged spelling was `error: must use 'struct' tag`. Every other
//     type position already routed through `collect::render_type`; an ivar
//     declaration is copied through as text, so
//     `emit::lower_ivar_decl` now tags it.
//   - an ARC bridging cast (`(__bridge void *)x`). `__bridge` is not a C
//     keyword, so it was `error: use of undeclared identifier
//     '__bridge'` -- meaning the real `src/OZTimer.m` transpiled to C
//     that could not compile. `emit::render_cast_expression` drops it, as
//     the oracle did in its own output for the same file. Both that source
//     and that generated file are gone since #267 retired OZTimer, so the
//     lowering is pinned by this test's own fixtures rather than by any
//     file in the tree; `__bridge` remains valid Objective-C that someone
//     may write.
//
// Handling the cast node also let it report its target type, so a message
// send against a cast receiver resolves instead of failing with "cannot
// statically resolve the receiver type ... (receiver type is 'id')" --
// covered by `send_through_cast_resolves_receiver` below and relied on by
// `behavior_foundation_array`.

mod common;
use common::{compile_and_run_strict, ozobject_src, ozq31_src};

#[test]
fn bare_class_name_ivar_gains_struct_tag() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Inner : OZObject {
	int _v;
}
- (void)setV:(int)v;
- (int)v;
@end
@implementation Inner
- (void)setV:(int)v {
	_v = v;
}
- (int)v {
	return _v;
}
@end

@interface Outer : OZObject {
	Inner *_bare;
	struct Inner *_tagged;
}
- (void)build;
- (int)bareV;
- (int)taggedV;
- (void)dealloc;
@end
@implementation Outer
- (void)build {
	_bare = [Inner alloc];
	[_bare setV:11];
	_tagged = [Inner alloc];
	[_tagged setV:22];
}
- (int)bareV {
	return [_bare v];
}
- (int)taggedV {
	return [_tagged v];
}
- (void)dealloc {
	/* Both ivars are released automatically on dealloc. */
}
@end

#include <stdio.h>
int main(void) {
	Outer *o = [Outer alloc];
	[o build];
	printf(\"bare=%d\\n\", [o bareV]);
	printf(\"tagged=%d\\n\", [o taggedV]);
	[o release];
	return 0;
}
"
    );
    let stdout = compile_and_run_strict(&src, "bare_class_name_ivar_gains_struct_tag");
    assert_eq!(stdout, "bare=11\ntagged=22\n");
}

#[test]
fn bridge_cast_qualifier_is_dropped() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Bridger : OZObject
- (void *)erase:(void (^)(void))blk;
@end
@implementation Bridger
- (void *)erase:(void (^)(void))blk {
	return (__bridge void *)blk;
}
@end

static int g_ran = 0;
static void marker(void) {
	g_ran = 1;
}

#include <stdio.h>
int main(void) {
	Bridger *b = [Bridger alloc];
	void *erased = [b erase:^(void) { }];
	printf(\"erased_non_null=%d\\n\", erased != 0);
	marker();
	printf(\"ran=%d\\n\", g_ran);
	[b release];
	return 0;
}
"
    );
    let stdout = compile_and_run_strict(&src, "bridge_cast_qualifier_is_dropped");
    assert_eq!(stdout, "erased_non_null=1\nran=1\n");
}

/// A cast is the one place a bare `id` can be narrowed back to a class
/// without inference, so the cast's declared type has to be reported for
/// the send to resolve.
#[test]
fn send_through_cast_resolves_receiver() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        ozq31_src(),
        "\
#include <stdio.h>
int main(void) {
	id boxed = [OZQ31 fixedWithInt32:42];
	printf(\"val=%d\\n\", [((OZQ31 *)boxed) int32Value]);
	[boxed release];
	return 0;
}
"
    );
    let stdout = compile_and_run_strict(&src, "send_through_cast_resolves_receiver");
    assert_eq!(stdout, "val=42\n");
}
