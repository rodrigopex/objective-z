/* oz-pool: OZObject=1,OZFixedPoint=16 */
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
	OZFixedPoint *n = @42;
	int v = [n intValue];
	[n release];
	return v;
}

- (float)floatFromLiteral {
	OZFixedPoint *n = @(3.5f);
	float v = [n floatValue];
	[n release];
	return v;
}

- (int)intFromExpr {
	int x = 7;
	OZFixedPoint *n = @(x + 3);
	int v = [n int32Value];
	[n release];
	return v;
}

- (int)int8Roundtrip {
	OZFixedPoint *n = @(100);
	int v = [n int8Value];
	[n release];
	return v;
}

- (int)uint16Roundtrip {
	OZFixedPoint *n = @(1000);
	int v = [n uint16Value];
	[n release];
	return v;
}

- (int)boolTrue {
	OZFixedPoint *n = @(42);
	int v = [n boolValue];
	[n release];
	return v;
}

- (int)boolFalse {
	OZFixedPoint *n = @(0);
	int v = [n boolValue];
	[n release];
	return v;
}

- (int)rawNonZero {
	OZFixedPoint *n = @(5);
	int v = [n rawValue] != 0;
	[n release];
	return v;
}

- (int)shiftForTen {
	OZFixedPoint *n = @(10);
	int v = [n shift];
	[n release];
	return v;
}

- (int)addResult {
	OZFixedPoint *a = @(10);
	OZFixedPoint *b = @(20);
	OZFixedPoint *c = [a add:b];
	int v = [c int32Value];
	[a release];
	[b release];
	[c release];
	return v;
}

- (int)subResult {
	OZFixedPoint *a = @(50);
	OZFixedPoint *b = @(20);
	OZFixedPoint *c = [a sub:b];
	int v = [c int32Value];
	[a release];
	[b release];
	[c release];
	return v;
}

- (int)mulResult {
	OZFixedPoint *a = @(6);
	OZFixedPoint *b = @(7);
	OZFixedPoint *c = [a mul:b];
	int v = [c int32Value];
	[a release];
	[b release];
	[c release];
	return v;
}

- (float)divResult {
	OZFixedPoint *a = @(10);
	OZFixedPoint *b = @(4);
	OZFixedPoint *c = [a div:b];
	float v = [c floatValue];
	[a release];
	[b release];
	[c release];
	return v;
}

@end
