/**
 * @file Object+Protocol.h
 * @brief Protocol for transpiled OZ objects.
 */
#pragma once
#include <stdbool.h>

#ifndef BOOL
typedef bool BOOL;
#endif

/**
 * @brief Protocol for objects.
 *
 * Defines the minimal interface that transpiled objects must implement
 * for basic introspection.
 */
@protocol ObjectProtocol
@required
- (BOOL)isEqual:(id)anObject;
- (int)cDescription:(char *)buf maxLength:(int)maxLen;
@end
