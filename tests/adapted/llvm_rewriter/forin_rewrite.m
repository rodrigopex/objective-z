/*
 * Adapted from: clang/test/Rewriter/objc-modern-fast-enumeration.mm
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies for-in lowers to OZIteratorProtocol loop.
 */
/* oz-pool: OZObject=1,OZNumber=3,OZArray=1,ForInObj=1 */
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
	for (OZNumber *n in arr) {
		_sum = _sum + [n intValue];
	}
}
- (int)sum {
	return _sum;
}
@end
