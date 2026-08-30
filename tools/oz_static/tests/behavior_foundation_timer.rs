// SPDX-License-Identifier: Apache-2.0
//
// behavior_foundation_timer.rs - OZTimer, the `k_timer` wrapper, ported
// from tests/behavior/cases/foundation/timer_basic.m and its
// timer_basic_test.c driver.
//
// OZTimer itself is transplanted from the real `src/OZTimer.m` (see
// `common::oztimer_src`, which documents the one rewrite it needs and
// why that rewrite is an oz_static bug rather than a property of the
// class).
//
// There is no Zephyr on host, so `#include <zephyr/kernel.h>` resolves
// to `tests/behavior/include/zephyr_stubs/zephyr/kernel.h` via
// `compile_and_run_with_zephyr_stubs` -- the same stub the oracle's own
// host-side timer tests use. Its `k_timer_start`/`k_timer_stop` are
// no-ops and nothing schedules anything, so expiry is driven the way the
// oracle's driver drives it: by calling `expiry_fn` on the embedded timer
// directly. What that tests is the wiring -- that the block reached the
// timer as a function pointer and that userdata round-trips -- not
// Zephyr's scheduling.
//
// The oracle's second case, timer_zephyr.m, is not ported: it is a
// twister/ztest case that needs the real kernel.

mod common;
use common::{compile_and_run_with_zephyr_stubs, oztimer_src, ozobject_src as PREAMBLE};

/// Ported from timer_basic_test.c's `test_timer_init_and_userdata` plus
/// `test_timer_expiry_fires_block`: the expiry block recovers its target
/// from timer userdata and mutates it, exactly the shape the oracle's
/// case uses, `__bridge` cast included -- `emit::render_cast_expression`
/// drops the qualifier, so `common::oztimer_src` needs no rewrite.
#[test]
fn timer_userdata_round_trips_and_expiry_block_fires() {
    let src = format!(
        "{}{}\n\
#include <stdio.h>

@interface TimerTarget : OZObject {{
	int _value;
}}
- (instancetype)initWithValue:(int)v;
- (int)value;
- (void)increment;
@end

@implementation TimerTarget
- (instancetype)initWithValue:(int)v {{
	_value = v;
	return self;
}}
- (int)value {{
	return _value;
}}
- (void)increment {{
	_value = _value + 1;
}}
@end

/* Bare `TimerTarget`, not `struct TimerTarget`: oz_static rewrites a
 * known class name into `struct <name>` wherever it appears, so writing
 * the tag here would emit `struct struct TimerTarget`. */
static void expiry_fn(struct k_timer *t) {{
	TimerTarget *tgt = (TimerTarget *)k_timer_user_data_get(t);
	[tgt increment];
}}

int main(void) {{
	TimerTarget *tgt = [TimerTarget alloc];
	[tgt initWithValue:10];

	OZTimer *timer = [OZTimer alloc];
	[timer initWithUserData:tgt expiry:expiry_fn stop:0];

	printf(\"userdata_is_target=%d\\n\", [timer userdata] == (void *)tgt);
	printf(\"value_before=%d\\n\", [tgt value]);

	/* Fire expiry by hand -- k_timer_start is a no-op on host, so
	 * nothing schedules this. Same approach as the oracle's driver. */
	timer->_timer.expiry_fn(&timer->_timer);
	printf(\"value_after_one=%d\\n\", [tgt value]);
	timer->_timer.expiry_fn(&timer->_timer);
	printf(\"value_after_two=%d\\n\", [tgt value]);

	[timer release];
	[tgt release];
	return 0;
}}
",
        PREAMBLE(),
        oztimer_src()
    );
    let stdout =
        compile_and_run_with_zephyr_stubs(&src, "timer_userdata_round_trips_and_expiry_fires");
    assert_eq!(
        stdout,
        "userdata_is_target=1\nvalue_before=10\nvalue_after_one=11\nvalue_after_two=12\n"
    );
}

/// Ported from timer_basic_test.c's `test_timer_start_stop_no_crash`,
/// and covers `-dealloc` reaching the embedded `struct k_timer` ivar.
#[test]
fn timer_start_stop_and_dealloc() {
    let src = format!(
        "{}{}\n\
#include <stdio.h>

int main(void) {{
	OZTimer *timer = [OZTimer alloc];
	[timer initWithUserData:0 expiry:0 stop:0];
	[timer startAfter:100 period:500];
	[timer stop];
	printf(\"start_stop_ok\\n\");
	printf(\"userdata_is_null=%d\\n\", [timer userdata] == 0);
	[timer release];
	printf(\"released_ok\\n\");
	return 0;
}}
",
        PREAMBLE(),
        oztimer_src()
    );
    let stdout = compile_and_run_with_zephyr_stubs(&src, "timer_start_stop_and_dealloc");
    assert_eq!(stdout, "start_stop_ok\nuserdata_is_null=1\nreleased_ok\n");
}

/// A non-capturing block literal passed as the expiry argument -- the
/// shape the oracle's case actually writes (`expiry:^(struct k_timer *t)
/// {...}`), hoisted to a static C function by `emit::render_block`.
#[test]
fn timer_accepts_block_literal_for_expiry() {
    let src = format!(
        "{}{}\n\
#include <stdio.h>

static int g_fired = 0;

int main(void) {{
	OZTimer *timer = [OZTimer alloc];
	[timer initWithUserData:0
	                 expiry:^(struct k_timer *t) {{
		                 g_fired = g_fired + 1;
	                 }}
	                   stop:0];
	printf(\"fired_before=%d\\n\", g_fired);
	timer->_timer.expiry_fn(&timer->_timer);
	printf(\"fired_after=%d\\n\", g_fired);
	[timer release];
	return 0;
}}
",
        PREAMBLE(),
        oztimer_src()
    );
    let stdout = compile_and_run_with_zephyr_stubs(&src, "timer_accepts_block_literal_for_expiry");
    assert_eq!(stdout, "fired_before=0\nfired_after=1\n");
}
