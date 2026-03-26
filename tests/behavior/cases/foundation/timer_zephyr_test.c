/* Behavior test: OZTimer zephyr regression (OZ-078) */
#include "unity.h"
#include "oz_dispatch.h"
#include "OZTimer_ozh.h"
#include "TimerZephyrTarget_ozh.h"
#include "TimerZephyrTest_ozh.h"

void test_timer_zephyr_alloc(void)
{
	struct OZTimer *t = OZTimer_alloc();
	TEST_ASSERT_NOT_NULL(t);
	OZObject_release((struct OZObject *)t);
}

void test_timer_zephyr_init_and_userdata(void)
{
	struct TimerZephyrTarget *tgt = TimerZephyrTarget_initWithValue_(
		TimerZephyrTarget_alloc(), 42);
	TEST_ASSERT_NOT_NULL(tgt);

	struct TimerZephyrTest *tt = TimerZephyrTest_initWithTarget_(
		TimerZephyrTest_alloc(), tgt);
	TEST_ASSERT_NOT_NULL(tt);

	struct OZTimer *timer = TimerZephyrTest_timer(tt);
	TEST_ASSERT_NOT_NULL(timer);
	struct OZObject *ud = OZTimer_userdata(timer);
	TEST_ASSERT_EQUAL_PTR(tgt, ud);

	OZObject_release((struct OZObject *)tt);
	OZObject_release((struct OZObject *)tgt);
}

void test_timer_zephyr_expiry_fires_block(void)
{
	struct TimerZephyrTarget *tgt = TimerZephyrTarget_initWithValue_(
		TimerZephyrTarget_alloc(), 10);
	struct TimerZephyrTest *tt = TimerZephyrTest_initWithTarget_(
		TimerZephyrTest_alloc(), tgt);
	struct OZTimer *timer = TimerZephyrTest_timer(tt);

	/* Simulate k_timer expiry by calling the callback directly */
	timer->_timer.expiry_fn(&timer->_timer);

	TEST_ASSERT_EQUAL_INT(11, TimerZephyrTarget_value(tgt));

	OZObject_release((struct OZObject *)tt);
	OZObject_release((struct OZObject *)tgt);
}
