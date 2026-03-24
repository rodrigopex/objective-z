/* Boxed expression @(expr) test — exercises variable, arithmetic, and
 * function-call boxing through OZQ31_fixedWith*() helpers. */

#import "OZTestBase.h"
#import <Foundation/OZQ31.h>

static int triple(int x) { return x * 3; }

@interface BoxedTest : OZObject {
	OZQ31 *_fromVar;
	OZQ31 *_fromExpr;
	OZQ31 *_fromCall;
	OZQ31 *_fromFloat;
	OZQ31 *_fromUint;
}
- (void)run;
- (OZQ31 *)fromVar;
- (OZQ31 *)fromExpr;
- (OZQ31 *)fromCall;
- (OZQ31 *)fromFloat;
- (OZQ31 *)fromUint;
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
- (OZQ31 *)fromVar { return _fromVar; }
- (OZQ31 *)fromExpr { return _fromExpr; }
- (OZQ31 *)fromCall { return _fromCall; }
- (OZQ31 *)fromFloat { return _fromFloat; }
- (OZQ31 *)fromUint { return _fromUint; }
@end
