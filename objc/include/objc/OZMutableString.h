/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZMutableString.h
 * @brief Mutable string class for runtime description support.
 *
 * Provides OZMutableString for -description return values.
 * Uses an inline buffer to avoid a secondary heap allocation.
 */
#pragma once
#import <objc/OZObject.h>
#import <objc/OZString+Protocol.h>

/**
 * @brief Mutable string with heap-allocated dynamic buffer.
 * @headerfile OZMutableString.h objc/OZMutableString.h
 * @ingroup objc
 *
 * Used as the return type for -description methods.
 * Buffer grows via objc_realloc when capacity is exceeded.
 */
@interface OZMutableString : OZObject <OZStringProtocol> {
	char *_buf;
	unsigned int _length;
	unsigned int _capacity;
}

/**
 * @brief Create an autoreleased string from a C string.
 * @param str The C string to copy into the buffer.
 * @return An autoreleased OZMutableString, or nil on failure.
 */
+ (id)stringWithCString:(const char *)str;

/**
 * @brief Append a C string to the end of the buffer.
 * @param str The C string to append. If NULL, nothing happens.
 */
- (void)appendCString:(const char *)str;

/**
 * @brief Append a string object conforming to OZStringProtocol.
 * @param str The string to append. If nil, nothing happens.
 */
- (void)appendString:(id<OZStringProtocol>)str;

/**
 * @brief Returns the C string representation.
 * @return A pointer to the internal buffer.
 */
- (const char *)cStr;

/**
 * @brief Returns the length of the string.
 * @return The length in bytes, not including the null terminator.
 */
- (unsigned int)length;

@end
