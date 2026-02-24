/**
 * @file OZObject.h
 * @brief Managed root class with reference counting.
 *
 * Subclass OZObject (not Object) for automatic memory management.
 * Object remains available for lightweight allocations without refcounting.
 */
#pragma once
#import <objc/Object.h>
#include <zephyr/sys/atomic.h>

/**
 * @brief Managed root class with atomic reference counting.
 * @headerfile OZObject.h objc/OZObject.h
 * @ingroup objc
 *
 * OZObject extends Object with retain/release/autorelease semantics.
 * The refcount uses Zephyr's atomic API for thread safety.
 */
@interface OZObject : Object {
	atomic_t _refcount;
}

/**
 * @brief Allocate a new instance with refcount = 1.
 * @return A pointer to the instance, or nil if allocation failed.
 */
+ (id)alloc;

/**
 * @brief Initialize the instance.
 * @return The initialized object.
 */
- (id)init;

/**
 * @brief Increment the reference count.
 * @return self.
 */
- (id)retain;

/**
 * @brief Decrement the reference count. Calls dealloc when it reaches 0.
 */
- (oneway void)release;

/**
 * @brief Add self to the current autorelease pool.
 * @return self.
 */
- (id)autorelease;

/**
 * @brief Return the current reference count.
 * @return The reference count value.
 */
- (unsigned int)retainCount;

/**
 * @brief Free the object's resources. Called when refcount reaches 0.
 */
- (void)dealloc;

@end
