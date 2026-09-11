/*
 * Adapted from: clang/test/Rewriter/objc-modern-boxing.mm,
 *               objc-modern-numeric-literal.mm
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies @42, @(expr) lower to OZNumber factory calls.
 */
#import "OZTestBase.h"
#import <Foundation/OZNumber.h>

@interface BoxingObj : OZObject {
	OZNumber *_literal;
	OZNumber *_expr;
}
- (void)run;
- (OZNumber *)literal;
- (OZNumber *)expr;
@end

@implementation BoxingObj
- (void)run {
	_literal = @(42);
	int x = 10;
	_expr = @(x + 5);
}
- (OZNumber *)literal { return _literal; }
- (OZNumber *)expr { return _expr; }
@end
