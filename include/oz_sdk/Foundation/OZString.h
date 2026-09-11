/**
 * @file OZString.h
 * @brief Immutable string class for OZ transpiler.
 *
 * Lightweight ObjC interface that Clang can parse for AST dump.
 * The transpiler emits a pure-C struct and static string constants.
 */
#pragma once
#import "OZObject.h"

@interface OZString : OZObject {
	size_t _length;
	const char *_data;
}
- (const char *)cString;
- (size_t)length;
- (BOOL)isEqual:(id)anObject;
- (BOOL)isEqualToString:(OZString *)aString;
- (BOOL)hasPrefix:(OZString *)prefix;
- (BOOL)hasSuffix:(OZString *)suffix;
- (int)getDescription:(char *)buf maxLength:(size_t)maxLen;
@end

#ifdef __clang__
@compatibility_alias NSString OZString;
#endif
