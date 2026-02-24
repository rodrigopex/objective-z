/**
 * @file OZAutoreleasePool.h
 * @brief Autorelease pool with per-thread stack.
 *
 * Provides @autoreleasepool {} support for gnustep-1.7 runtime,
 * and OZ_AUTORELEASEPOOL macro for non-ARC code.
 */
#pragma once
#import <objc/Object.h>

#ifndef OBJZ_ARP_CAPACITY
#define OBJZ_ARP_CAPACITY 64
#endif

/**
 * @brief Per-thread autorelease pool.
 * @ingroup objc
 *
 * Inherits from Object (not OZObject) because pools manage their own
 * lifetime via -drain, not refcounting.
 */
@interface OZAutoreleasePool : Object {
	id _objects[OBJZ_ARP_CAPACITY];
	unsigned int _count;
	OZAutoreleasePool *_parent;
}

/**
 * @brief Add an object to the current thread's autorelease pool.
 * @param obj The object to autorelease.
 */
+ (void)addObject:(id)obj;

/**
 * @brief Initialize a new pool and push it onto the thread's pool stack.
 * @return The initialized pool.
 */
- (id)init;

/**
 * @brief Release all objects and pop this pool from the stack.
 */
- (void)drain;

@end

/**
 * @brief Push a new autorelease pool onto the current thread's stack.
 * @return An opaque token to pass to __objc_autoreleasepool_pop().
 *
 * Used by ARC entry points (objc_autoreleasePoolPush) and the
 * @autoreleasepool {} syntax under gnustep-1.7.
 */
void *__objc_autoreleasepool_push(void);

/**
 * @brief Pop and drain the autorelease pool identified by token.
 * @param token The value returned by __objc_autoreleasepool_push().
 */
void __objc_autoreleasepool_pop(void *token);

#if __OBJC__
/**
 * @def OZ_AUTORELEASEPOOL
 * @brief Scoped autorelease pool macro for non-ARC code.
 *
 * Usage:
 *   OZ_AUTORELEASEPOOL {
 *       id obj = [[OZObject alloc] init];
 *       [obj autorelease];
 *   }
 */
#define OZ_AUTORELEASEPOOL                                                                         \
	for (OZAutoreleasePool *_oz_pool                                                           \
	             __attribute__((cleanup(__objc_pool_cleanup))) =                                \
	                     [[OZAutoreleasePool alloc] init],                                      \
	                     *_oz_once = (id)(uintptr_t)1;                                          \
	     _oz_once; _oz_once = nil)

static inline void __objc_pool_cleanup(OZAutoreleasePool **pp)
{
	[*pp drain];
}
#endif /* __OBJC__ */
