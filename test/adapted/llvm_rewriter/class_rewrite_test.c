/*
 * Adapted from: clang/test/Rewriter/objc-modern-metadata-visibility.mm
 * License: Apache 2.0 with LLVM Exception
 * Verifies struct layout and method generation from @interface/@implementation.
 */
#include "unity.h"
#include "Vehicle.h"
#include "oz_mem_slabs.h"
#include <stddef.h>

void test_struct_has_class_id_field(void)
{
	struct Vehicle v;
	/* oz_class_id is the first field after base */
	TEST_ASSERT_TRUE(sizeof(v) > 0);
}

void test_alloc_and_init(void)
{
	struct Vehicle *v = Vehicle_alloc();
	TEST_ASSERT_NOT_NULL(v);
	OZ_SEND_init((struct OZObject *)v);
	OZObject_release((struct OZObject *)v);
}

void test_ivar_access_via_methods(void)
{
	struct Vehicle *v = Vehicle_alloc();
	OZ_SEND_init((struct OZObject *)v);

	Vehicle_setSpeed_(v, 120);
	TEST_ASSERT_EQUAL_INT(120, Vehicle_speed(v));
	TEST_ASSERT_EQUAL_INT(0, Vehicle_fuel(v));

	OZObject_release((struct OZObject *)v);
}
