#import "OZTestBase.h"

@interface Vehicle : OZObject {
	int _speed;
}
- (instancetype)init;
- (int)speed;
@end

@implementation Vehicle
- (instancetype)init
{
	self = [super init];
	_speed = 60;
	return self;
}
- (int)speed { return _speed; }
@end

@interface Car : Vehicle
@end

@implementation Car
/* Does NOT override speed — inherits from Vehicle */
@end
