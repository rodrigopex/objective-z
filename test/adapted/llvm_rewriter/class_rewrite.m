/*
 * Adapted from: clang/test/Rewriter/objc-modern-metadata-visibility.mm
 * License: Apache 2.0 with LLVM Exception
 * Adaptation: Verifies @interface becomes a C struct with correct fields
 *             and @implementation generates callable C functions.
 */
#import "OZTestBase.h"

@interface Vehicle : OZObject {
	int _speed;
	int _fuel;
}
- (int)speed;
- (void)setSpeed:(int)speed;
- (int)fuel;
@end

@implementation Vehicle
- (int)speed { return _speed; }
- (void)setSpeed:(int)speed { _speed = speed; }
- (int)fuel { return _fuel; }
@end
