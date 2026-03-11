/*
 * Adapted from: clang/test/Rewriter protocol handling
 * License: Apache 2.0 with LLVM Exception
 * Verifies protocol dispatch generates correct switch-based routing.
 */
#include "unity.h"
#include "Circle_ozh.h"
#include "Square_ozh.h"
#include "oz_dispatch.h"
#include "oz_mem_slabs.h"

void test_protocol_dispatch_circle(void)
{
	struct Circle *c = Circle_alloc();
	OZ_SEND_init((struct OZObject *)c);

	int result = OZ_SEND_draw((struct OZObject *)c);
	TEST_ASSERT_EQUAL_INT(0, result);

	OZObject_release((struct OZObject *)c);
}

void test_protocol_dispatch_square(void)
{
	struct Square *s = Square_alloc();
	OZ_SEND_init((struct OZObject *)s);
	Square_setSide_(s, 5);

	int result = OZ_SEND_draw((struct OZObject *)s);
	TEST_ASSERT_EQUAL_INT(20, result);

	OZObject_release((struct OZObject *)s);
}
