/*
 * Behavioral spec derived from: ObjFW OFStringTests.m
 * ObjFW license: LGPL-3.0-only
 * This test is ORIGINAL CODE — no ObjFW code was copied.
 * Pattern: equality/hash contract for OZString.
 */
/* oz-pool: OZObject=1,OZString=2,StrEqTest=1 */
#import "OZFoundationBase.h"

@interface StrEqTest : OZObject {
	BOOL _equalResult;
}
- (void)run;
- (BOOL)equalResult;
@end

@implementation StrEqTest
- (void)run {
	OZString *a = @"hello";
	OZString *b = @"hello";
	_equalResult = [a isEqual:b];
}
- (BOOL)equalResult {
	return _equalResult;
}
@end
