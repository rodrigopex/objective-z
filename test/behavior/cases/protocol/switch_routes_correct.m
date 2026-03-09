#import "OZTestBase.h"

@protocol Togglable
- (int)toggle;
@end

@interface LightSwitch : OZObject <Togglable> {
	int _state;
}
@end

@implementation LightSwitch
- (int)toggle { _state = !_state; return _state; }
@end

@interface Fan : OZObject <Togglable> {
	int _running;
}
@end

@implementation Fan
- (int)toggle { _running = !_running; return _running + 10; }
@end
