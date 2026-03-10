/*
 * Adapted from: GNUstep libobjc2 — Test/NilTest.m
 * License: MIT
 * Adaptation: Verifies nil receiver returns zero for all return types.
 *             Uses generated C functions with nil guard.
 */
#import "OZTestBase.h"

@interface Target : OZObject {
	int _value;
}
- (int)value;
- (void)setValue:(int)value;
@end

@implementation Target
- (int)value { return _value; }
- (void)setValue:(int)value { _value = value; }
@end
