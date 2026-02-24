/**
 * @file slot.c
 * @brief GNUstep-1.7 slot lookup for [super ...] calls.
 *
 * gnustep-1.7 uses objc_slot_lookup_super() which returns a
 * struct objc_slot* (the caller reads the IMP from slot->method).
 * We bridge this to the existing objc_msg_lookup_super().
 */
#include "api.h"
#include <objc/runtime.h>

/**
 * GNUstep slot structure. The caller accesses field 4 (method)
 * to get the IMP after objc_slot_lookup_super returns.
 */
struct objc_slot {
	Class owner;
	Class cached_for;
	const char *types;
	unsigned int version;
	IMP method;
};

/* objc_msg_lookup_super from message.c */
extern IMP objc_msg_lookup_super(struct objc_super *super, SEL selector);

/**
 * @brief Look up a method slot for a super send.
 * @param super Pointer to {receiver, superclass} struct.
 * @param selector The selector to look up.
 * @return Pointer to a slot containing the IMP, or a slot with NULL method.
 *
 * gnustep-1.7 emits calls to this instead of objc_msg_lookup_super.
 * We use a per-thread static slot to avoid dynamic allocation.
 */
struct objc_slot *objc_slot_lookup_super(struct objc_super *super, SEL selector)
{
	static struct objc_slot _slot;

	IMP imp = objc_msg_lookup_super(super, selector);

	_slot.owner = Nil;
	_slot.cached_for = Nil;
	_slot.types = NULL;
	_slot.version = 0;
	_slot.method = imp;

	return &_slot;
}
