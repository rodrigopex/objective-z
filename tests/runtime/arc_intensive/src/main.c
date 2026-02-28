/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Intensive ARC test suite — 10 suites, ~44 tests.
 *
 * Inspired by Apple objc4 runtime tests. Validates every ARC code path
 * including scope cleanup, .cxx_destruct, autorelease pools, RVO,
 * object graphs, immortal objects, static slab leak detection,
 * heap stats, properties, and stress operations.
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>
#include <objc/arc.h>
#include <objc/malloc.h>
#include <objc/pool.h>

/* ── MRR helpers (defined in helpers.m) ────────────────────────── */

extern id test_create_tracked(int tag);
extern unsigned int test_get_rc(id obj);
extern void test_reset_tracking(void);
extern void *test_pool_push(void);
extern void test_pool_pop(void *p);
extern id test_create_pool_obj(int tag);
extern id test_prop_create(void);
extern ptrdiff_t test_prop_offset(void);
extern void *test_prop_read_ivar(id obj);
extern void test_prop_write_ivar(id obj, id val);
extern id test_get_immortal_string(void);

/* ── Tracking globals (defined in helpers.m) ──────────────────── */

extern int g_dealloc_count;
extern int g_dealloc_tags[];
extern int g_dealloc_tag_idx;

/* ── Pool slab stat wrappers (compile-time via OBJZ_POOL) ────── */

OBJZ_POOL_DECLARE(ArcPoolObj);

static uint32_t test_pool_slab_used(void)
{
	return k_mem_slab_num_used_get(&OBJZ_POOL(ArcPoolObj));
}

static uint32_t test_pool_slab_free(void)
{
	return k_mem_slab_num_free_get(&OBJZ_POOL(ArcPoolObj));
}

/* ── ARC helpers (defined in arc_helpers.m) ───────────────────── */

/* Suite 1: scope */
extern void arc_test_scope_single(void);
extern void arc_test_scope_multi_reverse(void);
extern void arc_test_scope_nested(void);
extern void arc_test_scope_early_return(void);
extern void arc_test_scope_loop(void);
extern void arc_test_scope_cond_true(void);
extern void arc_test_scope_cond_false(void);

/* Suite 2: .cxx_destruct */
extern void arc_test_cxx_single_ivar(void);
extern void arc_test_cxx_multi_ivar(void);
extern void arc_test_cxx_hierarchy(void);
extern void arc_test_cxx_chain(void);

/* Suite 3: autorelease (ARC-compiled) */
extern void arc_test_autoreleasepool_syntax(void);

/* Suite 4: RVO */
extern void arc_test_rvo_round_trip(void);
extern void arc_test_rvo_chain(void);

/* Suite 5: object graphs */
extern void arc_test_graph_linear(void);
extern void arc_test_graph_deep_chain(void);
extern void arc_test_graph_retain_cycle(void);

/* Suite 7: static pool + ARC */
extern void arc_test_pool_scope(void);
extern void arc_test_pool_cycle(int count);

/* Suite 9: properties (ARC-compiled) */
extern void arc_test_prop_atomic(void);
extern void arc_test_prop_nonatomic(void);
extern void arc_test_prop_overwrite(void);
extern void arc_test_prop_same_object(void);

/* Suite 10: stress (ARC-compiled) */
extern void arc_test_stress_alloc_loop(void);
extern void arc_test_stress_autorelease_capacity(void);

/* ══════════════════════════════════════════════════════════════════
 *  Suite 1: arc_scope — ARC compiler-generated scope cleanup
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_scope, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_scope, test_single_var)
{
	test_reset_tracking();
	arc_test_scope_single();
	zassert_equal(g_dealloc_count, 1,
		      "single local should dealloc at scope exit");
}

ZTEST(arc_scope, test_multi_reverse_order)
{
	test_reset_tracking();
	arc_test_scope_multi_reverse();
	zassert_equal(g_dealloc_count, 3,
		      "3 locals should all dealloc");
	/* Clang typically releases in reverse declaration order */
	zassert_equal(g_dealloc_tags[0], 3, "last declared released first");
	zassert_equal(g_dealloc_tags[1], 2, "middle released second");
	zassert_equal(g_dealloc_tags[2], 1, "first declared released last");
}

