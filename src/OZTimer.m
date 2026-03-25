/* Timer implementation wrapping Zephyr k_timer for OZ transpiler. */

#import <Foundation/OZTimer.h>
#include <zephyr/kernel.h>

@implementation OZTimer

- (instancetype)initWithUserData:(id)userData
                          expiry:(void (^)(struct k_timer *timer))expBlock
                            stop:(void (^)(struct k_timer *timer))stopBlock
{
	_userdata = userData;
	k_timer_init(&_timer, (k_timer_expiry_t)expBlock,
		     (k_timer_stop_t)stopBlock);
	k_timer_user_data_set(&_timer, (__bridge void *)userData);
	return self;
}

- (void)startAfter:(uint32_t)delayMs period:(uint32_t)periodMs
{
	k_timer_start(&_timer, K_MSEC(delayMs), K_MSEC(periodMs));
}

- (void)stop
{
	k_timer_stop(&_timer);
}

- (id)userdata
{
	return _userdata;
}

- (void)dealloc
{
	k_timer_stop(&_timer);
	k_timer_user_data_set(&_timer, (void *)0);
}

@end
