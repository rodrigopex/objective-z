/* oz-pool: OZObject=1,OZNumber=16 */
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
	OZNumber *n = @42;
	int v = [n intValue];
	return v;
}

- (float)floatFromLiteral {
	OZNumber *n = @(3.5f);
	float v = [n floatValue];
	return v;
}

- (int)intFromExpr {
	int x = 7;
	OZNumber *n = @(x + 3);
	int v = [n int32Value];
	return v;
}

- (int)int8Roundtrip {
	OZNumber *n = @(100);
	int v = [n int8Value];
	return v;
}

- (int)uint16Roundtrip {
	OZNumber *n = @(1000);
	int v = [n unsignedInt16Value];
	return v;
}

- (int)boolTrue {
	OZNumber *n = @(42);
	int v = [n boolValue];
	return v;
}

- (int)boolFalse {
	OZNumber *n = @(0);
	int v = [n boolValue];
	return v;
}

- (int)rawNonZero {
	OZNumber *n = @(5);
	int v = [n rawValue] != 0;
	return v;
}

- (int)shiftForTen {
	OZNumber *n = @(10);
	int v = [n shift];
	return v;
}

- (int)addResult {
	OZNumber *a = @(10);
	OZNumber *b = @(20);
	OZNumber *c = [a adding:b];
	int v = [c int32Value];
	return v;
}

- (int)subResult {
	OZNumber *a = @(50);
	OZNumber *b = @(20);
	OZNumber *c = [a subtracting:b];
	int v = [c int32Value];
	return v;
}

- (int)mulResult {
	OZNumber *a = @(6);
	OZNumber *b = @(7);
	OZNumber *c = [a multiplyingBy:b];
	int v = [c int32Value];
	return v;
}

- (float)divResult {
	OZNumber *a = @(10);
	OZNumber *b = @(4);
	OZNumber *c = [a dividingBy:b];
	float v = [c floatValue];
	return v;
}

@end
