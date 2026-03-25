/* oz-pool: OZObject=1,OZQ31=4,OZArray=2 */
#import "OZFoundationBase.h"

@interface ArrayTest : OZObject
- (unsigned int)literalCount;
- (int)firstElement;
- (BOOL)outOfBoundsNil;
@end

@implementation ArrayTest
- (unsigned int)literalCount {
	OZArray *arr = @[@(1), @(2), @(3)];
	unsigned int c = [arr count];
	return c;
}
- (int)firstElement {
	OZArray *arr = @[@(42), @(7)];
	OZQ31 *n = [arr objectAtIndex:0];
	int v = [n intValue];
	return v;
}
- (BOOL)outOfBoundsNil {
	OZArray *arr = @[@(1)];
	id obj = [arr objectAtIndex:99];
	return obj == nil;
}
@end
