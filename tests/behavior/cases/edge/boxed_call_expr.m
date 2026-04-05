/* oz-pool: OZObject=1,OZQ31=1,BoxedCallTest=1 */
#import "OZTestBase.h"
#import <Foundation/OZQ31.h>

static int computeValue(void) { return 99; }

@interface BoxedCallTest : OZObject {
	OZQ31 *_boxed;
}
- (void)run;
- (OZQ31 *)boxed;
@end

@implementation BoxedCallTest
- (void)run {
	_boxed = @(computeValue());
}
- (OZQ31 *)boxed {
	return _boxed;
}
@end
