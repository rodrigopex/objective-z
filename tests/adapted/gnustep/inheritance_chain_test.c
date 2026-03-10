/*
 * Adapted from: GNUstep libobjc2 — Test/InheritanceTest.m
 * License: MIT
 * Verifies method resolution through a 3-level hierarchy.
 */
#include "unity.h"
#include "Animal.h"
#include "Dog.h"
#include "Puppy.h"
#include "oz_dispatch.h"
#include "oz_mem_slabs.h"

void test_base_class_method(void)
{
	struct Animal *a = Animal_alloc();
	OZ_SEND_init((struct OZObject *)a);

	TEST_ASSERT_EQUAL_INT(0, OZ_SEND_sound((struct OZObject *)a));

	OZObject_release((struct OZObject *)a);
}

void test_override_in_child(void)
{
	struct Dog *d = Dog_alloc();
	OZ_SEND_init((struct OZObject *)d);

	TEST_ASSERT_EQUAL_INT(1, OZ_SEND_sound((struct OZObject *)d));

	OZObject_release((struct OZObject *)d);
}

void test_override_in_grandchild(void)
{
	struct Puppy *p = Puppy_alloc();
	OZ_SEND_init((struct OZObject *)p);

	TEST_ASSERT_EQUAL_INT(2, OZ_SEND_sound((struct OZObject *)p));

	OZObject_release((struct OZObject *)p);
}

void test_inherited_method_from_base(void)
{
	struct Dog *d = Dog_alloc();
	OZ_SEND_init((struct OZObject *)d);

	TEST_ASSERT_EQUAL_INT(0, Animal_legs((struct Animal *)d));

	OZObject_release((struct OZObject *)d);
}
