/*
 * Adapted from: tests/objc-reference/runtime/message_dispatch/src/main.c
 * Verifies method dispatch resolves correctly through hierarchy.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "Animal_ozh.h"
#include "Dog_ozh.h"
#include "Cat_ozh.h"

void test_parent_dispatch(void)
{
	struct Animal *a = Animal_alloc();
	TEST_ASSERT_EQUAL_INT(0, Animal_speak(a));
	OZObject_release((struct OZObject *)a);
}

void test_child_override_dispatch(void)
{
	struct Dog *d = Dog_alloc();
	TEST_ASSERT_EQUAL_INT(1, Dog_speak(d));
	OZObject_release((struct OZObject *)d);
}

void test_sibling_dispatch_independent(void)
{
	struct Cat *c = Cat_alloc();
	TEST_ASSERT_EQUAL_INT(2, Cat_speak(c));
	OZObject_release((struct OZObject *)c);
}
