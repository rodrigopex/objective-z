/**
 * @file OZMutableString.h
 * @brief Mutable string class for OZ transpiler.
 *
 * Heap-allocated dynamic string that inherits from OZString.
 * Buffer grows via capacity doubling when needed.
 */
#pragma once
#import "OZString.h"

@interface OZMutableString : OZString {
	unsigned int _capacity;
}
- (id)initWithCString:(const char *)str;
- (id)initWithString:(OZString *)aString;
- (id)initWithCapacity:(unsigned int)capacity;
- (void)appendString:(OZString *)aString;
- (void)appendCString:(const char *)str;
- (void)setString:(OZString *)aString;
@end
