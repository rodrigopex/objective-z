/*
 * Adapted from: tests/objc-reference/runtime/flat_dispatch/src/main.c
 * Verifies multi-level hierarchy dispatch resolves at each level.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "Base_ozh.h"
#include "Mid_ozh.h"
#include "Tip_ozh.h"

void test_base_level(void)
{
	struct Base *b = Base_alloc();
	TEST_ASSERT_EQUAL_INT(1, Base_level(b));
	OZObject_release((struct OZObject *)b);
}

void test_mid_level_override(void)
{
	struct Mid *m = Mid_alloc();
	TEST_ASSERT_EQUAL_INT(2, Mid_level(m));
	OZObject_release((struct OZObject *)m);
}

void test_tip_level_override(void)
{
	struct Tip *t = Tip_alloc();
	TEST_ASSERT_EQUAL_INT(3, Tip_level(t));
	OZObject_release((struct OZObject *)t);
}
