/*
 * Adapted from: tests/objc-reference/runtime/message_dispatch/src/main.c
 * Adaptation: Removed objc_msg_lookup introspection.
 *             Replaced with direct method calls verifying dispatch.
 *             Pattern: method override resolves to child implementation.
 */
#import "OZTestBase.h"

@interface Animal : OZObject {
	int _legs;
}
- (int)legs;
- (int)speak;
@end

@implementation Animal
- (int)legs { return _legs; }
- (int)speak { return 0; }
@end

@interface Dog : Animal
- (int)speak;
@end

@implementation Dog
- (int)speak { return 1; }
@end

@interface Cat : Animal
- (int)speak;
@end

@implementation Cat
- (int)speak { return 2; }
@end
