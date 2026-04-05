/*
 * Adapted from: GNUstep libobjc2 — Test/category_properties.m
 * License: MIT
 * Adaptation: Verifies properties declared in categories generate accessors.
 */
#import "OZTestBase.h"

@interface Gadget : OZObject {
	int _power;
}
- (int)power;
@end

@implementation Gadget
- (int)power { return _power; }
@end

@interface Gadget (Settings)
- (void)setPower:(int)p;
- (int)doublePower;
@end

@implementation Gadget (Settings)
- (void)setPower:(int)p { _power = p; }
- (int)doublePower { return _power * 2; }
@end
