/*
 * Adapted from: clang/test/Rewriter/objc-modern-fast-enumeration.mm
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies for-in lowers to IteratorProtocol loop.
 */
/* oz-pool: OZObject=1,OZQ31=3,OZArray=1,ForInObj=1 */
#import "OZFoundationBase.h"

@interface ForInObj : OZObject {
	int _sum;
}
- (void)sumArray;
- (int)sum;
@end

@implementation ForInObj
- (void)sumArray {
	OZArray *arr = @[@(1), @(2), @(3)];
	_sum = 0;
	for (OZQ31 *n in arr) {
		_sum = _sum + [n intValue];
	}
}
- (int)sum {
	return _sum;
}
@end
