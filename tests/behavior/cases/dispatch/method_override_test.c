/* Behavior test: child overrides parent method */
#include "unity.h"
#include "Dog_ozh.h"
#include "oz_mem_slabs.h"

void test_child_calls_override(void)
{
	struct Dog *d = Dog_alloc();
	TEST_ASSERT_EQUAL_INT(2, Dog_sound(d));
	OZObject_release((struct OZObject *)d);
}

void test_parent_calls_original(void)
{
	struct Animal *a = Animal_alloc();
	TEST_ASSERT_EQUAL_INT(1, Animal_sound(a));
	OZObject_release((struct OZObject *)a);
}
