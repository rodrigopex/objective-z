/*
 * Adapted from: GNUstep libobjc2 — Test/PropertyAttributeTest.m
 * License: MIT
 * Adaptation: Replaced objc_msgSend with direct calls, assert with Unity.
 *             Removed runtime introspection calls.
 */
#import "OZTestBase.h"

@interface Config : OZObject
@property(nonatomic, assign) int level;
@property(nonatomic, assign) int mode;
@end

@implementation Config
@synthesize level = _level;
@synthesize mode = _mode;
@end
