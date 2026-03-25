/* oz-pool: OZObject=1,OZQ31=32 */
/* Behavior test: OZQ31 integer-only cDescription and division (no stdio). */
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
	OZQ31 *a = @(10);
	OZQ31 *b = @(4);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}

- (float)divTenByThree
{
	OZQ31 *a = @(10);
	OZQ31 *b = @(3);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}

- (float)divNegTenByTwo
{
	OZQ31 *a = @(-10);
	OZQ31 *b = @(2);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}

- (float)divTenByNegTwo
{
	OZQ31 *a = @(10);
	OZQ31 *b = @(-2);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}

- (float)divNegByNeg
{
	OZQ31 *a = @(-10);
	OZQ31 *b = @(-2);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}

- (float)divSelfBySelf
{
	OZQ31 *a = @(42);
	OZQ31 *b = @(42);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}

- (float)divSmallByLarge
{
	OZQ31 *a = @(1);
	OZQ31 *b = @(1000);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}

- (float)divLargeBySmall
{
	OZQ31 *a = @(1000);
	OZQ31 *b = @(1);
	OZQ31 *c = [a div:b];
	float v = [c floatValue];
	return v;
}

- (int)divByZeroRaw
{
	OZQ31 *a = @(10);
	OZQ31 *b = [OZQ31 fixedWithRaw:0 shift:0];
	OZQ31 *c = [a div:b];
	int v = [c rawValue];
	return v;
}

@end
