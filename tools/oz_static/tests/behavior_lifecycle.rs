// SPDX-License-Identifier: Apache-2.0
//
// behavior_lifecycle.rs - OZ-092: lifecycle-category parity port from the
// Python pipeline's tests/behavior/cases/lifecycle/ fixtures. Each fixture
// there is an `X.m` (class declarations only) + hand-written `X_test.c`
// (Unity assertions against the Python-generated API). Ported here as one
// inlined source per test, with a `main()` that printf's the values the
// original Unity assertions checked, and an exact-stdout assertion in the
// Rust test (see tests/end_to_end_behavior.rs for the established pattern).
//
// Uses the real `OZObject` (`common::OZOBJECT_SRC`) as the root class.

mod common;
use common::{compile_and_run, OZOBJECT_SRC};

// tests/behavior/cases/lifecycle/alloc_failure_enomem.{m,_test.c} is not
// ported here: it declares a pool of size 1 (`/* oz-pool: Box=1 */`) and
// asserts that the *second* alloc against an exhausted slab returns NULL.
// oz_static's `{Class}_oz_alloc` is plain malloc-based, not slab-backed
// (see companion.rs's render_alloc_free doc comment and OZ-092/#190's own
// note on this), so there is no fixed-size pool to exhaust on demand.
// The behavior this fixture is really guarding -- alloc returning NULL
// safely on allocation failure rather than crashing -- does have a
// generated equivalent (`if (!obj) { return (struct {name} *)0; }` in
// every `{Class}_oz_alloc`), but there's no portable way to force malloc
// itself to fail from a plain host test in this harness (no allocator
// mocking hook), so exercising that path at runtime isn't practical here.
// Skipped, not silently dropped -- tracked under OZ-092.

