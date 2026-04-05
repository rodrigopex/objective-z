/*
 * Adapted from: tests/objc-reference/runtime/arc_intensive/src/main.c
 * Verifies nested object graph with multiple ivars works correctly.
 */
#include "unity.h"
#include "NestedArcTest_ozh.h"

void test_nested_arc_object_graph(void)
{
	struct NestedArcTest *t = NestedArcTest_alloc();
	NestedArcTest_run(t);
	TEST_ASSERT_EQUAL_INT(10, NestedArcTest_leftVal(t));
	TEST_ASSERT_EQUAL_INT(20, NestedArcTest_rightVal(t));
	OZObject_release((struct OZObject *)t);
}
