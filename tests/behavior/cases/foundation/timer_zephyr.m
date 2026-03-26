/* oz-pool: OZObject=2,OZTimer=1,TimerZephyrTest=1,TimerZephyrTarget=1 */
/* Minimal OZTimer test for Zephyr integration (OZ-078 regression).
 * Uses OZTestBase.h instead of OZFoundationBase.h to avoid pulling
 * Foundation classes that need malloc (OZMutableString). */
#import "OZTestBase.h"
#import <Foundation/OZTimer.h>
#include <zephyr/kernel.h>
#import "OZTimer.m"

@interface TimerZephyrTarget : OZObject {
	int _value;
}
- (instancetype)initWithValue:(int)v;
- (int)value;
- (void)increment;
@end

@implementation TimerZephyrTarget

- (instancetype)initWithValue:(int)v
{
	_value = v;
	return self;
}

- (int)value
{
	return _value;
}

- (void)increment
{
	_value = _value + 1;
}

@end

@interface TimerZephyrTest : OZObject {
	OZTimer *_timer;
	TimerZephyrTarget *_target;
}
- (instancetype)initWithTarget:(TimerZephyrTarget *)target;
- (OZTimer *)timer;
- (TimerZephyrTarget *)target;
@end

@implementation TimerZephyrTest

- (instancetype)initWithTarget:(TimerZephyrTarget *)target
{
	_target = target;
	_timer = [[OZTimer alloc]
		initWithUserData:target
			  expiry:^(struct k_timer *t) {
				  TimerZephyrTarget *tgt =
					  (__bridge TimerZephyrTarget *)k_timer_user_data_get(t);
				  [tgt increment];
			  }
			    stop:nil];
	return self;
}

- (OZTimer *)timer
{
	return _timer;
}

- (TimerZephyrTarget *)target
{
	return _target;
}

@end
