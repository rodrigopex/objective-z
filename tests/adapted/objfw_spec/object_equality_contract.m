/*
 * Behavioral spec derived from: ObjFW OFObjectTests.m
 * ObjFW license: LGPL-3.0-only
 * This test is ORIGINAL CODE — no ObjFW code was copied.
 * Pattern: object isEqual: to itself, not equal to distinct object.
 */
#import "OZTestBase.h"

@interface EqTest : OZObject {
	BOOL _selfEqual;
	BOOL _distinctNotEqual;
}
- (void)run;
- (BOOL)selfEqual;
- (BOOL)distinctNotEqual;
@end

@implementation EqTest
- (void)run {
	_selfEqual = [self isEqual:self];
	OZObject *other = [OZObject alloc];
	_distinctNotEqual = ![self isEqual:other];
}
- (BOOL)selfEqual {
	return _selfEqual;
}
- (BOOL)distinctNotEqual {
	return _distinctNotEqual;
}
@end