ZTEST(arc_scope, test_nested_scopes)
{
	test_reset_tracking();
	arc_test_scope_nested();
	zassert_equal(g_dealloc_count, 2,
		      "inner + outer should both dealloc");
	zassert_equal(g_dealloc_tags[0], 200,
		      "inner scope should dealloc first");
	zassert_equal(g_dealloc_tags[1], 100,
		      "outer scope should dealloc second");
}

ZTEST(arc_scope, test_early_return)
{
	test_reset_tracking();
	arc_test_scope_early_return();
	zassert_equal(g_dealloc_count, 1,
		      "early return should still release");
}

ZTEST(arc_scope, test_loop_variable)
{
	test_reset_tracking();
	arc_test_scope_loop();
	zassert_equal(g_dealloc_count, 5,
		      "5 loop iterations should produce 5 deallocs");
}

ZTEST(arc_scope, test_conditional_true)
{
	test_reset_tracking();
	arc_test_scope_cond_true();
	zassert_equal(g_dealloc_count, 1,
		      "true branch should dealloc");
	zassert_equal(g_dealloc_tags[0], 10, "tag from true branch");
}

ZTEST(arc_scope, test_conditional_false)
{
	test_reset_tracking();
	arc_test_scope_cond_false();
	zassert_equal(g_dealloc_count, 1,
		      "false branch should dealloc");
	zassert_equal(g_dealloc_tags[0], 20, "tag from false/else branch");
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 2: arc_cxx_destruct — .cxx_destruct ivar cleanup
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_cxx_destruct, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_cxx_destruct, test_single_ivar)
{
	test_reset_tracking();
	arc_test_cxx_single_ivar();
	/* IvarOwner dealloc (+1) + TrackedObj(10) dealloc (+1) = 2 */
	zassert_equal(g_dealloc_count, 2,
		      "owner + child should both dealloc");
}

ZTEST(arc_cxx_destruct, test_multi_ivar)
{
	test_reset_tracking();
	arc_test_cxx_multi_ivar();
	/* MultiIvarOwner (+1) + TrackedObj(10) (+1) + TrackedObj(20) (+1) = 3 */
	zassert_equal(g_dealloc_count, 3,
		      "owner + both children should dealloc");
}

ZTEST(arc_cxx_destruct, test_hierarchy)
{
	test_reset_tracking();
	arc_test_cxx_hierarchy();
	/*
	 * SubIvarOwner dealloc (+1) → .cxx_destruct releases extra(TrackedObj 20, +1)
	 * IvarOwner dealloc (+1) → .cxx_destruct releases child(TrackedObj 10, +1)
	 * Total: 4 deallocs
	 *
	 * Note: ARC auto-inserts [super dealloc] in SubIvarOwner.
	 * .cxx_destruct walks the class hierarchy calling per-class destructors.
	 */
	zassert_true(g_dealloc_count >= 3,
		     "at least owner + 2 tracked objects should dealloc");
}

ZTEST(arc_cxx_destruct, test_chain)
{
	test_reset_tracking();
	arc_test_cxx_chain();
	/* IvarOwner a (+1) → releases IvarOwner b (+1) → releases TrackedObj(99) (+1) = 3 */
	zassert_equal(g_dealloc_count, 3,
		      "chain a->b->leaf should cascade deallocs");
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 3: arc_autorelease — Autorelease pool exhaustive
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_autorelease, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_autorelease, test_basic_drain)
{
	test_reset_tracking();
	void *pool = test_pool_push();
	id obj = test_create_tracked(1);
	objc_autorelease(obj);
	test_pool_pop(pool);
	zassert_equal(g_dealloc_count, 1,
		      "pool drain should release autoreleased object");
}

ZTEST(arc_autorelease, test_nested_pools)
{
	test_reset_tracking();
	void *outer = test_pool_push();
	id obj_outer = test_create_tracked(100);
	objc_autorelease(obj_outer);

	void *inner = test_pool_push();
	id obj_inner = test_create_tracked(200);
	objc_autorelease(obj_inner);

	/* Drain inner — only inner objects released */
	test_pool_pop(inner);
	zassert_equal(g_dealloc_count, 1,
		      "only inner pool object should dealloc");
	zassert_equal(g_dealloc_tags[0], 200,
		      "inner pool tag should be 200");

	/* Drain outer — outer objects released */
	test_pool_pop(outer);
	zassert_equal(g_dealloc_count, 2,
		      "outer pool object should now dealloc too");
	zassert_equal(g_dealloc_tags[1], 100,
		      "outer pool tag should be 100");
}

