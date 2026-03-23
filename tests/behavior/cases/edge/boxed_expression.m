/* Boxed expression @(expr) test — exercises variable, arithmetic, and
 * function-call boxing through OZFixedPoint_fixedWith*() helpers. */

#import "OZTestBase.h"
#import <Foundation/OZFixedPoint.h>

static int triple(int x) { return x * 3; }

@interface BoxedTest : OZObject {
	OZFixedPoint *_fromVar;
	OZFixedPoint *_fromExpr;
	OZFixedPoint *_fromCall;
	OZFixedPoint *_fromFloat;
	OZFixedPoint *_fromUint;
}
- (void)run;
- (OZFixedPoint *)fromVar;
- (OZFixedPoint *)fromExpr;
- (OZFixedPoint *)fromCall;
- (OZFixedPoint *)fromFloat;
- (OZFixedPoint *)fromUint;
@end

@implementation BoxedTest
- (void)run
{
	int val = 7;
	_fromVar = @(val);
	_fromExpr = @(val + 3);
	_fromCall = @(triple(val));
	float f = 2.5f;
	_fromFloat = @(f);
	unsigned int u = 1000;
	_fromUint = @(u);
}
- (OZFixedPoint *)fromVar { return _fromVar; }
- (OZFixedPoint *)fromExpr { return _fromExpr; }
- (OZFixedPoint *)fromCall { return _fromCall; }
- (OZFixedPoint *)fromFloat { return _fromFloat; }
- (OZFixedPoint *)fromUint { return _fromUint; }
@end
