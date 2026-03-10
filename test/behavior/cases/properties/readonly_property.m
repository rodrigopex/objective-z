#import "OZTestBase.h"

@interface Stamp : OZObject
@property(nonatomic, assign, readonly) int serial;
@end

@implementation Stamp
@synthesize serial = _serial;
- (instancetype)init
{
	self = [super init];
	_serial = 999;
	return self;
}
@end
