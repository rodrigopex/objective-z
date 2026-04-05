/*
 * Adapted from: clang/test/Rewriter/blockcast3.mm, blockstruct.m
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies non-capturing block lowers to function pointer.
 */
#import "OZTestBase.h"

@interface BlockObj : OZObject {
	int _result;
}
- (void)run;
- (int)result;
@end

@implementation BlockObj
- (void)run {
	int (^add)(int, int) = ^(int a, int b) {
		return a + b;
	};
	_result = add(30, 12);
}
- (int)result {
	return _result;
}
@end
