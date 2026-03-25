/* Behavior test: OZTimer with __bridge cast in expiry block */
#include "unity.h"
#include "oz_dispatch.h"
#include "OZTimer_ozh.h"
#include "TimerTarget_ozh.h"
#include "TimerTest_ozh.h"

void test_timer_init_and_userdata(void)
{
	struct TimerTarget *tgt = TimerTarget_initWithValue_(
		TimerTarget_alloc(), 10);
	TEST_ASSERT_NOT_NULL(tgt);

	struct TimerTest *tt = TimerTest_initWithTarget_(
		TimerTest_alloc(), tgt);
	TEST_ASSERT_NOT_NULL(tt);

	/* timer userdata points to target */
	struct OZTimer *timer = TimerTest_timer(tt);
	TEST_ASSERT_NOT_NULL(timer);
	struct OZObject *ud = OZTimer_userdata(timer);
	TEST_ASSERT_EQUAL_PTR(tgt, ud);

	OZObject_release((struct OZObject *)tt);
	OZObject_release((struct OZObject *)tgt);
}

void test_timer_expiry_fires_block(void)
{
	struct TimerTarget *tgt = TimerTarget_initWithValue_(
		TimerTarget_alloc(), 42);
	struct TimerTest *tt = TimerTest_initWithTarget_(
		TimerTest_alloc(), tgt);
	struct OZTimer *timer = TimerTest_timer(tt);

	/* Manually fire expiry — simulates k_timer on host */
	timer->_timer.expiry_fn(&timer->_timer);

	/* Block called [tgt increment] via __bridge recovery */
	TEST_ASSERT_EQUAL_INT(43, TimerTarget_value(tgt));

	/* Fire again */
	timer->_timer.expiry_fn(&timer->_timer);
	TEST_ASSERT_EQUAL_INT(44, TimerTarget_value(tgt));

	OZObject_release((struct OZObject *)tt);
	OZObject_release((struct OZObject *)tgt);
}

void test_timer_start_stop_no_crash(void)
{
	struct TimerTarget *tgt = TimerTarget_initWithValue_(
		TimerTarget_alloc(), 0);
	struct TimerTest *tt = TimerTest_initWithTarget_(
		TimerTest_alloc(), tgt);
	struct OZTimer *timer = TimerTest_timer(tt);

	OZTimer_startAfter_period_(timer, 100, 500);
	OZTimer_stop(timer);

	OZObject_release((struct OZObject *)tt);
	OZObject_release((struct OZObject *)tgt);
}
