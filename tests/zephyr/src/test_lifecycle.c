/* SPDX-License-Identifier: Apache-2.0 */
/* Lifecycle tests: alloc, class_id, refcount, release */
#include <zephyr/ztest.h>
#include "oz_mem_slabs.h"

ZTEST_SUITE(lifecycle, NULL, NULL, NULL, NULL, NULL);

ZTEST(lifecycle, test_alloc_returns_non_null)
{
	struct Widget *w = Widget_alloc();
	zassert_not_null(w, "Widget alloc returned NULL");
	OZObject_release((struct OZObject *)w);
}

ZTEST(lifecycle, test_alloc_sets_class_id)
{
	struct Widget *w = Widget_alloc();
	zassert_equal(OZ_CLASS_Widget, w->base.oz_class_id,
		      "Expected class id %d, got %d",
		      OZ_CLASS_Widget, w->base.oz_class_id);
	OZObject_release((struct OZObject *)w);
}

ZTEST(lifecycle, test_alloc_sets_refcount_one)
{
	struct Widget *w = Widget_alloc();
	zassert_equal(1u, __objc_refcount_get(w),
		      "Expected refcount 1, got %u", __objc_refcount_get(w));
	OZObject_release((struct OZObject *)w);
}

ZTEST(lifecycle, test_init_sets_default_tag)
{
	struct Widget *w = Widget_alloc();
	zassert_equal(0, Widget_tag(w), "Default tag should be 0");
	OZObject_release((struct OZObject *)w);
}
