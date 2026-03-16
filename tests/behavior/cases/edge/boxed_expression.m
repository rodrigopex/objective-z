/* Boxed expression @(expr) test — exercises variable, arithmetic, and
 * function-call boxing through OZNumber_initXxx() helpers. */

#import "OZTestBase.h"
#import <Foundation/OZNumber.h>

static int triple(int x) { return x * 3; }

@interface BoxedTest : OZObject {
	OZNumber *_fromVar;
	OZNumber *_fromExpr;
	OZNumber *_fromCall;
	OZNumber *_fromFloat;
	OZNumber *_fromUint;
}
- (void)run;
- (OZNumber *)fromVar;
- (OZNumber *)fromExpr;
- (OZNumber *)fromCall;
- (OZNumber *)fromFloat;
- (OZNumber *)fromUint;
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
- (OZNumber *)fromVar { return _fromVar; }
- (OZNumber *)fromExpr { return _fromExpr; }
- (OZNumber *)fromCall { return _fromCall; }
- (OZNumber *)fromFloat { return _fromFloat; }
- (OZNumber *)fromUint { return _fromUint; }
@end
