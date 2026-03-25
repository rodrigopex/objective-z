/* oz-pool: OZObject=2,OZTimer=1,TimerTest=1,TimerTarget=1 */
#import "OZFoundationBase.h"
#import <Foundation/OZTimer.h>
#include <zephyr/kernel.h>
#import "OZTimer.m"

@interface TimerTarget : OZObject {
	int _value;
}
- (instancetype)initWithValue:(int)v;
- (int)value;
- (void)increment;
@end

@implementation TimerTarget

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

@interface TimerTest : OZObject {
	OZTimer *_timer;
	TimerTarget *_target;
}
- (instancetype)initWithTarget:(TimerTarget *)target;
- (OZTimer *)timer;
- (TimerTarget *)target;
@end

@implementation TimerTest

- (instancetype)initWithTarget:(TimerTarget *)target
{
	_target = target;
	_timer = [[OZTimer alloc]
		initWithUserData:target
			  expiry:^(struct k_timer *t) {
				  TimerTarget *tgt =
					  (__bridge TimerTarget *)k_timer_user_data_get(t);
				  [tgt increment];
			  }
			    stop:nil];
	return self;
}

- (OZTimer *)timer
{
	return _timer;
}

- (TimerTarget *)target
{
	return _target;
}

@end
