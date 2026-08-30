// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_defer.rs - OZ-092 Foundation work: OZDefer, ported
// from tests/behavior/cases/foundation/defer_basic.m.
//
// OZDefer itself is transplanted from the real `src/OZDefer.m` (see
// `common::ozdefer_src`). Unlike the real oracle test (which only checks
// "no crash" on release, since the Python pipeline's ARC would otherwise
// make the block-firing timing hard to observe directly), this adds an
// explicit global flag the deferred block flips, so dealloc-time firing
// is actually asserted, not just implied by an absence of a crash.
//
// tests/behavior/cases/foundation/defer_block_ivar.m (+ its
// `defer_block_ivar_test.c` driver) is a regression test for a bug where
// a block ivar's `^`-to-`*` C conversion emitted invalid declarator
// syntax (`void (*)(...) _block` instead of `void (*_block)(...)`). That
// bug class used not to apply to oz_static, which copied ivar
// declarations through verbatim; `emit::lower_ivar_decl` now does the
// conversion, so the test is ported below as
// `block_ivar_declares_valid_function_pointer`.
//
// `DeferTest`'s `_cleanup` ivar is released automatically when the owner is
// deallocated (`companion::render_release_ivars`), so its `-dealloc` must
// NOT release it by hand -- doing so is rejected, because the two releases
// together would be a double free. This is where oz_static deliberately
// parts company with the oracle, whose `_emit_user_dealloc` appends the
// automatic releases after the user's body and so double-releases silently.

mod common;
use common::{compile_and_run, compile_and_run_strict, ozdefer_src, ozobject_src as PREAMBLE};

#[test]
fn defer_basic_fires_block_with_owner_on_dealloc() {
    let src = format!(
        "{}{}\n\
static int g_fired = 0;
static void *g_fired_owner = 0;

@interface DeferTest : OZObject {{
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
	/* _cleanup is released automatically -- see
	 * companion::render_release_ivars. Releasing it here is rejected. */
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
        PREAMBLE(), ozdefer_src()
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

@interface OwnerlessTest : OZObject {{
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
	/* _cleanup is released automatically -- see
	 * companion::render_release_ivars. Releasing it here is rejected. */
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
        PREAMBLE(), ozdefer_src()
    );
    let stdout = compile_and_run(&src, "defer_with_block_only_no_owner");
    assert_eq!(stdout, "fired=1\nowner_is_null=1\n");
}

/// Ported from tests/behavior/cases/foundation/defer_block_ivar.m and its
/// `defer_block_ivar_test.c` driver: reaches OZDefer's block ivar
/// directly through the generated struct, assigning a plain C function to
/// it and calling it. This only compiles if `emit::lower_ivar_decl`
/// turned the real header's `void (^_block)(id);` into a well-formed
/// function-pointer field -- a regressed declarator (`void (*)(id)
/// _block`) is a syntax error, and a field of the wrong pointer type
/// trips `-Werror=incompatible-pointer-types` via `compile_and_run_strict`.
/// The real header also spells `_owner` `__unsafe_unretained`, so this
/// covers the ARC-qualifier strip reaching the struct as well.
#[test]
fn block_ivar_declares_valid_function_pointer() {
    let src = format!(
        "{}{}\n\
static int g_block_called = 0;

static void test_block_fn(id owner) {{
	(void)owner;
	g_block_called = 1;
}}

#include <stdio.h>
int main(void) {{
	OZDefer *d = [OZDefer alloc];
	d->_block = test_block_fn;
	d->_owner = 0;
	d->_block(d->_owner);
	printf(\"called=%d\\n\", g_block_called);
	printf(\"owner_field_is_null=%d\\n\", d->_owner == 0);
	[d release];
	return 0;
}}
",
        PREAMBLE(),
        ozdefer_src()
    );
    let stdout = compile_and_run_strict(&src, "block_ivar_declares_valid_function_pointer");
    assert_eq!(stdout, "called=1\nowner_field_is_null=1\n");
}
