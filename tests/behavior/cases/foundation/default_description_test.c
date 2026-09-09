/* Behavior test: the inherited `-cDescription:maxLength:` default (#354).
 *
 * See the `.m` for why this sends the selector directly rather than going
 * through `OZLog`, and why the address is never asserted.
 */
#include "unity.h"
#include "Plain_ozh.h"

int run_default_names_the_class(void);
int run_own_description_wins(void);
int run_bounded(void);

void test_inherited_description_names_the_class_and_address(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_default_names_the_class());
}

void test_a_classs_own_description_still_wins(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_own_description_wins());
}

void test_the_default_never_writes_past_max_length(void)
{
	TEST_ASSERT_EQUAL_INT(1, run_bounded());
}
