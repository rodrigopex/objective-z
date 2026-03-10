/*
 * Behavioral spec: methods with computed return values.
 * Original test — no Apple code.
 */
#include "unity.h"
#include "Geometry.h"
#include "oz_mem_slabs.h"

void test_origin_methods(void)
{
	struct Geometry *g = Geometry_alloc();
	OZ_SEND_init((struct OZObject *)g);

	Geometry_setOriginX_y_(g, 10, 20);
	TEST_ASSERT_EQUAL_INT(10, Geometry_originX(g));
	TEST_ASSERT_EQUAL_INT(20, Geometry_originY(g));

	OZObject_release((struct OZObject *)g);
}

void test_dimension_methods(void)
{
	struct Geometry *g = Geometry_alloc();
	OZ_SEND_init((struct OZObject *)g);

	Geometry_setWidth_height_(g, 100, 200);
	TEST_ASSERT_EQUAL_INT(100, Geometry_width(g));
	TEST_ASSERT_EQUAL_INT(200, Geometry_height(g));

	OZObject_release((struct OZObject *)g);
}

void test_computed_values(void)
{
	struct Geometry *g = Geometry_alloc();
	OZ_SEND_init((struct OZObject *)g);

	Geometry_setWidth_height_(g, 5, 10);
	TEST_ASSERT_EQUAL_INT(50, Geometry_area(g));
	TEST_ASSERT_EQUAL_INT(30, Geometry_perimeter(g));

	OZObject_release((struct OZObject *)g);
}
