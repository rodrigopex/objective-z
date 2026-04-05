/* oz-pool: BlockParamTest=1 */
#import "OZTestBase.h"

@interface BlockParamTest : OZObject {
	int _computed;
}
- (void)applyBlock:(int (^)(int))blk toValue:(int)v;
- (int)computed;
@end

@implementation BlockParamTest
- (void)applyBlock:(int (^)(int))blk toValue:(int)v {
	_computed = blk(v);
}
- (int)computed {
	return _computed;
}
@end
