/*
 * Adapted from: clang/test/Rewriter/objc-modern-property-attributes.mm
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies property-like getter/setter generates correct C
 *             functions with expected naming convention.
 */
#import "OZTestBase.h"

@interface Sensor : OZObject
@property(nonatomic, assign) int temperature;
@property(nonatomic, assign, readonly) int humidity;
@end

@implementation Sensor
@synthesize temperature = _temperature;
@synthesize humidity = _humidity;
@end
