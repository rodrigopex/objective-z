/* oz-pool: OZObject=1,OZNumber=1,BoxedEnumTest=1 */
#import "OZTestBase.h"
#import <Foundation/OZNumber.h>

enum StatusCode {
	StatusOK = 200,
	StatusNotFound = 404
};

@interface BoxedEnumTest : OZObject {
	OZNumber *_boxed;
}
- (void)boxStatus:(enum StatusCode)code;
- (OZNumber *)boxed;
@end

@implementation BoxedEnumTest
- (void)boxStatus:(enum StatusCode)code {
	_boxed = @(code);
}
- (OZNumber *)boxed {
	return _boxed;
}
@end
