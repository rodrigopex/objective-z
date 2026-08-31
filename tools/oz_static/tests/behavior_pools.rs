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
use common::{
    compile_and_run, ozarray_src, ozdictionary_src, ozobject_src, ozq31_src,
};

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


// ---------------------------------------------------------------------
// The shared element pool (OZ-098)
//
// `@[...]` and `@{...}` need a buffer of `id` slots beyond the collection
// object itself. Those buffers used to come from plain `malloc`, which is
// exactly what the per-class slab work removed everywhere else, so on a
// no-heap Zephyr target a literal still reached libc. They now come from
// one shared `OZ_MEM_BLOCKS_DEFINE(oz_item_pool, ...)` through the PAL --
// `sys_mem_blocks` on Zephyr, a count-enforcing malloc wrapper on host.
//
// As with the slabs above, it is that host-side count enforcement that
// makes exhaustion observable here rather than only on hardware.
// ---------------------------------------------------------------------

/// Two literals, five elements between them, so the pool holds five.
/// Element counts are the point: the object slots are counted separately
/// (one per literal), and conflating the two would size the pool at two.
#[test]
fn item_pool_is_sized_from_literal_element_counts() {
    let src = format!(
        "{}{}{}{}",
        ozobject_src(),
        ozq31_src(),
        ozarray_src(),
        "\
@interface Lits : OZObject
- (unsigned int)run;
@end
@implementation Lits
- (unsigned int)run {
	OZArray *three = @[@(1), @(2), @(3)];
	OZArray *two = @[@(4), @(5)];
	return [three count] + [two count];
}
@end

int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.companion_c
            .contains("OZ_MEM_BLOCKS_DEFINE(oz_item_pool, sizeof(struct OZObject *), 5, 4)"),
        "expected a five-slot item pool; got:\n{}",
        out.companion_c
    );
    assert!(
        out.companion_h.contains("extern oz_mem_blocks_t oz_item_pool;"),
        "expected the pool to be declared in the shared header; got:\n{}",
        out.companion_h
    );
}

/// A dictionary literal takes two slots per pair, not one: keys and values
/// share one contiguous run, `_keys` pointing at its first half and
/// `_values` at its second (`companion::render_dict_support`). Sizing it
/// per pair would hand back half the buffer the builder writes into.
#[test]
fn dictionary_literal_reserves_two_slots_per_pair() {
    let src = format!(
        "{}{}{}{}",
        ozobject_src(),
        ozq31_src(),
        ozdictionary_src(),
        "\
@interface Dicts : OZObject
- (unsigned int)run;
@end
@implementation Dicts
- (unsigned int)run {
	OZDictionary *d = @{@(1): @(10), @(2): @(20)};
	return [d count];
}
@end

int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.companion_c
            .contains("OZ_MEM_BLOCKS_DEFINE(oz_item_pool, sizeof(struct OZObject *), 4, 4)"),
        "expected two pairs to reserve four slots; got:\n{}",
        out.companion_c
    );
}

/// No literals, no pool -- not a zero-sized one. `SYS_MEM_BLOCKS_DEFINE`
/// with a zero block count is not a usable pool, and nothing would draw
/// from it anyway.
#[test]
fn no_item_pool_is_emitted_when_nothing_needs_one() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Plain : OZObject
@end
@implementation Plain
@end

int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    let all = format!("{}{}{}", out.source_c, out.companion_h, out.companion_c);
    assert!(
        !all.contains("oz_item_pool"),
        "expected no item pool at all; got:\n{}",
        all
    );
}

