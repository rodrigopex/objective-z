/* oz-pool: OZObject=1,OZString=1,StringAccessTest=1 */
#import "OZFoundationBase.h"

@interface StringAccessTest : OZObject {
	unsigned int _len;
	BOOL _cStringValid;
}
- (void)run;
- (unsigned int)len;
- (BOOL)cStringValid;
@end

@implementation StringAccessTest
- (void)run {
	OZString *s = @"hello";
	_len = [s length];
	const char *cs = [s cString];
	_cStringValid = (cs != nil);
}
- (unsigned int)len {
	return _len;
}
- (BOOL)cStringValid {
	return _cStringValid;
}
@end
