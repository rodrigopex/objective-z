/* Behavior test: @selector, SEL, -respondsToSelector:, -performSelector: (#226) */
#include "unity.h"
#include "Selectors_ozh.h"

void test_selector_send(void)
{
	/* Eight checks: 255 = 0b11111111. */
	TEST_ASSERT_EQUAL_INT(255, Selectors_cls_check());
}
