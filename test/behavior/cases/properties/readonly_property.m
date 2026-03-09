#import "OZTestBase.h"

@interface Stamp : OZObject {
	int _serial;
}
- (instancetype)init;
- (int)serial;
@end

@implementation Stamp
- (instancetype)init
{
	self = [super init];
	_serial = 999;
	return self;
}
- (int)serial { return _serial; }
@end
