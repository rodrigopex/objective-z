/* SPDX-License-Identifier: Apache-2.0 */
/* Dispatch tests: super calls, inheritance chain, method override */
#include <zephyr/ztest.h>

ZTEST_SUITE(dispatch, NULL, NULL, NULL, NULL, NULL);

ZTEST(dispatch, test_super_calls_parent_init)
{
	struct Child *c = Child_alloc();
	OZ_SEND_init((struct OZObject *)c);

	/* Parent init sets baseVal=10, child init sets childVal=20 */
	zassert_equal(10, Base_baseVal((struct Base *)c),
		      "Expected baseVal=10 from super init");
	zassert_equal(20, Child_childVal(c),
		      "Expected childVal=20 from child init");

	OZObject_release((struct OZObject *)c);
}

ZTEST(dispatch, test_deep_inheritance_depth)
{
	struct Level4 *l4 = Level4_alloc();
	zassert_equal(4, Level4_depth(l4), "Level4 depth should be 4");
	OZObject_release((struct OZObject *)l4);

	struct Level1 *l1 = Level1_alloc();
	zassert_equal(1, Level1_depth(l1), "Level1 depth should be 1");
	OZObject_release((struct OZObject *)l1);
}

ZTEST(dispatch, test_protocol_dispatch_depth)
{
	struct Level3 *l3 = Level3_alloc();
	/* OZ_SEND_depth dispatches via vtable */
	zassert_equal(3, OZ_SEND_depth((struct OZObject *)l3),
		      "Protocol dispatch: Level3 depth should be 3");
	OZObject_release((struct OZObject *)l3);
}
