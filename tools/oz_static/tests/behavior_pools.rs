// SPDX-License-Identifier: Apache-2.0
//
// behavior_pools.rs - the slab allocator and its sizing.
//
// oz_static used to allocate every object with plain malloc, which is
// unbounded, so nothing about pool sizing was observable. Objects now come
// from a per-class `OZ_SLAB_DEFINE` slab (`companion::render_alloc_free`):
// a real `k_mem_slab` on Zephyr, and on host a malloc-backed slab that
// still enforces the block count. That enforcement is what makes these
// tests possible at all -- exhaustion is reachable on host, not only on
// hardware.
//
// Sizes come from counting allocation *sites*
// (`pools::PoolSizes::analyze`), ported from the oracle's
// `_count_alloc_calls`, overridable by the `/* oz-pool: Class=N */`
// directive the oracle's own cases use and by `--pool-sizes` on the
// command line.

mod common;
use common::{compile_and_run, ozobject_src};

/// A one-slot pool serves one *simultaneously live* object; the next
/// request returns nil rather than corrupting anything.
///
/// The two allocations are held in separate locals on purpose. An earlier
/// version of this test allocated in a loop and relied on nothing releasing
/// the previous iteration's object -- which stopped being true once
/// scope-based ARC landed (`emit::render_scoped_block`), because the local
/// is now released at the end of each iteration and the slot recycles. That
/// made the test pass for the wrong reason and then fail outright, so the
/// bound is now demonstrated by real concurrent liveness, which no amount of
/// reclamation can undo.
#[test]
fn pool_bound_is_enforced_and_exhaustion_returns_nil() {
    let src = format!(
        "/* oz-pool: Counter=1 */\n{}{}",
        ozobject_src(),
        "\
@interface Counter : OZObject {
	int _n;
}
@end
@implementation Counter
@end

#include <stdio.h>
int main(void) {
	Counter *first = [Counter alloc];
	Counter *second = [Counter alloc];
	printf(\"first_ok=%d\\n\", first != 0);
	printf(\"second_is_nil=%d\\n\", second == 0);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "pool_bound_is_enforced_and_exhaustion_returns_nil");
    assert_eq!(stdout, "first_ok=1\nsecond_is_nil=1\n");
}

/// Releasing returns the slot, so the same one-slot pool serves any number
/// of sequential allocations. This is what distinguishes a slab from a
/// counter that only ever decrements.
#[test]
fn released_slot_is_reused() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Counter : OZObject {
	int _n;
}
@end
@implementation Counter
@end

#include <stdio.h>
int main(void) {
	int ok = 0;
	for (int i = 0; i < 5; i++) {
		Counter *c = [Counter alloc];
		if (c) {
			ok = ok + 1;
		}
		[c release];
	}
	printf(\"ok=%d\\n\", ok);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "released_slot_is_reused");
    assert_eq!(stdout, "ok=5\n");
}

/// The `/* oz-pool: ... */` directive raises the bound. Same source as
/// `pool_bound_is_enforced_and_exhaustion_returns_nil` apart from the
/// directive, so the difference in outcome is attributable to it alone.
#[test]
fn oz_pool_directive_raises_the_bound() {
    let src = format!(
        "/* oz-pool: Counter=3 */\n{}{}",
        ozobject_src(),
        "\
@interface Counter : OZObject {
	int _n;
}
@end
@implementation Counter
@end

#include <stdio.h>
int main(void) {
	int ok = 0;
	int failed = 0;
	for (int i = 0; i < 3; i++) {
		Counter *c = [Counter alloc];
		if (c) {
			ok = ok + 1;
		} else {
			failed = failed + 1;
		}
	}
	printf(\"ok=%d\\n\", ok);
	printf(\"failed=%d\\n\", failed);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "oz_pool_directive_raises_the_bound");
    assert_eq!(stdout, "ok=3\nfailed=0\n");
}

/// Several distinct allocation sites each reserve a slot, so straight-line
/// code that allocates N times needs no directive at all.
#[test]
fn each_alloc_site_reserves_a_slot() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Counter : OZObject {
	int _n;
}
@end
@implementation Counter
@end

#include <stdio.h>
int main(void) {
	Counter *a = [Counter alloc];
	Counter *b = [Counter alloc];
	Counter *c = [Counter alloc];
	printf(\"all=%d\\n\", (a != 0) + (b != 0) + (c != 0));
	printf(\"distinct=%d\\n\", (a != b) && (b != c) && (a != c));
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "each_alloc_site_reserves_a_slot");
    assert_eq!(stdout, "all=3\ndistinct=1\n");
}

/// A pool size naming a class that doesn't exist is a hard error, not a
/// silently-ignored line -- otherwise the pool quietly keeps its counted
/// size and a typo looks like it worked.
#[test]
fn pool_size_for_unknown_class_rejected() {
    let overrides: oz_static::PoolOverrides =
        [("Nonexistent".to_string(), 4usize)].into_iter().collect();
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Counter : OZObject
@end
@implementation Counter
@end
"
    );
    let diags = match oz_static::transpile_with_pool_sizes(&src, &overrides) {
        Ok(_) => panic!("expected an unknown --pool-sizes class to be rejected"),
        Err(diags) => diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"),
    };
    assert!(diags.contains("Nonexistent"), "diagnostics: {}", diags);
    assert!(diags.contains("not a class"), "diagnostics: {}", diags);
}

/// A malformed `--pool-sizes` argument is rejected at parse time. The
/// identically-shaped source directive is deliberately lenient instead
/// (see `pools::parse_pool_directive`) -- a flag was unambiguously meant
/// as one, a comment may not have been.
#[test]
fn malformed_pool_sizes_argument_rejected() {
    assert!(oz_static::pools::parse_pool_sizes("Counter=3").is_ok());
    assert!(oz_static::pools::parse_pool_sizes("Counter").is_err());
    assert!(oz_static::pools::parse_pool_sizes("Counter=abc").is_err());
    assert!(oz_static::pools::parse_pool_sizes("=3").is_err());
}

/// Guards the reason `for_class` floors at one: a class nothing allocates
/// still gets a usable slab, because its alloc function is emitted whether
/// or not this translation unit calls it, and a zero-block
/// `K_MEM_SLAB_DEFINE` is not a slab.
#[test]
fn never_allocated_class_still_gets_a_slot() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Unused : OZObject {
	int _n;
}
@end
@implementation Unused
@end

#include <stdio.h>
int main(void) {
	printf(\"ran=1\\n\");
	return 0;
}
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    let all = format!("{}{}", out.source_c, out.companion_c);
    assert!(
        all.contains("OZ_SLAB_DEFINE(oz_slab_Unused, sizeof(struct Unused), 1, 4)"),
        "expected a one-slot slab for the unused class; got:\n{}",
        all
    );
    let stdout = compile_and_run(&src, "never_allocated_class_still_gets_a_slot");
    assert_eq!(stdout, "ran=1\n");
}