ZTEST(arc_autorelease, test_empty_pool)
{
	void *pool = test_pool_push();
	/* No objects added */
	test_pool_pop(pool);
	/* If we get here, empty pool drains without crash */
	zassert_true(true, "empty pool should drain cleanly");
}

ZTEST(arc_autorelease, test_multiple_objects)
{
	test_reset_tracking();
	void *pool = test_pool_push();
	for (int i = 0; i < 10; i++) {
		id obj = test_create_tracked(i);
		objc_autorelease(obj);
	}
	test_pool_pop(pool);
	zassert_equal(g_dealloc_count, 10,
		      "all 10 autoreleased objects should dealloc");
}

ZTEST(arc_autorelease, test_lifo_order)
{
	test_reset_tracking();
	void *pool = test_pool_push();
	for (int i = 0; i < 5; i++) {
		id obj = test_create_tracked(i);
		objc_autorelease(obj);
	}
	test_pool_pop(pool);
	zassert_equal(g_dealloc_count, 5, "5 objects should dealloc");
	/* Pool drains in reverse (LIFO) order */
	zassert_equal(g_dealloc_tags[0], 4, "last autoreleased first drained");
	zassert_equal(g_dealloc_tags[1], 3, "second-to-last drained second");
	zassert_equal(g_dealloc_tags[2], 2, "middle drained third");
	zassert_equal(g_dealloc_tags[3], 1, "second drained fourth");
	zassert_equal(g_dealloc_tags[4], 0, "first autoreleased last drained");
}

ZTEST(arc_autorelease, test_autoreleasepool_syntax)
{
	test_reset_tracking();
	arc_test_autoreleasepool_syntax();
	zassert_equal(g_dealloc_count, 1,
		      "@autoreleasepool {} should drain and dealloc");
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 4: arc_rvo — Return Value Optimization
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_rvo, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_rvo, test_round_trip)
{
	test_reset_tracking();
	arc_test_rvo_round_trip();
	zassert_equal(g_dealloc_count, 1,
		      "factory→consumer should produce exactly 1 dealloc");
}

ZTEST(arc_rvo, test_chain)
{
	test_reset_tracking();
	arc_test_rvo_chain();
	zassert_equal(g_dealloc_count, 1,
		      "factory→intermediate→consumer should produce 1 dealloc");
}

ZTEST(arc_rvo, test_retainAutoreleaseReturnValue)
{
	test_reset_tracking();
	void *pool = test_pool_push();
	id obj = test_create_tracked(1);

	id ret = objc_retainAutoreleaseReturnValue(obj);
	zassert_equal(ret, obj, "should return same object");
	/* retain + autorelease: rc goes from 1 to 2 (retain), pool owns one */
	zassert_equal(test_get_rc(obj), 2,
		      "rc should be 2 after retainAutoreleaseReturnValue");

	test_pool_pop(pool);
	zassert_equal(test_get_rc(obj), 1,
		      "rc should be 1 after pool drain");
	objc_release(obj);
	zassert_equal(g_dealloc_count, 1, "should dealloc after final release");
}

ZTEST(arc_rvo, test_retainAutorelease)
{
	test_reset_tracking();
	void *pool = test_pool_push();
	id obj = test_create_tracked(1);

	id ret = objc_retainAutorelease(obj);
	zassert_equal(ret, obj, "should return same object");
	zassert_equal(test_get_rc(obj), 2,
		      "rc should be 2 after retainAutorelease");

	test_pool_pop(pool);
	zassert_equal(test_get_rc(obj), 1,
		      "rc should be 1 after pool drain");
	objc_release(obj);
	zassert_equal(g_dealloc_count, 1, "should dealloc");
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 5: arc_graph — Object graph lifecycle
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_graph, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_graph, test_linear_ab)
{
	test_reset_tracking();
	arc_test_graph_linear();
	zassert_equal(g_dealloc_count, 2,
		      "A→B linear graph: both should dealloc");
}

ZTEST(arc_graph, test_deep_chain)
{
	test_reset_tracking();
	arc_test_graph_deep_chain();
	zassert_equal(g_dealloc_count, 4,
		      "A→B→C→D chain: all 4 should dealloc");
}

