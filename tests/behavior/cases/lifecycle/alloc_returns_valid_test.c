/* Behavior test: alloc returns a valid, non-null pointer */
#include "unity.h"
#include "Widget_ozh.h"

void test_alloc_returns_non_null(void)
{
	struct Widget *w = Widget_alloc();
	TEST_ASSERT_NOT_NULL(w);
	OZObject_release((struct OZObject *)w);
}

void test_alloc_sets_class_id(void)
{
	struct Widget *w = Widget_alloc();
	TEST_ASSERT_EQUAL_INT(OZ_CLASS_Widget, w->base.oz_class_id);
	OZObject_release((struct OZObject *)w);
}

void test_alloc_sets_refcount_one(void)
{
	struct Widget *w = Widget_alloc();
	TEST_ASSERT_EQUAL_UINT(1, __objc_refcount_get(w));
	OZObject_release((struct OZObject *)w);
}
