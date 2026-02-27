/**
 * @file OZArray.h
 * @brief Immutable array class for ObjC collection literals.
 *
 * Provides OZArray for @[...] array literals.
 * Elements are retained on creation and released on dealloc.
 * Aliased to NSArray under Clang for compiler literal codegen.
 */
#pragma once
#import <objc/Object.h>
#import <objc/NSFastEnumeration.h>

/**
 * @brief Immutable ordered collection.
 * @headerfile OZArray.h objc/OZArray.h
 * @ingroup objc
 *
 * Stores a retained copy of the objects passed to the factory method.
 * Supports indexed subscript syntax (arr[0]).
 */
@interface OZArray : Object {
	__unsafe_unretained id *_items;
	unsigned int _count;
}

/**
 * @brief Create an array from a C array of objects.
 * @param objects C array of id values (may be NULL when count is 0).
 * @param count Number of elements.
 * @return An autoreleased OZArray, or nil on allocation failure.
 */
+ (id)arrayWithObjects:(const id *)objects count:(unsigned int)count;

/**
 * @brief Return the number of elements.
 */
- (unsigned int)count;

/**
 * @brief Return the object at the given index.
 * @param index Zero-based index. Must be < count.
 */
- (id)objectAtIndex:(unsigned int)index;

/**
 * @brief Subscript support: arr[index].
 */
- (id)objectAtIndexedSubscript:(unsigned int)index;

/**
 * @brief Fast enumeration support for for...in loops.
 * @param state Compiler-managed iteration state.
 * @param stackbuf Scratch buffer (unused — items returned directly).
 * @param len Size of stackbuf (unused).
 * @return Number of objects returned, or 0 when done.
 */
- (unsigned long)countByEnumeratingWithState:(struct NSFastEnumerationState *)state
				     objects:(__unsafe_unretained id *)stackbuf
				       count:(unsigned long)len;

#ifdef CONFIG_OBJZ_BLOCKS
/**
 * @brief Enumerate all objects using a block callback.
 * @param block Called for each element with the object, its index,
 *              and a pointer to a BOOL that can be set to YES to stop.
 */
- (void)enumerateObjectsUsingBlock:(void (^)(id obj, unsigned int idx, BOOL *stop))block;
#endif

@end

#ifdef __clang__
@compatibility_alias NSArray OZArray;
#endif
