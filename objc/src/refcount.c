/**
 * @file refcount.c
 * @brief Atomic reference counting core for OZObject.
 *
 * Pure C — no Objective-C syntax. Uses Zephyr atomic API for
 * thread-safe refcount operations on Cortex-M (LDREX/STREX).
 */
#include "api.h"
#include <zephyr/sys/atomic.h>

/*
 * OZObject layout (32-bit ARM):
 *   offset 0: Class isa       (4 bytes, inherited from Object)
 *   offset 4: atomic_t _refcount (4 bytes, OZObject's first ivar)
 */
#define REFCOUNT_OFFSET sizeof(struct objc_object)

static inline atomic_t *__objc_refcount_ptr(id obj)
{
	return (atomic_t *)((char *)obj + REFCOUNT_OFFSET);
}

/**
 * @brief Increment the reference count of a managed object.
 * @param obj The object to retain. Must be an OZObject instance.
 * @return obj.
 */
id __objc_refcount_retain(id obj)
{
	if (obj == nil) {
		return nil;
	}
	atomic_inc(__objc_refcount_ptr(obj));
	return obj;
}

/**
 * @brief Decrement the reference count of a managed object.
 * @param obj The object to release. Must be an OZObject instance.
 * @return true if the refcount reached zero (caller should dealloc).
 */
bool __objc_refcount_release(id obj)
{
	if (obj == nil) {
		return false;
	}
	atomic_val_t old = atomic_dec(__objc_refcount_ptr(obj));
	/* atomic_dec returns the PREVIOUS value; object is dead when old == 1 */
	return (old == 1);
}

/**
 * @brief Read the current reference count.
 * @param obj The object to query. Must be an OZObject instance.
 * @return The current reference count, or 0 if obj is nil.
 */
unsigned int __objc_refcount_get(id obj)
{
	if (obj == nil) {
		return 0;
	}
	return (unsigned int)atomic_get(__objc_refcount_ptr(obj));
}

/**
 * @brief Set the reference count (used during +alloc).
 * @param obj The object. Must be an OZObject instance.
 * @param value The initial refcount value.
 */
void __objc_refcount_set(id obj, atomic_val_t value)
{
	if (obj != nil) {
		atomic_set(__objc_refcount_ptr(obj), value);
	}
}

/*
 * Autorelease support — delegates to OZAutoreleasePool via a function
 * pointer to avoid circular dependency between refcount.c and
 * OZAutoreleasePool.m.
 */
static void (*_autorelease_add_fn)(id obj) = NULL;

/**
 * @brief Register the autorelease callback.
 *
 * Called by OZAutoreleasePool during initialization to provide the
 * +addObject: implementation without a compile-time dependency.
 */
void __objc_refcount_set_autorelease_fn(void (*fn)(id))
{
	_autorelease_add_fn = fn;
}

/**
 * @brief Add an object to the current autorelease pool.
 * @param obj The object to autorelease.
 * @return obj, or nil if no pool is active.
 */
id __objc_autorelease_add(id obj)
{
	if (obj == nil) {
		return nil;
	}
	if (_autorelease_add_fn != NULL) {
		_autorelease_add_fn(obj);
	}
	return obj;
}
