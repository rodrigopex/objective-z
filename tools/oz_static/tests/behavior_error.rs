// SPDX-License-Identifier: Apache-2.0
//
// behavior_error.rs - OZ-092 (#190): port of the Python pipeline's "error"
// category behavior fixtures (tests/behavior/cases/error/) to oz_static.
//
// Ported from:
//   - tests/behavior/cases/error/release_nil_safe.m
//   - tests/behavior/cases/error/slab_reuse_after_free.m
//
// Uses the real `OZObject` (`common::OZOBJECT_SRC`) as the root class.

mod common;
use common::{compile_and_run, OZOBJECT_SRC};

#[test]
fn release_and_retain_nil_are_safe() {
    // Ported from release_nil_safe.m / release_nil_safe_test.c:
    //   - test_release_nil_no_crash: releasing nil must not crash.
    //   - test_retain_nil_returns_null: retaining nil must return null.
    //
    // Skipped: test_retain_count_nil_is_zero (OZObject_retainCount(nil) ==
    // 0) has no oz_static equivalent -- oz_static exposes no public
    // retainCount accessor at all; the refcount is an internal field
    // touched only by retain/release, never surfaced as a selector or
    // function. See OZ-092 (#190).
    let src = format!(
        "{}{}",
        OZOBJECT_SRC,
        "\
@interface Marker : OZObject
@end
@implementation Marker
@end

#include <stdio.h>

int main(void) {
	Marker *m = 0;
	[m release];
	Marker *r = [m retain];
	printf(\"release_nil_ok=1\\n\");
	printf(\"retain_nil_is_null=%d\\n\", r == 0 ? 1 : 0);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "release_and_retain_nil_are_safe");
    assert_eq!(stdout, "release_nil_ok=1\nretain_nil_is_null=1\n");
}

#[test]
fn alloc_free_alloc_yields_independent_fresh_object() {
    // Ported from slab_reuse_after_free.m / slab_reuse_after_free_test.c:
    //   - test_slab_reuse_after_release: after alloc/set/release, a new
    //     alloc must succeed and be independently usable and correctly
    //     (freshly) initialized -- not carrying over the prior object's
    //     data.
    //
    // Skipped: test_slab_exhaustion_returns_null (a 2-slot pool's third
    // alloc returns null) has no oz_static equivalent -- that test is
    // exercising Python's slab-pool mechanics specifically. oz_static's
    // `{Class}_oz_alloc` is malloc-based with no fixed capacity (see
    // companion.rs's render_alloc_free doc comment), so there is no
    // bounded pool to exhaust. See OZ-092 (#190).
    let src = format!(
        "{}{}",
        OZOBJECT_SRC,
        "\
@interface Gadget : OZObject {
	int _tag;
}
- (int)tag;
- (void)setTag:(int)tag;
@end

@implementation Gadget
- (int)tag {
	return _tag;
}
- (void)setTag:(int)tag {
	_tag = tag;
}
@end

#include <stdio.h>

int main(void) {
	Gadget *g1 = [Gadget alloc];
	[g1 setTag:99];
	printf(\"tag1=%d\\n\", [g1 tag]);
	[g1 release];

	Gadget *g2 = [Gadget alloc];
	printf(\"g2_not_null=%d\\n\", g2 != 0 ? 1 : 0);
	printf(\"tag2_default=%d\\n\", [g2 tag]);
	[g2 release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "alloc_free_alloc_yields_independent_fresh_object");
    assert_eq!(stdout, "tag1=99\ng2_not_null=1\ntag2_default=0\n");
}
