/**
 * @file OZArray.h
 * @brief Immutable array class for ObjC collection literals.
 *
 * Provides OZArray for @[...] array literals.
 * Elements are retained on creation and released on dealloc.
 * Aliased to NSArray under Clang for compiler literal codegen.
 */
#pragma once
#import <objc/OZObject.h>

/**
 * @brief Immutable ordered collection.
 * @headerfile OZArray.h objc/OZArray.h
 * @ingroup objc
 *
 * Stores a retained copy of the objects passed to the factory method.
 * Supports indexed subscript syntax (arr[0]).
 */
@interface OZArray : OZObject {
	id *_items;
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

@end

#ifdef __clang__
@compatibility_alias NSArray OZArray;
#endif