ZTEST(arc_graph, test_retain_cycle_leaks)
{
	test_reset_tracking();
	arc_test_graph_retain_cycle();
	/*
	 * A↔B cycle: neither should dealloc because the cycle keeps
	 * both alive. This is expected behavior — no weak refs.
	 * This test verifies the runtime does NOT crash on cycles.
	 */
	zassert_equal(g_dealloc_count, 0,
		      "retain cycle should NOT dealloc (expected leak)");
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 6: arc_immortal — Immortal object safety
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_immortal, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_immortal, test_string_retain_noop)
{
	id str = test_get_immortal_string();
	unsigned int rc_before = test_get_rc(str);
	objc_retain(str);
	unsigned int rc_after = test_get_rc(str);
	/*
	 * Immortal objects have objc_class_flag_immortal set.
	 * retain should be a no-op (refcount does not change).
	 */
	zassert_equal(rc_before, rc_after,
		      "immortal string retain should not change rc");
}

ZTEST(arc_immortal, test_string_release_noop)
{
	id str = test_get_immortal_string();
	unsigned int rc_before = test_get_rc(str);
	objc_release(str);
	unsigned int rc_after = test_get_rc(str);
	zassert_equal(rc_before, rc_after,
		      "immortal string release should not change rc");
}

ZTEST(arc_immortal, test_string_stress)
{
	id str = test_get_immortal_string();
	unsigned int rc_before = test_get_rc(str);
	for (int i = 0; i < 100; i++) {
		objc_retain(str);
	}
	for (int i = 0; i < 100; i++) {
		objc_release(str);
	}
	unsigned int rc_after = test_get_rc(str);
	zassert_equal(rc_before, rc_after,
		      "100x retain/release on immortal should be stable");
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 7: arc_pool — Static slab + ARC leak detection
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_slab, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_slab, test_scope_returns_slab)
{
	test_reset_tracking();
	uint32_t used_before = test_pool_slab_used();

	arc_test_pool_scope();

	uint32_t used_after = test_pool_slab_used();
	zassert_equal(g_dealloc_count, 1,
		      "pooled object should dealloc at scope exit");
	zassert_equal(used_after, used_before,
		      "slab should return to same used count");
}

ZTEST(arc_slab, test_cycle_consistency)
{
	test_reset_tracking();
	uint32_t free_before = test_pool_slab_free();

	arc_test_pool_cycle(10);

	uint32_t free_after = test_pool_slab_free();
	zassert_equal(g_dealloc_count, 10,
		      "10 cycles should produce 10 deallocs");
	zassert_equal(free_after, free_before,
		      "slab free count should be unchanged after cycles");
}

ZTEST(arc_slab, test_mrr_alloc_arc_release)
{
	test_reset_tracking();
	uint32_t used_before = test_pool_slab_used();

	/* Alloc via MRR wrapper, release via ARC entry point */
	id obj = test_create_pool_obj(42);
	zassert_not_null(obj, "pool alloc should succeed");

	uint32_t used_during = test_pool_slab_used();
	zassert_equal(used_during, used_before + 1,
		      "slab should show 1 more used");

	objc_release(obj);

	uint32_t used_after = test_pool_slab_used();
	zassert_equal(g_dealloc_count, 1, "should dealloc");
	zassert_equal(used_after, used_before,
		      "slab should return to original used count");
}

