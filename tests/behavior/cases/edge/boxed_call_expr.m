/* oz-pool: OZObject=1,OZNumber=1,BoxedCallTest=1 */
#import "OZTestBase.h"
#import <Foundation/OZNumber.h>

static int computeValue(void) { return 99; }

@interface BoxedCallTest : OZObject {
	OZNumber *_boxed;
}
- (void)run;
- (OZNumber *)boxed;
@end

@implementation BoxedCallTest
- (void)run {
	_boxed = @(computeValue());
}
- (OZNumber *)boxed {
	return _boxed;
}
@end
