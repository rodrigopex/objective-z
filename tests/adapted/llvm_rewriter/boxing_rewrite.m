/*
 * Adapted from: clang/test/Rewriter/objc-modern-boxing.mm,
 *               objc-modern-numeric-literal.mm
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies @42, @(expr) lower to OZQ31 factory calls.
 */
#import "OZTestBase.h"
#import <Foundation/OZQ31.h>

@interface BoxingObj : OZObject {
	OZQ31 *_literal;
	OZQ31 *_expr;
}
- (void)run;
- (OZQ31 *)literal;
- (OZQ31 *)expr;
@end

@implementation BoxingObj
- (void)run {
	_literal = @(42);
	int x = 10;
	_expr = @(x + 5);
}
- (OZQ31 *)literal { return _literal; }
- (OZQ31 *)expr { return _expr; }
@end