ZTEST(arc_slab, test_exhaustion_heap_fallback)
{
	test_reset_tracking();
	uint32_t free_before = test_pool_slab_free();
	struct sys_memory_stats heap_before;
	objc_stats(&heap_before);

	/* Allocate all 8 slab slots */
	id slab_objs[8];
	for (int i = 0; i < 8; i++) {
		slab_objs[i] = test_create_pool_obj(i);
		zassert_not_null(slab_objs[i], "slab alloc %d should succeed", i);
	}
	zassert_equal(test_pool_slab_free(), 0,
		      "all 8 slab slots should be used");

	/* 2 more allocations fall back to heap */
	id heap_objs[2];
	for (int i = 0; i < 2; i++) {
		heap_objs[i] = test_create_pool_obj(100 + i);
		zassert_not_null(heap_objs[i], "heap fallback %d should succeed", i);
	}

	/* Release all via objc_release (ARC entry point) */
	for (int i = 0; i < 8; i++) {
		objc_release(slab_objs[i]);
	}
	for (int i = 0; i < 2; i++) {
		objc_release(heap_objs[i]);
	}

	zassert_equal(g_dealloc_count, 10,
		      "all 10 objects should dealloc");
	zassert_equal(test_pool_slab_free(), free_before,
		      "slab should be fully free");

	struct sys_memory_stats heap_after;
	objc_stats(&heap_after);
	zassert_equal(heap_after.allocated_bytes, heap_before.allocated_bytes,
		      "heap should return to baseline after fallback cleanup");
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 8: arc_heap — Heap stats leak detection
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_heap, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_heap, test_heap_baseline)
{
	test_reset_tracking();
	struct sys_memory_stats before, after;
	objc_stats(&before);

	id obj = test_create_tracked(1);
	zassert_not_null(obj, "alloc should succeed");

	struct sys_memory_stats during;
	objc_stats(&during);
	zassert_true(during.allocated_bytes > before.allocated_bytes,
		     "allocated bytes should increase during alloc");

	objc_release(obj);

	objc_stats(&after);
	zassert_equal(after.allocated_bytes, before.allocated_bytes,
		      "allocated bytes should return to baseline after dealloc");
}

ZTEST(arc_heap, test_no_leak_cycle)
{
	test_reset_tracking();
	struct sys_memory_stats before, after;
	objc_stats(&before);

	for (int i = 0; i < 50; i++) {
		id obj = test_create_tracked(i);
		objc_release(obj);
	}

	objc_stats(&after);
	zassert_equal(g_dealloc_count, 50, "50 objects should dealloc");
	zassert_equal(after.allocated_bytes, before.allocated_bytes,
		      "50 alloc/release cycles should not leak");
}

ZTEST(arc_heap, test_autorelease_heap_baseline)
{
	test_reset_tracking();
	struct sys_memory_stats before, after;
	objc_stats(&before);

	void *pool = test_pool_push();
	for (int i = 0; i < 10; i++) {
		id obj = test_create_tracked(i);
		objc_autorelease(obj);
	}
	test_pool_pop(pool);

	objc_stats(&after);
	zassert_equal(g_dealloc_count, 10, "10 objects should dealloc on drain");
	zassert_equal(after.allocated_bytes, before.allocated_bytes,
		      "heap should return to baseline after pool drain");
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 9: arc_property — Property accessor edge cases
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_property, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_property, test_overwrite)
{
	test_reset_tracking();
	id obj = test_prop_create();
	id old_val = test_create_tracked(1);
	id new_val = test_create_tracked(2);
	ptrdiff_t off = test_prop_offset();

	objc_setProperty(obj, NULL, off, old_val, NO, NO);
	zassert_equal(test_get_rc(old_val), 2, "old_val rc should be 2");

	objc_setProperty(obj, NULL, off, new_val, NO, NO);
	zassert_equal(test_get_rc(old_val), 1, "old_val rc should drop to 1");
	zassert_equal(test_get_rc(new_val), 2, "new_val rc should be 2");

	objc_setProperty(obj, NULL, off, nil, NO, NO);
	objc_release(old_val);
	objc_release(new_val);
	objc_release(obj);
}

ZTEST(arc_property, test_same_object_noop)
{
	id obj = test_prop_create();
	id val = test_create_tracked(1);
	ptrdiff_t off = test_prop_offset();

	objc_setProperty(obj, NULL, off, val, NO, NO);
	zassert_equal(test_get_rc(val), 2, "rc should be 2");

	objc_setProperty(obj, NULL, off, val, NO, NO);
	zassert_equal(test_get_rc(val), 2,
		      "setting same object should not change rc");

	objc_setProperty(obj, NULL, off, nil, NO, NO);
	objc_release(val);
	objc_release(obj);
}

ZTEST(arc_property, test_specialized_atomic_setter)
{
	id obj = test_prop_create();
	id val = test_create_tracked(1);
	ptrdiff_t off = test_prop_offset();

	objc_setProperty_atomic(obj, NULL, val, off);
	zassert_equal(test_get_rc(val), 2, "val rc should be 2 after atomic set");

	id stored = test_prop_read_ivar(obj);
	zassert_equal(stored, val, "ivar should hold stored value");

	objc_setProperty_atomic(obj, NULL, nil, off);
	zassert_equal(test_get_rc(val), 1, "val rc should drop after nil set");

	objc_release(val);
	objc_release(obj);
}

