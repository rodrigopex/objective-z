/**
 * @file OZTimer.h
 * @brief Timer class wrapping Zephyr k_timer for OZ transpiler.
 *
 * OZTimer holds a k_timer, expiry/stop blocks, and a strong userdata
 * reference. The blocks receive the raw k_timer pointer so the
 * callback can recover userdata via k_timer_user_data_get() with
 * a __bridge cast.
 */
#pragma once
#import "OZObject.h"
#include <zephyr/kernel.h>

@interface OZTimer : OZObject {
	struct k_timer _timer;
	void (^_expiryBlock)(struct k_timer *);
	void (^_stopBlock)(struct k_timer *);
	id _userdata;
}
- (instancetype)initWithUserData:(id)userData
                          expiry:(void (^)(struct k_timer *timer))expBlock
                            stop:(void (^)(struct k_timer *timer))stopBlock;
- (void)startAfter:(uint32_t)delayMs period:(uint32_t)periodMs;
- (void)stop;
- (id)userdata;
- (void)dealloc;
@end
