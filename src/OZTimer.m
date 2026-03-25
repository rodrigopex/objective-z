/* Timer implementation wrapping Zephyr k_timer for OZ transpiler. */

#import <Foundation/OZTimer.h>
#include <zephyr/kernel.h>

@implementation OZTimer

- (instancetype)initWithUserData:(id)userData
                          expiry:(void (^)(struct k_timer *timer))expBlock
                            stop:(void (^)(struct k_timer *timer))stopBlock
{
	_userdata = userData;
	_expiryBlock = expBlock;
	_stopBlock = stopBlock;
	__oz_timer_setup(&_timer, (__bridge void *)expBlock,
			 (__bridge void *)stopBlock,
			 (__bridge void *)userData);
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
