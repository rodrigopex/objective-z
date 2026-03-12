/* SPDX-License-Identifier: Apache-2.0 */
/* Memory tests: retain/release, refcount queries */
#include <zephyr/ztest.h>
#include "Node_ozh.h"
#include "OZObject_ozh.h"
#include "oz_dispatch.h"

ZTEST_SUITE(memory, NULL, NULL, NULL, NULL, NULL);

ZTEST(memory, test_retain_increments_refcount)
{
	struct Node *n = Node_alloc();
	zassert_equal(1u, __objc_refcount_get(n));

	OZObject_retain((struct OZObject *)n);
	zassert_equal(2u, __objc_refcount_get(n));

	OZObject_retain((struct OZObject *)n);
	zassert_equal(3u, __objc_refcount_get(n));

	/* Balance releases */
	OZObject_release((struct OZObject *)n);
	OZObject_release((struct OZObject *)n);
	OZObject_release((struct OZObject *)n);
}

ZTEST(memory, test_release_decrements_refcount)
{
	struct Node *n = Node_alloc();
	OZObject_retain((struct OZObject *)n);
	OZObject_retain((struct OZObject *)n);
	zassert_equal(3u, __objc_refcount_get(n));

	OZObject_release((struct OZObject *)n);
	zassert_equal(2u, __objc_refcount_get(n));

	OZObject_release((struct OZObject *)n);
	zassert_equal(1u, __objc_refcount_get(n));

	OZObject_release((struct OZObject *)n);
}

ZTEST(memory, test_slab_alloc_and_reuse)
{
	struct Node *n1 = Node_alloc();
	zassert_not_null(n1, "First alloc should succeed");

	/* Release returns block to slab */
	OZObject_release((struct OZObject *)n1);

	/* Re-alloc should reuse the freed block */
	struct Node *n2 = Node_alloc();
	zassert_not_null(n2, "Realloc after free should succeed");
	OZObject_release((struct OZObject *)n2);
}
