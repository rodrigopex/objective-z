/* Behavior test: method with 3 arguments */
#include "unity.h"
#include "Calc_ozh.h"
#include "oz_mem_slabs.h"

void test_three_args_sum(void)
{
	struct Calc *m = Calc_alloc();
	int result = Calc_addA_b_c_(m, 10, 20, 30);
	TEST_ASSERT_EQUAL_INT(60, result);
	OZObject_release((struct OZObject *)m);
}

void test_three_args_negative(void)
{
	struct Calc *m = Calc_alloc();
	int result = Calc_addA_b_c_(m, -5, 3, 2);
	TEST_ASSERT_EQUAL_INT(0, result);
	OZObject_release((struct OZObject *)m);
}
