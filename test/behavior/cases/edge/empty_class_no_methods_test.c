/* Behavior test: empty class alloc/release cycle */
#include "unity.h"
#include "EmptyClass.h"
#include "oz_mem_slabs.h"

void test_alloc_returns_non_null(void)
{
	struct EmptyClass *obj = EmptyClass_alloc();
	TEST_ASSERT_NOT_NULL(obj);
	OZObject_release((struct OZObject *)obj);
}

void test_class_id_set_correctly(void)
{
	struct EmptyClass *obj = EmptyClass_alloc();
	TEST_ASSERT_EQUAL_INT(OZ_CLASS_EmptyClass, obj->base.oz_class_id);
	OZObject_release((struct OZObject *)obj);
}

void test_refcount_starts_at_one(void)
{
	struct EmptyClass *obj = EmptyClass_alloc();
	TEST_ASSERT_EQUAL_UINT(1, __objc_refcount_get(obj));
	OZObject_release((struct OZObject *)obj);
}
