/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Tests for the global flat dispatch table.
 */
#include <objc/runtime.h>
#include <zephyr/ztest.h>

/* C-callable wrappers from helpers.m */
extern id test_fd_create_base(void);
extern id test_fd_create_child(void);
extern id test_fd_create_grandchild(void);
extern id test_fd_create_peer(void);
extern void test_fd_dealloc(id obj);
extern int test_fd_call_value(id obj);
extern int test_fd_call_shared(id obj);
extern int test_fd_call_class_value(void);
extern int test_fd_call_child_only(id obj);
extern Class test_fd_get_base_class(void);

/* Runtime entry points for super send test */
struct objc_selector {
	const char *name;
	const char *types;
};
struct objc_super {
	id receiver;
	Class superclass;
};
extern IMP objc_msg_lookup(id, SEL);
extern IMP objc_msg_lookup_super(struct objc_super *, SEL);

/* ── Direct instance method dispatch ─────────────────────────────── */

ZTEST(flat_dispatch, test_direct_method)
{
	id obj = test_fd_create_base();

	zassert_not_null(obj, "alloc failed");
	zassert_equal(test_fd_call_value(obj), 10, "FDBase -value should return 10");
	zassert_equal(test_fd_call_value(obj), 10, "second call should also return 10");
	test_fd_dealloc(obj);
}

/* ── Direct class method dispatch ────────────────────────────────── */

ZTEST(flat_dispatch, test_class_method)
{
	zassert_equal(test_fd_call_class_value(), 42, "FDBase +classValue should return 42");
}

/* ── Inherited method at depth=1 ─────────────────────────────────── */

ZTEST(flat_dispatch, test_inherited_depth1)
{
	id child = test_fd_create_child();

	zassert_not_null(child, "alloc failed");

	/* Inherited from FDBase */
	zassert_equal(test_fd_call_value(child), 10, "FDChild should inherit -value -> 10");

	/* Own method */
	zassert_equal(test_fd_call_child_only(child), 20, "FDChild -childOnly -> 20");

	test_fd_dealloc(child);
}

/* ── Inherited method at depth=2 ─────────────────────────────────── */

ZTEST(flat_dispatch, test_inherited_depth2)
{
	id gc = test_fd_create_grandchild();

	zassert_not_null(gc, "alloc failed");

	/* Inherited through FDChild -> FDBase */
	zassert_equal(test_fd_call_value(gc), 10, "FDGrandChild should inherit -value -> 10");

	/* Inherited from FDChild */
	zassert_equal(test_fd_call_child_only(gc), 20,
		      "FDGrandChild should inherit -childOnly -> 20");

	test_fd_dealloc(gc);
}

/* ── Category method override ────────────────────────────────────── */

ZTEST(flat_dispatch, test_category_override)
{
	id base = test_fd_create_base();

	zassert_not_null(base, "alloc failed");

	/* Category FDBase(Override) replaces -shared: 100 -> 999 */
	zassert_equal(test_fd_call_shared(base), 999,
		      "category should override FDBase -shared -> 999");

	test_fd_dealloc(base);
}

/* ── No cross-class contamination ────────────────────────────────── */

ZTEST(flat_dispatch, test_no_cross_contamination)
{
	id base = test_fd_create_base();
	id peer = test_fd_create_peer();

	zassert_not_null(base, "alloc failed");
	zassert_not_null(peer, "alloc failed");

	/* Same selector name, different classes, different IMPs */
	zassert_equal(test_fd_call_value(base), 10, "FDBase -value -> 10");
	zassert_equal(test_fd_call_value(peer), 77, "FDPeer -value -> 77");
	zassert_equal(test_fd_call_shared(peer), 200, "FDPeer -shared -> 200");

	test_fd_dealloc(base);
	test_fd_dealloc(peer);
}

/* ── objc_msg_lookup_super correctness ───────────────────────────── */

ZTEST(flat_dispatch, test_super_send)
{
	id child = test_fd_create_child();

	zassert_not_null(child, "alloc failed");

	/*
	 * Category overrides FDBase -shared -> 999.
	 * FDChild inherits that override.  A [super shared] from
	 * FDChild should also see the category override (999),
	 * since super resolves to FDBase which has the category applied.
	 */
	struct objc_selector shared_sel = {.name = "shared", .types = NULL};
	struct objc_super sup = {.receiver = child, .superclass = test_fd_get_base_class()};
	IMP imp = objc_msg_lookup_super(&sup, &shared_sel);

	zassert_not_null(imp, "super lookup should find -shared");
	int result = ((int (*)(id, SEL))imp)(child, &shared_sel);

	zassert_equal(result, 999, "[super shared] should return category override 999");

	test_fd_dealloc(child);
}

/* ── Inherited category override at depth=2 ──────────────────────── */

ZTEST(flat_dispatch, test_inherited_category_override)
{
	id gc = test_fd_create_grandchild();
	id peer = test_fd_create_peer();

	zassert_not_null(gc, "alloc failed");
	zassert_not_null(peer, "alloc failed");

	/* GrandChild inherits FDBase(Override) -shared -> 999 */
	zassert_equal(test_fd_call_shared(gc), 999,
		      "FDGrandChild should inherit category override -> 999");

	/* Peer has its own -shared -> 200 (unaffected by category) */
	zassert_equal(test_fd_call_shared(peer), 200, "FDPeer -shared -> 200");

	test_fd_dealloc(gc);
	test_fd_dealloc(peer);
}

/* ── Unknown selector returns NULL ───────────────────────────────── */

ZTEST(flat_dispatch, test_unknown_selector)
{
	id base = test_fd_create_base();
	struct objc_selector fake_sel = {.name = "nonExistentMethod99", .types = NULL};

	zassert_not_null(base, "alloc failed");

	IMP imp = objc_msg_lookup(base, &fake_sel);

	/*
	 * Unknown selectors should return NULL from the flat table.
	 * objc_msg_lookup prints a warning but still returns NULL.
	 */
	zassert_is_null(imp, "unknown selector should return NULL IMP");

	test_fd_dealloc(base);
}

ZTEST_SUITE(flat_dispatch, NULL, NULL, NULL, NULL, NULL);
