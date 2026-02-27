/**
 * @file OZDictionary.h
 * @brief Immutable dictionary class for ObjC collection literals.
 *
 * Provides OZDictionary for @{key: value, ...} dictionary literals.
 * Keys and values are retained on creation and released on dealloc.
 * Aliased to NSDictionary under Clang for compiler literal codegen.
 */
#pragma once
#import <objc/Object.h>
#import <objc/NSFastEnumeration.h>

/**
 * @brief Immutable key-value collection.
 * @headerfile OZDictionary.h objc/OZDictionary.h
 * @ingroup objc
 *
 * Stores retained copies of keys and values. Lookup is linear scan
 * using -isEqual: on keys. Supports keyed subscript syntax (dict[@"k"]).
 */
@interface OZDictionary : Object {
	__unsafe_unretained id *_keys;
	__unsafe_unretained id *_values;
	unsigned int _count;
}

/**
 * @brief Create a dictionary from parallel key/value C arrays.
 * @param objects C array of values.
 * @param keys C array of keys.
 * @param count Number of key-value pairs.
 * @return An autoreleased OZDictionary, or nil on allocation failure.
 */
+ (id)dictionaryWithObjects:(const id *)objects
		    forKeys:(const id *)keys
		      count:(unsigned int)count;

/**
 * @brief Return the number of key-value pairs.
 */
- (unsigned int)count;

/**
 * @brief Return the value associated with a key, or nil.
 * @param key The key to look up (compared via -isEqual:).
 */
- (id)objectForKey:(id)key;

/**
 * @brief Subscript support: dict[key].
 */
- (id)objectForKeyedSubscript:(id)key;

/**
 * @brief Fast enumeration over keys for for...in loops.
 * @param state Compiler-managed iteration state.
 * @param stackbuf Scratch buffer (unused — keys returned directly).
 * @param len Size of stackbuf (unused).
 * @return Number of keys returned, or 0 when done.
 */
- (unsigned long)countByEnumeratingWithState:(struct NSFastEnumerationState *)state
				     objects:(__unsafe_unretained id *)stackbuf
				       count:(unsigned long)len;

@end

#ifdef __clang__
@compatibility_alias NSDictionary OZDictionary;
#endif
