/*
 * Behavioral spec derived from: ObjFW OFStringTests.m
 * ObjFW license: LGPL-3.0-only
 * This test is ORIGINAL CODE — no ObjFW code was copied.
 * Pattern: OZString length and cString round-trip.
 */
/* oz-pool: OZObject=1,OZString=1,StrLenTest=1 */
#import "OZFoundationBase.h"

@interface StrLenTest : OZObject {
	unsigned int _len;
	BOOL _cStringValid;
}
- (void)run;
- (unsigned int)len;
- (BOOL)cStringValid;
@end

@implementation StrLenTest
- (void)run {
	OZString *s = @"world";
	_len = [s length];
	_cStringValid = ([s cString] != nil);
}
- (unsigned int)len {
	return _len;
}
- (BOOL)cStringValid {
	return _cStringValid;
}
@end
