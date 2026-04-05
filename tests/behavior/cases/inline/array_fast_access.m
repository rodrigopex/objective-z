/* oz-pool: OZObject=1,OZQ31=3,OZArray=1,ArrayAccessTest=1 */
#import "OZFoundationBase.h"

@interface ArrayAccessTest : OZObject {
	unsigned int _count;
	int _firstVal;
}
- (void)run;
- (unsigned int)count;
- (int)firstVal;
@end

@implementation ArrayAccessTest
- (void)run {
	OZArray *arr = @[@(100), @(200), @(300)];
	_count = [arr count];
	OZQ31 *first = [arr objectAtIndex:0];
	_firstVal = [first intValue];
}
- (unsigned int)count {
	return _count;
}
- (int)firstVal {
	return _firstVal;
}
@end
