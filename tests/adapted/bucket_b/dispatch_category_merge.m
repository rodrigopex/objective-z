/*
 * Adapted from: tests/objc-reference/runtime/categories/src/main.c
 * Adaptation: Removed class_respondsToSelector introspection.
 *             Replaced with direct category method calls.
 *             Pattern: category adds method, callable on instance.
 */
#import "OZTestBase.h"

@interface Calculator : OZObject {
	int _value;
}
- (int)value;
- (void)setValue:(int)v;
@end

@implementation Calculator
- (int)value { return _value; }
- (void)setValue:(int)v { _value = v; }
@end

@interface Calculator (Math)
- (void)add:(int)n;
- (void)multiply:(int)n;
@end

@implementation Calculator (Math)
- (void)add:(int)n { _value = _value + n; }
- (void)multiply:(int)n { _value = _value * n; }
@end
