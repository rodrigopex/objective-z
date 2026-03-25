/* oz-pool: OZObject=2,OZTimer=1,TimerTarget=1 */
#import "OZFoundationBase.h"
#import <Foundation/OZTimer.h>
#include <zephyr/kernel.h>
#import "OZTimer.m"

@interface TimerTarget : OZObject {
	int _fired;
}
- (int)fired;
- (void)markFired;
@end

@implementation TimerTarget

- (int)fired
{
	return _fired;
}

- (void)markFired
{
	_fired = 1;
}

@end
