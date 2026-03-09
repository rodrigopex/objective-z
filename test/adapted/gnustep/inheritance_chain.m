/*
 * Adapted from: GNUstep libobjc2 — Test/InheritanceTest.m
 * License: MIT
 * Adaptation: Replaced runtime introspection with direct method calls.
 *             Tests 3-level hierarchy method resolution.
 */
#import "OZTestBase.h"

@interface Animal : OZObject {
	int _legs;
}
- (int)legs;
- (int)sound;
@end

@implementation Animal
- (int)legs { return _legs; }
- (int)sound { return 0; }
@end

@interface Dog : Animal
@end

@implementation Dog
- (int)sound { return 1; }
@end

@interface Puppy : Dog
@end

@implementation Puppy
- (int)sound { return 2; }
@end
