/* Behavior test: child inherits parent's method without override */
#include "unity.h"
#include "Car_ozh.h"

void test_inherited_method_via_parent(void)
{
	struct Car *c = Car_alloc();
	OZ_SEND_init((struct OZObject *)c);

	/* Car has no speed method — should call Vehicle's via parent cast */
	TEST_ASSERT_EQUAL_INT(60, Vehicle_speed((struct Vehicle *)c));

	OZObject_release((struct OZObject *)c);
}