/// Ported from tests/behavior/cases/lifecycle/alloc_returns_valid.{m,_test.c}.
/// Original asserted: alloc returns non-null, sets the class id
/// (`w->base._meta.class_id` in Python's OZObject layout), and sets the
/// refcount to 1. oz_static's root stores these fields directly
/// (`oz_class_id`, `oz_refcount`) rather than nested under a `_meta`
/// struct; a non-root class reaches them through the `base` embedding hop.
#[test]
fn alloc_returns_valid_pointer_class_id_and_refcount() {
    let src = format!(
        "{}{}",
        OZOBJECT_SRC,
        "\
@interface Widget : OZObject {
	int _tag;
}
- (void)setTag:(int)t;
- (int)tag;
@end

@implementation Widget
- (void)setTag:(int)t {
	_tag = t;
}
- (int)tag {
	return _tag;
}
@end

#include <stdio.h>

int main(void) {
	Widget *w = [Widget alloc];
	printf(\"nonnull=%d\\n\", w != 0);
	printf(\"class_id_matches=%d\\n\", w->base.oz_class_id == OZ_STATIC_CLASS_Widget);
	printf(\"refcount=%d\\n\", oz_atomic_get(&w->base.oz_refcount));
	[w release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "alloc_returns_valid_pointer_class_id_and_refcount");
    assert_eq!(stdout, "nonnull=1\nclass_id_matches=1\nrefcount=1\n");
}

/// Ported from tests/behavior/cases/lifecycle/dealloc_frees_slab.{m,_test.c}.
/// Original name/setup ("oz-pool: Slot=1") is specific to Python's
/// fixed-size slab pool: allocate the pool's one block, release it, and
/// prove the block is returned to the pool by allocating again. oz_static
/// has no slab -- `{Class}_oz_alloc`/`_oz_free` are plain malloc/free (see
/// companion.rs) -- so there is no pool to exhaust or return a block to.
/// The equivalent guarantee that *does* carry over: `-dealloc`/`release`
/// actually runs and frees the storage such that allocation continues to
/// work correctly afterward (no leak/corruption from the free path).
#[test]
fn release_then_realloc_succeeds() {
    let src = format!(
        "{}{}",
        OZOBJECT_SRC,
        "\
@interface Slot : OZObject
@end
@implementation Slot
@end

#include <stdio.h>

int main(void) {
	Slot *s1 = [Slot alloc];
	printf(\"first_nonnull=%d\\n\", s1 != 0);
	[s1 release];

	Slot *s2 = [Slot alloc];
	printf(\"second_nonnull=%d\\n\", s2 != 0);
	[s2 release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "release_then_realloc_succeeds");
    assert_eq!(stdout, "first_nonnull=1\nsecond_nonnull=1\n");
}

/// Ported from tests/behavior/cases/lifecycle/dealloc_reentrant_guard.{m,_test.c}.
/// Classic ObjC pattern: `-dealloc` retains+releases self. Without a
/// re-entrancy guard, the nested release (rc 1->0 again) would trigger
/// `-dealloc` a second time -> infinite recursion / stack overflow.
/// oz_static's companion.rs generates this guard directly on the root's
/// `oz_deallocating` field (set before the dispatch switch runs, checked
/// by `oz_static_release` before it would recurse). Reaching the printf
/// after `[p release]` without crashing/hanging is the proof the guard
/// works, mirroring the original's `TEST_PASS()` (pass == "we got here").
#[test]
fn dealloc_reentrant_guard() {
    let src = format!(
        "{}{}",
        OZOBJECT_SRC,
        "\
@interface Probe : OZObject
- (void)dealloc;
@end
@implementation Probe
- (void)dealloc {
	[self retain];
	[self release];
	[super dealloc];
}
@end

#include <stdio.h>

int main(void) {
	Probe *p = [Probe alloc];
	printf(\"nonnull=%d\\n\", p != 0);
	[p release];
	printf(\"survived_reentrant_dealloc=1\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "dealloc_reentrant_guard");
    assert_eq!(stdout, "nonnull=1\nsurvived_reentrant_dealloc=1\n");
}

/// Ported from tests/behavior/cases/lifecycle/double_release_guard.{m,_test.c}.
/// Despite the fixture's name, the original `_test.c` does not actually
/// call release twice -- its own comment explains why: after the first
/// release drops rc to 0 the storage is freed, so a second release would
/// be a use-after-free, not a guarded no-op, in either pipeline. What it
/// actually verifies is that the single alloc -> release path (rc 1 -> 0,
/// dealloc, free) completes cleanly. Ported as exactly that; reaching the
/// final printf without a crash is the pass condition, mirroring the
/// original's trailing `TEST_PASS()`.
#[test]
fn release_completes_without_crash() {
    let src = format!(
        "{}{}",
        OZOBJECT_SRC,
        "\
@interface Item : OZObject
@end
@implementation Item
@end

#include <stdio.h>

int main(void) {
	Item *item = [Item alloc];
	printf(\"nonnull=%d\\n\", item != 0);
	[item release];
	printf(\"survived_release=1\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "release_completes_without_crash");
    assert_eq!(stdout, "nonnull=1\nsurvived_release=1\n");
}

/// Ported from tests/behavior/cases/lifecycle/init_sets_fields.{m,_test.c}.
/// `-init` chains to `[super init]` and sets ivar defaults; the original
/// asserted both fields via the Python pipeline's `OZ_PROTOCOL_SEND_init`
/// dispatch helper. oz_static has no such helper (dispatch is always
/// compile-time-fixed direct calls), so `-init` is just an ordinary
/// instance method here, requiring OZObject to declare/define its own
/// `-init` (a no-op returning self) for `[super init]` to resolve.
#[test]
fn init_sets_fields() {
    let src = format!(
        "{}{}",
        OZOBJECT_SRC,
        "\
@interface Gadget : OZObject {
	int _value;
	int _ready;
}
- (instancetype)init;
- (int)value;
- (int)ready;
@end

@implementation Gadget
- (instancetype)init {
	self = [super init];
	_value = 42;
	_ready = 1;
	return self;
}
- (int)value {
	return _value;
}
- (int)ready {
	return _ready;
}
@end

#include <stdio.h>

int main(void) {
	Gadget *g = [Gadget alloc];
	g = [g init];
	printf(\"value=%d\\n\", [g value]);
	printf(\"ready=%d\\n\", [g ready]);
	[g release];
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "init_sets_fields");
    assert_eq!(stdout, "value=42\nready=1\n");
}
