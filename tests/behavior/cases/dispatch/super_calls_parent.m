#import "OZTestBase.h"

@interface Base : OZObject {
	int _baseVal;
}
- (instancetype)init;
- (int)baseVal;
@end

@implementation Base
- (instancetype)init
{
	self = [super init];
	_baseVal = 10;
	return self;
}
- (int)baseVal { return _baseVal; }
@end

@interface Child : Base {
	int _childVal;
}
- (instancetype)init;
- (int)childVal;
@end

@implementation Child
- (instancetype)init
{
	self = [super init];
	_childVal = 20;
	return self;
}
- (int)childVal { return _childVal; }
@end
