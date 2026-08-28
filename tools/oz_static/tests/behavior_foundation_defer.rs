// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_defer.rs - OZ-092 Foundation work: OZDefer, ported
// from tests/behavior/cases/foundation/defer_basic.m.
//
// OZDefer itself is transplanted from the real `src/OZDefer.m` (see
// `common::OZDEFER_SRC`). Unlike the real oracle test (which only checks
// "no crash" on release, since the Python pipeline's ARC would otherwise
// make the block-firing timing hard to observe directly), this adds an
// explicit global flag the deferred block flips, so dealloc-time firing
// is actually asserted, not just implied by an absence of a crash.
//
// tests/behavior/cases/foundation/defer_block_ivar.m's second test
// (`test_block_ivar_callable_through_struct`) is a regression test for a
// Python-pipeline-specific bug (a block ivar's `^`-to-`*` C conversion
// once emitted invalid declarator syntax). That bug class can't recur in
// oz_static by construction -- ivar declarations are copied verbatim
// (see `common::OZDEFER_SRC`'s doc comment), so an ivar is only ever
// valid C because it was written that way in source, never because a
// declarator-rewrite pass got it right -- so it isn't ported.
//
// oz_static has no ARC (issue #189), so `DeferTest`'s own `-dealloc`
// explicitly releases its `_cleanup` ivar -- there's no automatic
// ivar-release to rely on the way the Python oracle's comment
// ("Release triggers dealloc -> releases _cleanup ivar") implies.

mod common;
use common::{compile_and_run, OZDEFER_SRC};

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
fn defer_basic_fires_block_with_owner_on_dealloc() {
    let src = format!(
        "{}{}\n\
static int g_fired = 0;
static void *g_fired_owner = 0;

@interface DeferTest : OZSRoot {{
	struct OZDefer *_cleanup;
	int _marker;
}}
- (instancetype)initWithCleanup;
- (int)marker;
- (void)dealloc;
@end

@implementation DeferTest
- (instancetype)initWithCleanup {{
	_marker = 99;
	_cleanup = [[OZDefer alloc] initWithOwner:self block:^(id owner) {{
		g_fired = 1;
		g_fired_owner = owner;
	}}];
	return self;
}}
- (int)marker {{
	return _marker;
}}
- (void)dealloc {{
	[_cleanup release];
}}
@end

#include <stdio.h>
int main(void) {{
	DeferTest *t = [DeferTest alloc];
	[t initWithCleanup];
	printf(\"marker=%d\\n\", [t marker]);
	printf(\"fired_before_release=%d\\n\", g_fired);
	[t release];
	printf(\"fired_after_release=%d\\n\", g_fired);
	printf(\"owner_was_self=%d\\n\", g_fired_owner == (void *)t);
	return 0;
}}
",
        PREAMBLE, OZDEFER_SRC
    );
    let stdout = compile_and_run(&src, "defer_basic_fires_block_with_owner_on_dealloc");
    assert_eq!(
        stdout,
        "marker=99\nfired_before_release=0\nfired_after_release=1\nowner_was_self=1\n"
    );
}

#[test]
fn defer_with_block_only_no_owner() {
    // defer's -initWithBlock: variant (no owner) -- the block still fires
    // on dealloc, with a nil owner.
    let src = format!(
        "{}{}\n\
static int g_fired = 0;
static void *g_owner_seen = (void *)1;

@interface OwnerlessTest : OZSRoot {{
	struct OZDefer *_cleanup;
}}
- (void)setup;
- (void)dealloc;
@end

@implementation OwnerlessTest
- (void)setup {{
	_cleanup = [[OZDefer alloc] initWithBlock:^(id owner) {{
		g_fired = 1;
		g_owner_seen = owner;
	}}];
}}
- (void)dealloc {{
	[_cleanup release];
}}
@end

#include <stdio.h>
int main(void) {{
	OwnerlessTest *t = [OwnerlessTest alloc];
	[t setup];
	[t release];
	printf(\"fired=%d\\n\", g_fired);
	printf(\"owner_is_null=%d\\n\", g_owner_seen == 0);
	return 0;
}}
",
        PREAMBLE, OZDEFER_SRC
    );
    let stdout = compile_and_run(&src, "defer_with_block_only_no_owner");
    assert_eq!(stdout, "fired=1\nowner_is_null=1\n");
}
