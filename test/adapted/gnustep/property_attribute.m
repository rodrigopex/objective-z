/*
 * Adapted from: GNUstep libobjc2 — Test/PropertyAttributeTest.m
 * License: MIT
 * Adaptation: Replaced objc_msgSend with direct calls, assert with Unity.
 *             Removed runtime introspection calls.
 */
#import "OZTestBase.h"

@interface Config : OZObject {
	int _level;
	int _mode;
}
- (int)level;
- (void)setLevel:(int)level;
- (int)mode;
- (void)setMode:(int)mode;
@end

@implementation Config
- (int)level { return _level; }
- (void)setLevel:(int)level { _level = level; }
- (int)mode { return _mode; }
- (void)setMode:(int)mode { _mode = mode; }
@end
