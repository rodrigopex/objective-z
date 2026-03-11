/* Behavior test: class method dispatch */
#include "unity.h"
#include "Factory_ozh.h"
#include "oz_mem_slabs.h"

void test_class_method_returns_value(void)
{
	TEST_ASSERT_EQUAL_INT(42, Factory_cls_version());
}
