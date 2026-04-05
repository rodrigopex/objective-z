/*
 * Adapted from: clang/test/Rewriter/func-in-impl.m
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies C functions inside @implementation preserved.
 */
#import "OZTestBase.h"

@interface FuncInImpl : OZObject {
	int _val;
}
- (void)run;
- (int)val;
@end

static int helperFunc(int x)
{
	return x * x;
}

@implementation FuncInImpl
- (void)run {
	_val = helperFunc(6);
}
- (int)val {
	return _val;
}
@end