ZTEST(arc_property, test_specialized_nonatomic_setter)
{
	id obj = test_prop_create();
	id val = test_create_tracked(1);
	ptrdiff_t off = test_prop_offset();

	objc_setProperty_nonatomic(obj, NULL, val, off);
	zassert_equal(test_get_rc(val), 2, "val rc should be 2");

	objc_setProperty_nonatomic(obj, NULL, nil, off);
	zassert_equal(test_get_rc(val), 1, "val rc should drop");

	objc_release(val);
	objc_release(obj);
}

ZTEST(arc_property, test_arc_atomic)
{
	test_reset_tracking();
	arc_test_prop_atomic();
	zassert_equal(g_dealloc_count, 2,
		      "holder + value should both dealloc");
}

ZTEST(arc_property, test_arc_nonatomic)
{
	test_reset_tracking();
	arc_test_prop_nonatomic();
	zassert_equal(g_dealloc_count, 2,
		      "holder + value should both dealloc");
}

ZTEST(arc_property, test_arc_overwrite)
{
	test_reset_tracking();
	arc_test_prop_overwrite();
	/* PropHolderNonatomic(+1) + TrackedObj(10)(+1) + TrackedObj(20)(+1) = 3 */
	zassert_equal(g_dealloc_count, 3,
		      "holder + both values should dealloc");
}

ZTEST(arc_property, test_arc_same_object)
{
	test_reset_tracking();
	arc_test_prop_same_object();
	/* PropHolderNonatomic(+1) + TrackedObj(3)(+1) = 2 */
	zassert_equal(g_dealloc_count, 2,
		      "holder + value should dealloc (same-obj set is safe)");
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 10: arc_stress — Stress / repeated operations
 * ══════════════════════════════════════════════════════════════════ */

ZTEST_SUITE(arc_stress, NULL, NULL, NULL, NULL, NULL);

ZTEST(arc_stress, test_retain_release_1000)
{
	id obj = test_create_tracked(1);

	for (int i = 0; i < 1000; i++) {
		objc_retain(obj);
	}
	zassert_equal(test_get_rc(obj), 1001, "rc should be 1001");

	for (int i = 0; i < 1000; i++) {
		objc_release(obj);
	}
	zassert_equal(test_get_rc(obj), 1, "rc should return to 1");

	objc_release(obj);
}

ZTEST(arc_stress, test_alloc_loop_100)
{
	test_reset_tracking();
	struct sys_memory_stats before, after;
	objc_stats(&before);

	arc_test_stress_alloc_loop();

	objc_stats(&after);
	zassert_equal(g_dealloc_count, 100,
		      "100 iterations should produce 100 deallocs");
	zassert_equal(after.allocated_bytes, before.allocated_bytes,
		      "heap should return to baseline");
}

ZTEST(arc_stress, test_autorelease_near_capacity)
{
	test_reset_tracking();
	arc_test_stress_autorelease_capacity();
	zassert_equal(g_dealloc_count, 60,
		      "60 autoreleased objects should all dealloc");
}

ZTEST(arc_stress, test_pool_slab_50_cycles)
{
	test_reset_tracking();
	uint32_t free_before = test_pool_slab_free();

	for (int i = 0; i < 50; i++) {
		id obj = test_create_pool_obj(i);
		objc_release(obj);
	}

	zassert_equal(g_dealloc_count, 50,
		      "50 pool alloc/release cycles should produce 50 deallocs");
	zassert_equal(test_pool_slab_free(), free_before,
		      "slab should be fully free after cycles");
}

ZTEST(arc_stress, test_storeStrong_100_swaps)
{
	test_reset_tracking();
	id loc = nil;
	struct sys_memory_stats before, after;
	objc_stats(&before);

	for (int i = 0; i < 100; i++) {
		id obj = test_create_tracked(i);
		objc_storeStrong(&loc, obj);
		objc_release(obj); /* balance the alloc — storeStrong retained */
	}

	/* loc still holds the last object — release it */
	objc_storeStrong(&loc, nil);

	objc_stats(&after);
	zassert_equal(g_dealloc_count, 100,
		      "100 storeStrong swaps should produce 100 deallocs");
	zassert_equal(after.allocated_bytes, before.allocated_bytes,
		      "heap should return to baseline");
}
