/* oz-pool: OZObject=1,OZDefer=1,BlockIvarTest=1 */
#import "OZFoundationBase.h"

@interface BlockIvarTest : OZObject {
	OZDefer *_defer;
}
- (instancetype)initWithDefer:(OZDefer *)d;
- (OZDefer *)defer;
@end

@implementation BlockIvarTest

- (instancetype)initWithDefer:(OZDefer *)d
{
	_defer = d;
	return self;
}

- (OZDefer *)defer
{
	return _defer;
}

@end
