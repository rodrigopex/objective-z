#import "OZTestBase.h"

@interface Gadget : OZObject {
	int _value;
	int _ready;
}
- (instancetype)init;
- (int)value;
- (int)ready;
@end

@implementation Gadget
- (instancetype)init
{
	self = [super init];
	_value = 42;
	_ready = 1;
	return self;
}
- (int)value { return _value; }
- (int)ready { return _ready; }
@end
