/**
 * @file OZString.h
 * @brief Defines the OZString class for constant strings.
 *
 * This class provides an immutable string object backed by Clang's
 * gnustep-2.0 constant string layout: { isa, flags, length, size, hash, data }.
 */
#pragma once
#include "OZString+Protocol.h"
#include "Object.h"

/**
 * @brief A constant string class (gnustep-2.0 layout).
 * @headerfile OZString.h objc/objc.h
 * @ingroup objc
 *
 * Ivar layout matches Clang's __objc_constant_string struct exactly.
 * For compatibility with modern Objective-C code, it is aliased to
 * `NSString` when compiling with Clang.
 */
@interface OZString : Object <OZStringProtocol> {
@private
  unsigned int _flags;  ///< Flags (reserved, currently 0).
  unsigned int _length; ///< Number of bytes in the string (not including NUL).
  unsigned int _size;   ///< Allocated size in bytes (same as length for constants).
  unsigned int _hash;   ///< Cached hash value (0 = not computed).
  const char *_data;    ///< Pointer to the null-terminated C-string data.
}

/**
 * @brief Returns the C-string representation of the constant string.
 * @return A pointer to the null-terminated C-string.
 */
- (const char *)cStr;

/**
 * @brief Returns the length of the constant string.
 * @return The length of the string in bytes, not including the null terminator.
 */
- (unsigned int)length;

@end

#ifdef __clang__
/**
 * @brief Provides a compatibility alias for `NSString`.
 *
 * When compiling with Clang, `OZString` is aliased to `NSString`
 * to allow for greater compatibility with modern Objective-C code and
 * frameworks.
 */
@compatibility_alias NSString OZString;
#endif
