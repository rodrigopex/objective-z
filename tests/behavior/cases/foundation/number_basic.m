/* oz-pool: OZObject=1,OZQ31=4 */
#import "OZFoundationBase.h"

@interface NumTest : OZObject
- (int)boxed;
@end

@implementation NumTest
- (int)boxed {
	OZQ31 *n = @(42);
	int v = [n intValue];
	return v;
}
@end
