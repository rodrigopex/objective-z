/* oz-pool: OZObject=1,OZNumber=4,OZArray=2 */
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
	[arr release];
	return c;
}
- (int)firstElement {
	OZArray *arr = @[@(42), @(7)];
	OZNumber *n = [arr objectAtIndex:0];
	int v = [n intValue];
	[arr release];
	return v;
}
- (BOOL)outOfBoundsNil {
	OZArray *arr = @[@(1)];
	id obj = [arr objectAtIndex:99];
	[arr release];
	return obj == nil;
}
@end
