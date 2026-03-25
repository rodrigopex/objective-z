/* Behavior test: OZTimer lifecycle, userdata, and __bridge callback */
#include "unity.h"
#include "oz_dispatch.h"
#include "OZTimer_ozh.h"
#include "TimerTarget_ozh.h"

static int g_expiry_called = 0;
static int g_target_fired = 0;

static void test_expiry_fn(struct k_timer *timer)
{
	struct TimerTarget *target =
		(struct TimerTarget *)k_timer_user_data_get(timer);
	TimerTarget_markFired(target);
	g_target_fired = TimerTarget_fired(target);
	g_expiry_called = 1;
}

void test_timer_alloc_and_userdata(void)
{
	struct TimerTarget *target = TimerTarget_alloc();
	TEST_ASSERT_NOT_NULL(target);
	target = (struct TimerTarget *)OZObject_init((struct OZObject *)target);

	struct OZTimer *timer = OZTimer_alloc();
	TEST_ASSERT_NOT_NULL(timer);
	timer = OZTimer_initWithUserData_expiry_stop_(
		timer, (struct OZObject *)target, test_expiry_fn,
		(void *)0);
	TEST_ASSERT_NOT_NULL(timer);

	/* userdata accessor returns the target */
	struct OZObject *ud = OZTimer_userdata(timer);
	TEST_ASSERT_EQUAL_PTR(target, ud);

	/* k_timer_user_data_get returns the same pointer */
	void *raw = k_timer_user_data_get(&timer->_timer);
	TEST_ASSERT_EQUAL_PTR(target, raw);

	/* Manually fire the expiry callback (host stub doesn't fire timers) */
	g_expiry_called = 0;
	g_target_fired = 0;
	timer->_timer.expiry_fn(&timer->_timer);
	TEST_ASSERT_EQUAL_INT(1, g_expiry_called);
	TEST_ASSERT_EQUAL_INT(1, g_target_fired);

	/* Release timer first (strong ref keeps target alive) */
	OZObject_release((struct OZObject *)timer);
	/* Now release target */
	OZObject_release((struct OZObject *)target);
}

void test_timer_start_stop(void)
{
	struct OZTimer *timer = OZTimer_alloc();
	TEST_ASSERT_NOT_NULL(timer);
	timer = OZTimer_initWithUserData_expiry_stop_(
		timer, (struct OZObject *)0, test_expiry_fn,
		(void *)0);

	/* start/stop should not crash (no-op on host stub) */
	OZTimer_startAfter_period_(timer, 100, 500);
	OZTimer_stop(timer);

	OZObject_release((struct OZObject *)timer);
}
