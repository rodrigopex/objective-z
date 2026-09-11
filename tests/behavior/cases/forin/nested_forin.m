/* oz-pool: OZObject=1,OZNumber=6,OZArray=3,NestedIterTest=1 */
#import "OZFoundationBase.h"

@interface NestedIterTest : OZObject {
	int _total;
}
- (void)nestedIteration;
- (int)total;
@end

@implementation NestedIterTest
- (void)nestedIteration {
	OZArray *outer = @[@(1), @(2)];
	OZArray *inner = @[@(10), @(20)];
	_total = 0;
	for (OZNumber *a in outer) {
		for (OZNumber *b in inner) {
			_total = _total + [a intValue] + [b intValue];
		}
	}
}
- (int)total {
	return _total;
}
@end
