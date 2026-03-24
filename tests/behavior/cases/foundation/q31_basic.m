/* oz-pool: OZObject=1,OZQ31=16 */
#import "OZFoundationBase.h"

@interface FPTest : OZObject
/* Q31 encoding / value extraction roundtrip */
- (int)intFromLiteral;
- (float)floatFromLiteral;
- (int)intFromExpr;
- (int)int8Roundtrip;
- (int)uint16Roundtrip;
- (int)boolTrue;
- (int)boolFalse;

/* Q31 introspection */
- (int)rawNonZero;
- (int)shiftForTen;

/* Arithmetic */
- (int)addResult;
- (int)subResult;
- (int)mulResult;
- (float)divResult;
@end

@implementation FPTest

- (int)intFromLiteral {
	OZQ31 *n = @42;
	int v = [n intValue];
	[n release];
	return v;
}

- (float)floatFromLiteral {
	OZQ31 *n = @(3.5f);
	float v = [n floatValue];
	[n release];
	return v;
}

- (int)intFromExpr {
	int x = 7;
	OZQ31 *n = @(x + 3);
	int v = [n int32Value];
	[n release];
	return v;
}

- (int)int8Roundtrip {
	OZQ31 *n = @(100);
	int v = [n int8Value];
	[n release];
	return v;
}

- (int)uint16Roundtrip {
	OZQ31 *n = @(1000);
	int v = [n uint16Value];
	[n release];
	return v;
}

- (int)boolTrue {
	OZQ31 *n = @(42);
	int v = [n boolValue];
	[n release];
	return v;
}

- (int)boolFalse {
	OZQ31 *n = @(0);
	int v = [n boolValue];
	[n release];
	return v;
}

- (int)rawNonZero {
	OZQ31 *n = @(5);
	int v = [n rawValue] != 0;
	[n release];
	return v;
}

- (int)shiftForTen {
	OZQ31 *n = @(10);
	int v = [n shift];
	[n release];
	return v;
}

- (int)addResult {
	OZQ31 *a = @(10);
	OZQ31 *b = @(20);
	OZQ31 *c = [a add:b];
	int v = [c int32Value];
	[a release];
	[b release];
	[c release];
	return v;
}

- (int)subResult {
	OZQ31 *a = @(50);
	OZQ31 *b = @(20);
	OZQ31 *c = [a sub:b];
	int v = [c int32Value];
	[a release];
	[b release];
	[c release];
	return v;
}

- (int)mulResult {
	OZQ31 *a = @(6);
	OZQ31 *b = @(7);
	OZQ31 *c = [a mul:b];
	int v = [c int32Value];
	[a release];
	[b release];
	[c release];
	return v;
}

- (float)divResult {
	OZQ31 *a = @(10);
	OZQ31 *b = @(4);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	[a release];
	[b release];
	[c release];
	return v;
}

@end
