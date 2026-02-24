/**
 * @file arc.c
 * @brief ARC entry point implementations.
 *
 * Pure C — compiled with -fno-objc-arc.  Delegates to the MRR
 * refcount layer and OZObject message sends via objc_msg_lookup.
 */
#include "api.h"
#include <objc/arc.h>
#include <objc/runtime.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>

/* refcount.c helpers */
extern id __objc_refcount_retain(id obj);
extern bool __objc_refcount_release(id obj);
extern id __objc_autorelease_add(id obj);

/* message.c */
extern IMP objc_msg_lookup(id receiver, SEL selector);

/**
 * Send -dealloc to obj via the message dispatch.
 * Used when release drops refcount to zero.
 */
static void __objc_arc_dealloc(id obj)
{
	static struct objc_selector dealloc_sel = {
		.sel_id = "dealloc",
		.sel_type = NULL,
	};
	IMP imp = objc_msg_lookup(obj, &dealloc_sel);
	if (imp != NULL) {
		((void (*)(id, SEL))imp)(obj, &dealloc_sel);
	}
}

/* ── Core ARC entry points ────────────────────────────────────────── */

id objc_retain(id obj)
{
	if (obj == nil) {
		return nil;
	}
	return __objc_refcount_retain(obj);
}

void objc_release(id obj)
{
	if (obj == nil) {
		return;
	}
	if (__objc_refcount_release(obj)) {
		__objc_arc_dealloc(obj);
	}
}

id objc_autorelease(id obj)
{
	if (obj == nil) {
		return nil;
	}
	return __objc_autorelease_add(obj);
}

void objc_storeStrong(id *location, id val)
{
	id old = *location;
	if (val == old) {
		return;
	}
	if (val != nil) {
		objc_retain(val);
	}
	*location = val;
	if (old != nil) {
		objc_release(old);
	}
}

id objc_retainAutorelease(id obj)
{
	return objc_autorelease(objc_retain(obj));
}

/* ── Return-value optimisation ────────────────────────────────────── */

/*
 * RV optimisation: when the caller of the returning function has
 * placed the marker "mov r7, r7" (0x463F) right before calling
 * objc_retainAutoreleasedReturnValue, we can skip the autorelease+
 * retain pair entirely.  A TLS flag coordinates the handshake.
 *
 * ARM Thumb-2 marker: mov r7, r7 = 0x463F (16-bit Thumb)
 */
#define RV_MARKER 0x463F

static __thread bool _rv_flag;

id objc_autoreleaseReturnValue(id obj)
{
	if (obj == nil) {
		return nil;
	}

	/*
	 * Check if the caller placed the RV marker.
	 *
	 * The returning function (e.g. createSensor) typically tail-calls
	 * us, so __builtin_return_address(0) yields the return address in
	 * the original caller (e.g. main) — right where the compiler
	 * inserted "mov r7, r7" before objc_retainAutoreleasedReturnValue.
	 *
	 * On Thumb, bit 0 of the return address is set; mask it off.
	 */
	void *ret = __builtin_return_address(0);
	uint16_t *pc = (uint16_t *)((uintptr_t)ret & ~(uintptr_t)1);
	if (pc != NULL && *pc == RV_MARKER) {
		_rv_flag = true;
		return obj;
	}

	return objc_autorelease(obj);
}

id objc_retainAutoreleaseReturnValue(id obj)
{
	return objc_autorelease(objc_retain(obj));
}

id objc_retainAutoreleasedReturnValue(id obj)
{
	if (_rv_flag) {
		_rv_flag = false;
		return obj;
	}
	return objc_retain(obj);
}

/* ── Weak reference stubs ─────────────────────────────────────────── */

id objc_storeWeak(id *location, id val)
{
	(void)location;
	(void)val;
	printk("FATAL: __weak is not supported by this runtime\n");
	k_panic();
	return nil;
}

id objc_loadWeakRetained(id *location)
{
	(void)location;
	printk("FATAL: __weak is not supported by this runtime\n");
	k_panic();
	return nil;
}

void objc_destroyWeak(id *location)
{
	(void)location;
	printk("FATAL: __weak is not supported by this runtime\n");
	k_panic();
}
