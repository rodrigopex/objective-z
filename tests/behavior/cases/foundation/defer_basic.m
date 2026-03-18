/* oz-pool: OZObject=1,OZDefer=2,DeferTest=1 */
#import "OZFoundationBase.h"

@interface DeferTest : OZObject {
	OZDefer *_cleanup;
	int _marker;
}
- (instancetype)initWithCleanup;
- (int)marker;
@end

@implementation DeferTest

- (instancetype)initWithCleanup
{
	_marker = 99;
	_cleanup = [[OZDefer alloc] initWithOwner:self block:^(id owner) {
		/* Block fires during dealloc — lifecycle validated by no crash */
	}];
	return self;
}

- (int)marker
{
	return _marker;
}

@end
