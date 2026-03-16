/* oz-pool: OZObject=1,OZNumber=4 */
#import "OZFoundationBase.h"

@interface NumTest : OZObject
- (int)boxed;
@end

@implementation NumTest
- (int)boxed {
	OZNumber *n = @(42);
	int v = [n intValue];
	[n release];
	return v;
}
@end
