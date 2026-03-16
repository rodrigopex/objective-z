/* oz-pool: OZObject=1 */
#import "OZFoundationBase.h"

@interface StringTest : OZObject
- (const char *)getHello;
- (unsigned int)helloLength;
- (BOOL)sameStringEqual;
@end

@implementation StringTest
- (const char *)getHello {
	OZString *s = @"hello";
	return [s cStr];
}
- (unsigned int)helloLength {
	OZString *s = @"hello";
	return [s length];
}
- (BOOL)sameStringEqual {
	OZString *a = @"hello";
	OZString *b = @"hello";
	return [a isEqual:b];
}
@end
