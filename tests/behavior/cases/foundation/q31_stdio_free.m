/* oz-pool: OZObject=1,OZNumber=32 */
/* Behavior test: OZNumber integer-only getDescription and division (no stdio). */
#import "OZFoundationBase.h"

@interface Q31NoStdio : OZObject
/* Division results extracted as float for assertion */
- (float)divTenByFour;
- (float)divTenByThree;
- (float)divNegTenByTwo;
- (float)divTenByNegTwo;
- (float)divNegByNeg;
- (float)divSelfBySelf;
- (float)divSmallByLarge;
- (float)divLargeBySmall;
- (int)divByZeroRaw;
@end

@implementation Q31NoStdio

- (float)divTenByFour
{
	OZNumber *a = @(10);
	OZNumber *b = @(4);
	OZNumber *c = [a dividingBy:b];
	float v = [c floatValue];
	return v;
}

- (float)divTenByThree
{
	OZNumber *a = @(10);
	OZNumber *b = @(3);
	OZNumber *c = [a dividingBy:b];
	float v = [c floatValue];
	return v;
}

- (float)divNegTenByTwo
{
	OZNumber *a = @(-10);
	OZNumber *b = @(2);
	OZNumber *c = [a dividingBy:b];
	float v = [c floatValue];
	return v;
}

- (float)divTenByNegTwo
{
	OZNumber *a = @(10);
	OZNumber *b = @(-2);
	OZNumber *c = [a dividingBy:b];
	float v = [c floatValue];
	return v;
}

- (float)divNegByNeg
{
	OZNumber *a = @(-10);
	OZNumber *b = @(-2);
	OZNumber *c = [a dividingBy:b];
	float v = [c floatValue];
	return v;
}

- (float)divSelfBySelf
{
	OZNumber *a = @(42);
	OZNumber *b = @(42);
	OZNumber *c = [a dividingBy:b];
	float v = [c floatValue];
	return v;
}

- (float)divSmallByLarge
{
	OZNumber *a = @(1);
	OZNumber *b = @(1000);
	OZNumber *c = [a dividingBy:b];
	float v = [c floatValue];
	return v;
}

- (float)divLargeBySmall
{
	OZNumber *a = @(1000);
	OZNumber *b = @(1);
	OZNumber *c = [a dividingBy:b];
	float v = [c floatValue];
	return v;
}

- (int)divByZeroRaw
{
	OZNumber *a = @(10);
	OZNumber *b = [OZNumber numberWithRaw:0 shift:0];
	OZNumber *c = [a dividingBy:b];
	int v = [c rawValue];
	return v;
}

@end
