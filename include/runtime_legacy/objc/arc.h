/**
 * @file arc.h
 * @brief ARC (Automatic Reference Counting) entry points.
 *
 * These functions are called by Clang-compiled code when -fobjc-arc
 * is enabled with -fobjc-runtime=gnustep-1.7.  The implementations
 * delegate to the refcount layer (refcount.c / Object).
 */
#pragma once
#include <objc/objc.h>

/**
 * @defgroup arc ARC Entry Points
 * @ingroup objc
 * @{
 */

/** Retain an object. Returns obj. */
id objc_retain(id obj);

/** Release an object. Calls dealloc when refcount reaches 0. */
void objc_release(id obj);

/** Add obj to the current autorelease pool. Returns obj. */
id objc_autorelease(id obj);

/**
 * Atomic strong store: release *location, retain val, store val.
 * If val is nil, just releases *location and zeros it.
 */
void objc_storeStrong(id *location, id val);

/**
 * Read an object-type property.
 * Non-atomic: direct ivar read.
 * Atomic: read under spinlock, retain+autorelease for safety.
 */
id objc_getProperty(id obj, SEL _cmd, ptrdiff_t offset, BOOL isAtomic);

/**
 * Write an object-type property.
 * Non-atomic: swap with retain new + release old.
 * Atomic: swap under spinlock, release old outside lock.
 * @note isCopy is ignored (no NSCopying support).
 */
void objc_setProperty(id obj, SEL _cmd, ptrdiff_t offset, id newValue,
		      BOOL isAtomic, BOOL isCopy);

/**
 * @name gnustep-2.0 Specialized Property Setters
 * Clang emits these instead of objc_setProperty for gnustep-2.0 ABI.
 * @{
 */
void objc_setProperty_atomic(id obj, SEL _cmd, id arg, ptrdiff_t offset);
void objc_setProperty_nonatomic(id obj, SEL _cmd, id arg, ptrdiff_t offset);
void objc_setProperty_atomic_copy(id obj, SEL _cmd, id arg, ptrdiff_t offset);
void objc_setProperty_nonatomic_copy(id obj, SEL _cmd, id arg, ptrdiff_t offset);
/** @} */

/** Retain + autorelease. Returns obj. */
id objc_retainAutorelease(id obj);

/** Autorelease for a return value (RV optimization). Returns obj. */
id objc_autoreleaseReturnValue(id obj);

/** Retain + autorelease a return value (RV optimization). Returns obj. */
id objc_retainAutoreleaseReturnValue(id obj);

/** Claim a returned autoreleased value (RV optimization). Returns obj. */
id objc_retainAutoreleasedReturnValue(id obj);

/**
 * Copy a block to the heap (or retain if already there).
 * ARC calls this when assigning a block to a strong variable.
 * Requires CONFIG_OBJZ_BLOCKS.
 */
id objc_retainBlock(id block);

/** Push a new autorelease pool. Returns an opaque token. */
void *objc_autoreleasePoolPush(void);

/** Pop and drain the pool identified by token. */
void objc_autoreleasePoolPop(void *token);

/**
 * @name Weak Reference Stubs
 * Weak references are not supported on this runtime.
 * These stubs call k_panic() if invoked.
 * @{
 */
id objc_storeWeak(id *location, id val);
id objc_loadWeakRetained(id *location);
void objc_destroyWeak(id *location);
/** @} */

/** @} */
