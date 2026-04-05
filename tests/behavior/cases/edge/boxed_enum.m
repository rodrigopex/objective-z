/* oz-pool: OZObject=1,OZQ31=1,BoxedEnumTest=1 */
#import "OZTestBase.h"
#import <Foundation/OZQ31.h>

enum StatusCode {
	StatusOK = 200,
	StatusNotFound = 404
};

@interface BoxedEnumTest : OZObject {
	OZQ31 *_boxed;
}
- (void)boxStatus:(enum StatusCode)code;
- (OZQ31 *)boxed;
@end

@implementation BoxedEnumTest
- (void)boxStatus:(enum StatusCode)code {
	_boxed = @(code);
}
- (OZQ31 *)boxed {
	return _boxed;
}
@end
