/* Behavior test: class identity (#226) */
#include "unity.h"
#include "Identity_ozh.h"

void test_class_identity(void)
{
	/* All six checks, so a partial failure names which one: 63 = 0b111111. */
	TEST_ASSERT_EQUAL_INT(63, Identity_cls_check());
}
