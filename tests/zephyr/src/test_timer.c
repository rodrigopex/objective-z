/* SPDX-License-Identifier: Apache-2.0 */
/* OZTimer regression tests for OZ-078: verify OZTimer is in the CMake
 * transpile pipeline and __oz_timer_setup is available in the Zephyr PAL. */
#include <zephyr/ztest.h>
#include "OZTimer_ozh.h"
#include "TimerZephyrTarget_ozh.h"
#include "TimerZephyrTest_ozh.h"
#include "OZObject_ozh.h"
#include "oz_dispatch.h"

ZTEST_SUITE(timer, NULL, NULL, NULL, NULL, NULL);

ZTEST(timer, test_timer_alloc)
{
	struct OZTimer *t = OZTimer_alloc();
	zassert_not_null(t, "OZTimer alloc returned NULL");
	OZObject_release((struct OZObject *)t);
}

ZTEST(timer, test_timer_init_sets_userdata)
{
	struct TimerZephyrTarget *tgt = TimerZephyrTarget_initWithValue_(
		TimerZephyrTarget_alloc(), 42);
	zassert_not_null(tgt, "TimerZephyrTarget alloc failed");

	struct TimerZephyrTest *tt = TimerZephyrTest_initWithTarget_(
		TimerZephyrTest_alloc(), tgt);
	zassert_not_null(tt, "TimerZephyrTest alloc failed");

	struct OZTimer *timer = TimerZephyrTest_timer(tt);
	zassert_not_null(timer, "timer getter returned NULL");

	struct OZObject *ud = OZTimer_userdata(timer);
	zassert_equal_ptr(ud, (struct OZObject *)tgt,
			  "userdata should point to TimerZephyrTarget");

	OZObject_release((struct OZObject *)tt);
	OZObject_release((struct OZObject *)tgt);
}

ZTEST(timer, test_timer_expiry_fires_block)
{
	struct TimerZephyrTarget *tgt = TimerZephyrTarget_initWithValue_(
		TimerZephyrTarget_alloc(), 10);
	struct TimerZephyrTest *tt = TimerZephyrTest_initWithTarget_(
		TimerZephyrTest_alloc(), tgt);
	struct OZTimer *timer = TimerZephyrTest_timer(tt);

	/* Simulate k_timer expiry by calling the callback directly */
	timer->_timer.expiry_fn(&timer->_timer);

	zassert_equal(11, TimerZephyrTarget_value(tgt),
		      "Expected value=11 after expiry callback");

	OZObject_release((struct OZObject *)tt);
	OZObject_release((struct OZObject *)tgt);
}
