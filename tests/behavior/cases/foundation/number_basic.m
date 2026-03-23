/* oz-pool: OZObject=1,OZFixedPoint=4 */
#import "OZFoundationBase.h"

@interface NumTest : OZObject
- (int)boxed;
@end

@implementation NumTest
- (int)boxed {
	OZFixedPoint *n = @(42);
	int v = [n intValue];
	[n release];
	return v;
}
@end
