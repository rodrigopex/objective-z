// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_heap.rs - OZHeap, the store behind
// `allocWithHeap:`, transplanted from the real `src/OZHeap.m` (see
// `common::ozheap_src`).
//
// The first two tests prove oz_static *transpiles* OZHeap into C that
// compiles, links, and runs -- a value-typed ivar of an externally-declared
// struct (`struct oz_heap_inner _inner`), a `self = [super init]` chain, a
// `size_t` return type, and `&self->_inner` address-of-ivar passing. They do
// not prove heap accounting: they compile with `-DOZ_PLATFORM_HOST` but not
// `-DOZ_HEAP_SUPPORT`, so the linked `oz_heap_init`/`oz_heap_used_bytes` are
// the header's own no-op stubs and `-usedBytes` answers 0 by construction.
//
// The last test does prove accounting, through the real malloc-backed PAL
// versions (`platform/oz_platform_host.h`, behind `OZ_HEAP_SUPPORT`) reached
// via `+allocWithHeap:`. It is the shape of the oracle's own
// `tests/behavior/cases/memory/heap_alloc.m`, which cannot itself be run
// through the cross-backend harness: that case's driver asserts on the
// oracle's root struct layout (`w->base._meta.class_id`) rather than on
// behavior -- see docs/STATUS.md.

mod common;
use common::{
    compile_and_run, compile_and_run_with_heap, ozheap_src, ozobject_src as PREAMBLE,
};

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
/// own `-dealloc`. The release is written by hand here, which ARC defers to
/// (see `emit::released_by_hand`).
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

/// `+allocWithHeap:` end to end: the storage comes from the heap it was
/// given, the heap's used-bytes reflects that, and freeing gives the space
/// back -- so `-usedBytes` is 0 again once the object dies.
///
/// This is the check that matters for the whole feature, because none of it
/// is visible earlier. Compiling and linking both passed while every object
/// allocated this way leaked: `@autoreleasepool` had its own renderer that
/// skipped ARC entirely, so nothing released them (see `emit::arc_enter`).
/// The heap's own accounting is what makes that observable at all.
#[test]
fn alloc_with_heap_takes_storage_from_the_heap_and_gives_it_back() {
    let src = format!(
        "{}{}\n\
#include <stdio.h>

@interface Widget : OZObject {{
\tint _tag;
}}
- (void)setTag:(int)t;
- (int)tag;
@end

@implementation Widget
- (void)setTag:(int)t {{
\t_tag = t;
}}
- (int)tag {{
\treturn _tag;
}}
- (void)dealloc {{
}}
@end

static char g_buf[1024];

int main(void) {{
\tOZHeap *h = [[OZHeap alloc] initWithBuffer:g_buf size:1024];
\tprintf(\"before=%zu\\n\", [h usedBytes]);
\t@autoreleasepool {{
\t\tWidget *w = [[Widget allocWithHeap:h] init];
\t\t[w setTag:7];
\t\tprintf(\"tag=%d\\n\", [w tag]);
\t\tprintf(\"during=%d\\n\", [h usedBytes] > 0);
\t}}
\tprintf(\"after=%zu\\n\", [h usedBytes]);
\t[h release];
\treturn 0;
}}
",
        PREAMBLE(),
        ozheap_src()
    );
    let stdout = compile_and_run_with_heap(
        &src,
        "alloc_with_heap_takes_storage_from_the_heap_and_gives_it_back",
    );
    assert_eq!(stdout, "before=0\ntag=7\nduring=1\nafter=0\n");
}
