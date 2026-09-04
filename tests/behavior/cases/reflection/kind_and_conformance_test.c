/* Behavior test: -isKindOfClass: and -conformsToProtocol: (#226) */
#include "unity.h"
#include "Kinds_ozh.h"

void test_kind_and_conformance(void)
{
	/* Eight checks: 255 = 0b11111111. */
	TEST_ASSERT_EQUAL_INT(255, Kinds_cls_check());
}
