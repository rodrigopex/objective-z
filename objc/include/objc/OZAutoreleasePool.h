/**
 * @file OZAutoreleasePool.h
 * @brief Autorelease pool with per-thread stack.
 *
 * Provides @autoreleasepool {} support for gnustep-1.7 runtime.
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
 * Pools manage their own lifetime via -drain rather than -release.
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
