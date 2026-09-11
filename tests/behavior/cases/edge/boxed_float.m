/* oz-pool: OZObject=1,OZNumber=1,BoxedFloatTest=1 */
#import "OZTestBase.h"
#import <Foundation/OZNumber.h>

@interface BoxedFloatTest : OZObject {
	OZNumber *_boxed;
}
- (void)run;
- (OZNumber *)boxed;
@end

@implementation BoxedFloatTest
- (void)run {
	float f = 3.14f;
	_boxed = @(f);
}
- (OZNumber *)boxed {
	return _boxed;
}
@end
