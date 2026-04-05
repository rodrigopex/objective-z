/* Behavior test: return in nested scope releases all locals without crash */
#include "unity.h"
#include "ArcReturnTest_ozh.h"

void test_return_in_nested_scope(void)
{
	struct ArcReturnTest *t = ArcReturnTest_alloc();
	int ret = ArcReturnTest_earlyReturnTest(t);
	/* Early return at i=1 must release the Inner local and not crash */
	TEST_ASSERT_EQUAL_INT(42, ret);
	OZObject_release((struct OZObject *)t);
}
