/* oz-pool: OZObject=1,OZQ31=1,BoxedFloatTest=1 */
#import "OZTestBase.h"
#import <Foundation/OZQ31.h>

@interface BoxedFloatTest : OZObject {
	OZQ31 *_boxed;
}
- (void)run;
- (OZQ31 *)boxed;
@end

@implementation BoxedFloatTest
- (void)run {
	float f = 3.14f;
	_boxed = @(f);
}
- (OZQ31 *)boxed {
	return _boxed;
}
@end
