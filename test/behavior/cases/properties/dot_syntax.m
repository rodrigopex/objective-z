#import "OZTestBase.h"

@interface Setting : OZObject {
	int _brightness;
}
- (void)setBrightness:(int)v;
- (int)brightness;
@end

@implementation Setting
- (void)setBrightness:(int)v { _brightness = v; }
- (int)brightness { return _brightness; }
@end
