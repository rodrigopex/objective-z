/*
 * Adapted from: clang/test/Rewriter/objc-modern-fast-enumeration.mm
 * Verifies for-in lowering to IteratorProtocol produces correct iteration.
 */
#include "unity.h"
#include "oz_dispatch.h"
#include "ForInObj_ozh.h"

void test_forin_rewrite_iterates(void)
{
	struct ForInObj *f = ForInObj_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)f);
	ForInObj_sumArray(f);
	TEST_ASSERT_EQUAL_INT(6, ForInObj_sum(f));
	OZObject_release((struct OZObject *)f);
}
