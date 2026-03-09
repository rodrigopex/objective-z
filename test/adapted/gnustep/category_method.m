/*
 * Adapted from: GNUstep libobjc2 — Test/CategoryTest.m
 * License: MIT
 * Adaptation: Replaced runtime introspection with direct method calls.
 *             Category methods are merged into the class at transpile time.
 */
#import "OZTestBase.h"

@interface Printer : OZObject {
	int _pages;
}
- (int)pages;
@end

@implementation Printer
- (int)pages { return _pages; }
@end

@interface Printer (Extras)
- (void)addPages:(int)count;
- (int)doublePages;
@end

@implementation Printer (Extras)
- (void)addPages:(int)count { _pages = _pages + count; }
- (int)doublePages { return _pages * 2; }
@end
