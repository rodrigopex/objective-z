/* PAL spinlock unit tests (host backend: no-op locks) */
#include "unity.h"
#include "platform/oz_platform.h"

void test_spin_lock_unlock(void)
{
	oz_spinlock_t lock = 0;
	oz_spinlock_key_t key = oz_spin_lock(&lock);
	oz_spin_unlock(&lock, key);
	/* No crash = success on host (no-op backend) */
	TEST_PASS();
}

void test_spinlock_macro(void)
{
	oz_spinlock_t lock = 0;
	int entered = 0;
	OZ_SPINLOCK(lock) {
		entered = 1;
	}
	TEST_ASSERT_EQUAL_INT(1, entered);
}
