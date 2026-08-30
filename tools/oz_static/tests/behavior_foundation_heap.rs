// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_heap.rs - OZHeap, the store behind
// `allocWithHeap:`, transplanted from the real `src/OZHeap.m` (see
// `common::ozheap_src`).
//
// SCOPE, stated plainly: these tests prove oz_static *transpiles* OZHeap
// into C that compiles, links, and runs -- a value-typed ivar of an
// externally-declared struct (`struct oz_heap_inner _inner`), a
// `self = [super init]` chain, a `size_t` return type, and
// `&self->_inner` address-of-ivar passing. They do NOT prove heap
// accounting: this harness compiles with `-DOZ_PLATFORM_HOST` but not
// `-DOZ_HEAP_SUPPORT`, so the linked `oz_heap_init`/`oz_heap_used_bytes`
// are the header's own no-op stubs, and `-usedBytes` answers 0 by
// construction. Real accounting needs the malloc-backed PAL versions
// (`platform/oz_platform_host.h`, behind `OZ_HEAP_SUPPORT`) reached
// through `allocWithHeap:`, which oz_static does not implement yet.
//
// The oracle's `tests/behavior/cases/memory/heap_alloc.m` is the
// corresponding case; it is NOT ported, because it exercises
// `[Cls allocWithHeap:]` -- the part oz_static lacks. Porting it would
// require asserting on behavior that doesn't exist here.

mod common;
use common::{compile_and_run, ozheap_src, ozobject_src as PREAMBLE};

#[test]
fn heap_init_with_buffer_and_used_bytes() {
    let src = format!(
        "{}{}\n\
#include <stdio.h>

static char g_buf[256];

int main(void) {{
	OZHeap *h = [OZHeap alloc];
	OZHeap *ret = [h initWithBuffer:g_buf size:256];
	printf(\"init_returned_self=%d\\n\", ret == h);
	printf(\"used=%zu\\n\", [h usedBytes]);
	printf(\"rc=%d\\n\", [h retainCount]);
	[h release];
	printf(\"released_ok\\n\");
	return 0;
}}
",
        PREAMBLE(),
        ozheap_src()
    );
    let stdout = compile_and_run(&src, "heap_init_with_buffer_and_used_bytes");
    // used=0 is the stub's answer, not a measurement -- see the module
    // comment.
    assert_eq!(stdout, "init_returned_self=1\nused=0\nrc=1\nreleased_ok\n");
}

/// OZHeap as a strong ivar of another class, released from that class's
/// own `-dealloc`: oz_static has no ARC (#189), so the release is
/// explicit.
#[test]
fn heap_held_as_ivar_and_released_by_owner() {
    let src = format!(
        "{}{}\n\
#include <stdio.h>

static char g_buf2[128];
static int g_owner_dealloc_ran = 0;

/* `struct OZHeap *`, not `OZHeap *`: oz_static copies an ivar
 * declaration's type text through as written (only ARC qualifiers and
 * block declarators are lowered), so a bare class name in ivar position
 * stays a bare class name and C rejects it. Same spelling the existing
 * OZDefer test uses for its `struct OZDefer *_cleanup` ivar. */
@interface Pool : OZObject {{
	struct OZHeap *_heap;
}}
- (instancetype)setup;
- (size_t)heapUsed;
- (void)dealloc;
@end

@implementation Pool
- (instancetype)setup {{
	_heap = [[OZHeap alloc] initWithBuffer:g_buf2 size:128];
	return self;
}}
- (size_t)heapUsed {{
	return [_heap usedBytes];
}}
- (void)dealloc {{
	g_owner_dealloc_ran = 1;
	/* _heap is released automatically on dealloc. */
}}
@end

int main(void) {{
	Pool *p = [Pool alloc];
	[p setup];
	printf(\"used=%zu\\n\", [p heapUsed]);
	printf(\"dealloc_before=%d\\n\", g_owner_dealloc_ran);
	[p release];
	printf(\"dealloc_after=%d\\n\", g_owner_dealloc_ran);
	return 0;
}}
",
        PREAMBLE(),
        ozheap_src()
    );
    let stdout = compile_and_run(&src, "heap_held_as_ivar_and_released_by_owner");
    assert_eq!(stdout, "used=0\ndealloc_before=0\ndealloc_after=1\n");
}