/// The `oz-item-pool:` directive overrides the counted size, the same way
/// `oz-pool:` does for class slabs.
#[test]
fn item_pool_directive_raises_the_bound() {
    let src = format!(
        "/* oz-item-pool: 16 */\n{}{}{}{}",
        ozobject_src(),
        ozq31_src(),
        ozarray_src(),
        "\
@interface Lits : OZObject
- (unsigned int)run;
@end
@implementation Lits
- (unsigned int)run {
	OZArray *a = @[@(1)];
	return [a count];
}
@end

int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    assert!(
        out.companion_c
            .contains("OZ_MEM_BLOCKS_DEFINE(oz_item_pool, sizeof(struct OZObject *), 16, 4)"),
        "expected the directive to win over the counted size of 1; got:\n{}",
        out.companion_c
    );
}

/// The two directives are scanned with separate keys and must not read
/// each other's numbers. `"oz-pool:"` does not occur inside
/// `"oz-item-pool:"` -- after `oz-` comes `item-` -- so neither `find`
/// can match the other, and this pins that down against a future rename.
#[test]
fn item_pool_directive_does_not_disturb_the_class_pool_directive() {
    let src = format!(
        "/* oz-item-pool: 9 */\n/* oz-pool: Lits=5 */\n{}{}{}{}",
        ozobject_src(),
        ozq31_src(),
        ozarray_src(),
        "\
@interface Lits : OZObject
- (unsigned int)run;
@end
@implementation Lits
- (unsigned int)run {
	OZArray *a = @[@(1)];
	return [a count];
}
@end

int main(void) { return 0; }
"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    let all = format!("{}{}", out.source_c, out.companion_c);
    assert!(
        all.contains("OZ_MEM_BLOCKS_DEFINE(oz_item_pool, sizeof(struct OZObject *), 9, 4)"),
        "item pool should be 9; got:\n{}",
        all
    );
    assert!(
        all.contains("OZ_SLAB_DEFINE(oz_slab_Lits, sizeof(struct Lits), 5, 4)"),
        "class slab should still be 5; got:\n{}",
        all
    );
}

/// The buffers really are bounded now: a pool with room for three slots
/// serves a three-element literal and then fails, and the builder answers
/// nil rather than handing back a half-built array.
///
/// Two separate live locals rather than a loop, for the reason
/// `pool_bound_is_enforced_and_exhaustion_returns_nil` above documents --
/// scope-based ARC recycles a loop-local's slot, so a loop would prove
/// nothing. That now holds for a *reassigned* local too, not only a fresh
/// per-iteration one -- an overwrite releases the previous object
/// (`emit::render_strong_local_assign`), which is why
/// `arc_strong_locals::reassigned_literal_needs_only_one_slot_and_one_buffer`
/// can run 100 iterations on one slot and a 2-slot item pool. What stays a
/// hard error is a literal the loop *accumulates*: see
/// `static_bar_rejects::array_literal_accumulated_in_a_loop_rejected`.
#[test]
fn item_pool_bound_is_enforced_and_exhaustion_returns_nil() {
    let src = format!(
        "/* oz-item-pool: 3 */\n{}{}{}{}",
        ozobject_src(),
        ozq31_src(),
        ozarray_src(),
        "\
#include <stdio.h>
int main(void) {
	OZArray *first = @[@(1), @(2), @(3)];
	OZArray *second = @[@(4), @(5), @(6)];
	printf(\"first=%d\\n\", first != 0);
	printf(\"second=%d\\n\", second != 0);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "item_pool_bound_is_enforced_and_exhaustion_returns_nil");
    assert_eq!(stdout, "first=1\nsecond=0\n");
}

/// Releasing a collection returns its element slots, so a pool sized for
/// one literal serves any number of non-overlapping ones. This is the
/// half `free_contiguous` is responsible for; without it the first test
/// above would still pass while the pool leaked every buffer.
#[test]
fn item_slots_return_to_the_pool_when_the_collection_is_released() {
    let src = format!(
        "/* oz-item-pool: 2 */\n{}{}{}{}",
        ozobject_src(),
        ozq31_src(),
        ozarray_src(),
        "\
#include <stdio.h>
int main(void) {
	int ok = 1;
	for (int i = 0; i < 5; i++) {
		OZArray *a = @[@(1), @(2)];
		if (a == 0) {
			ok = 0;
		}
	}
	printf(\"all=%d\\n\", ok);
	return 0;
}
"
    );
    let stdout =
        compile_and_run(&src, "item_slots_return_to_the_pool_when_the_collection_is_released");
    assert_eq!(stdout, "all=1\n");
}
