/*
 * Adapted from: GNUstep libobjc2 — Test/BasicTest.m
 * License: MIT
 * Adaptation: Replaced objc_msgSend with OZ_SEND, assert with Unity.
 */
#import "OZTestBase.h"

@interface Counter : OZObject {
	int _count;
}
- (void)increment;
- (void)decrement;
- (int)count;
@end

@implementation Counter
- (void)increment { _count = _count + 1; }
- (void)decrement { _count = _count - 1; }
- (int)count { return _count; }
@end
