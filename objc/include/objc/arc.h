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

/** Retain + autorelease. Returns obj. */
id objc_retainAutorelease(id obj);

/** Autorelease for a return value (RV optimization). Returns obj. */
id objc_autoreleaseReturnValue(id obj);

/** Retain + autorelease a return value (RV optimization). Returns obj. */
id objc_retainAutoreleaseReturnValue(id obj);

/** Claim a returned autoreleased value (RV optimization). Returns obj. */
id objc_retainAutoreleasedReturnValue(id obj);

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
