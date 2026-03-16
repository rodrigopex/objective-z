/* PAL atomic operations unit tests */
#include "unity.h"
#include "platform/oz_platform.h"

void test_atomic_init_sets_value(void)
{
	oz_atomic_t val;
	oz_atomic_init(&val, 42);
	TEST_ASSERT_EQUAL_INT(42, oz_atomic_get(&val));
}

void test_atomic_inc_returns_new_value(void)
{
	oz_atomic_t val;
	oz_atomic_init(&val, 0);

	TEST_ASSERT_EQUAL_INT(1, oz_atomic_inc(&val));
	TEST_ASSERT_EQUAL_INT(2, oz_atomic_inc(&val));
	TEST_ASSERT_EQUAL_INT(3, oz_atomic_inc(&val));
}

void test_atomic_inc_updates_stored_value(void)
{
	oz_atomic_t val;
	oz_atomic_init(&val, 10);
	oz_atomic_inc(&val);
	TEST_ASSERT_EQUAL_INT(11, oz_atomic_get(&val));
}

void test_atomic_dec_and_test_false_when_nonzero(void)
{
	oz_atomic_t val;
	oz_atomic_init(&val, 3);

	TEST_ASSERT_FALSE(oz_atomic_dec_and_test(&val));
	TEST_ASSERT_EQUAL_INT(2, oz_atomic_get(&val));

	TEST_ASSERT_FALSE(oz_atomic_dec_and_test(&val));
	TEST_ASSERT_EQUAL_INT(1, oz_atomic_get(&val));
}

void test_atomic_dec_and_test_true_at_zero(void)
{
	oz_atomic_t val;
	oz_atomic_init(&val, 1);

	TEST_ASSERT_TRUE(oz_atomic_dec_and_test(&val));
	TEST_ASSERT_EQUAL_INT(0, oz_atomic_get(&val));
}

void test_atomic_inc_dec_roundtrip(void)
{
	oz_atomic_t val;
	oz_atomic_init(&val, 1);

	oz_atomic_inc(&val);
	oz_atomic_inc(&val);
	TEST_ASSERT_EQUAL_INT(3, oz_atomic_get(&val));

	oz_atomic_dec_and_test(&val);
	oz_atomic_dec_and_test(&val);
	TEST_ASSERT_EQUAL_INT(1, oz_atomic_get(&val));
}
