/*
 * Adapted from: tests/objc-reference/runtime/categories/src/main.c
 * Verifies category methods are merged and callable.
 */
#include "unity.h"
#include "Calculator_ozh.h"

void test_category_add_method(void)
{
	struct Calculator *c = Calculator_alloc();
	Calculator_setValue_(c, 10);
	Calculator_add_(c, 5);
	TEST_ASSERT_EQUAL_INT(15, Calculator_value(c));
	OZObject_release((struct OZObject *)c);
}

void test_category_multiply_method(void)
{
	struct Calculator *c = Calculator_alloc();
	Calculator_setValue_(c, 3);
	Calculator_multiply_(c, 7);
	TEST_ASSERT_EQUAL_INT(21, Calculator_value(c));
	OZObject_release((struct OZObject *)c);
}

void test_base_and_category_coexist(void)
{
	struct Calculator *c = Calculator_alloc();
	Calculator_setValue_(c, 5);
	Calculator_add_(c, 10);
	Calculator_multiply_(c, 2);
	TEST_ASSERT_EQUAL_INT(30, Calculator_value(c));
	OZObject_release((struct OZObject *)c);
}
