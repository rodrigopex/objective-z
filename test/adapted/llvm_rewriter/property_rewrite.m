/*
 * Adapted from: clang/test/Rewriter/objc-modern-property-attributes.mm
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies property-like getter/setter generates correct C
 *             functions with expected naming convention.
 */
#import "OZTestBase.h"

@interface Sensor : OZObject {
	int _temperature;
	int _humidity;
}
- (int)temperature;
- (void)setTemperature:(int)temperature;
- (int)humidity;
@end

@implementation Sensor
- (int)temperature { return _temperature; }
- (void)setTemperature:(int)temperature { _temperature = temperature; }
- (int)humidity { return _humidity; }
@end
