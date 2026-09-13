// SPDX-License-Identifier: Apache-2.0
//
// behavior_error.rs - OZ-092 (#190): port of the Python pipeline's "error"
// category behavior fixtures (tests/behavior/cases/error/) to oz_static.
//
// Ported from:
//   - tests/behavior/cases/error/release_nil_safe.m
//   - tests/behavior/cases/error/slab_reuse_after_free.m
//
// Uses the real `OZObject` (`common::ozobject_src`) as the root class.

mod common;
use common::{compile_and_run, ozobject_src};

#[test]
fn release_and_retain_nil_are_safe() {
    // Ported from release_nil_safe.m / release_nil_safe_test.c:
    //   - test_release_nil_no_crash: releasing nil must not crash.
    //   - test_retain_nil_returns_null: retaining nil must return null.
    //   - test_retain_count_nil_is_zero: the refcount of nil is 0. This
    //     used to be skipped as having "no oz_static equivalent -- no
    //     public retainCount accessor at all", which #418 made untrue:
    //     `oz_static_retain_count` is the single entry point for reading
    //     one, and `-retainCount` lowers to it.
    //
    // The two sends this used to be written with -- `[m release]` and
    // `[m retain]` -- are located errors now that ARC is unconditional
    // (#428), so the case drives the generated runtime's own functions
    // instead. That is the same code the sends lowered to, and it is what
    // ARC itself emits; nil-safety is a property of those functions, not
    // of a source spelling, so this is where it belongs. A C call is not
    // the manual ownership model coming back -- ARC has no opinion about
    // one, and `main` here is plain C.
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Marker : OZObject
@end
@implementation Marker
@end

#include <stdio.h>

int main(void) {
	Marker *m = 0;
	oz_static_release((struct OZObject *)m);
	Marker *r = (Marker *)oz_static_retain((struct OZObject *)m);
	printf(\"release_nil_ok=1\\n\");
	printf(\"retain_nil_is_null=%d\\n\", r == 0 ? 1 : 0);
	printf(\"retain_count_nil=%d\\n\", oz_static_retain_count((struct OZObject *)m));
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "release_and_retain_nil_are_safe");
    assert_eq!(stdout, "release_nil_ok=1\nretain_nil_is_null=1\nretain_count_nil=0\n");
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
        ozobject_src(),
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
	/* One braced scope each, so the first Gadget is provably destroyed
	 * before the second is allocated -- the precondition this case is
	 * about, which a hand release used to establish (#428). Scope exit
	 * is where ARC releases an owned local. */
	{
		Gadget *g1 = [Gadget alloc];
		[g1 setTag:99];
		printf(\"tag1=%d\\n\", [g1 tag]);
	}
	{
		Gadget *g2 = [Gadget alloc];
		printf(\"g2_not_null=%d\\n\", g2 != 0 ? 1 : 0);
		printf(\"tag2_default=%d\\n\", [g2 tag]);
	}
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "alloc_free_alloc_yields_independent_fresh_object");
    assert_eq!(stdout, "tag1=99\ng2_not_null=1\ntag2_default=0\n");
}
