/*
 * Adapted from: GNUstep libobjc2 — Test/CategoryTest.m
 * License: MIT
 * Verifies category methods are merged into the class and callable.
 */
#include "unity.h"
#include "Printer.h"
#include "oz_mem_slabs.h"

void test_category_method_callable(void)
{
	struct Printer *p = Printer_alloc();
	OZ_SEND_init((struct OZObject *)p);

	Printer_addPages_(p, 10);
	TEST_ASSERT_EQUAL_INT(10, Printer_pages(p));
	TEST_ASSERT_EQUAL_INT(20, Printer_doublePages(p));

	OZObject_release((struct OZObject *)p);
}

void test_category_does_not_break_original(void)
{
	struct Printer *p = Printer_alloc();
	OZ_SEND_init((struct OZObject *)p);

	TEST_ASSERT_EQUAL_INT(0, Printer_pages(p));
	Printer_addPages_(p, 5);
	TEST_ASSERT_EQUAL_INT(5, Printer_pages(p));

	OZObject_release((struct OZObject *)p);
}
