#include "api.h"
#include "category.h"
#include "class.h"
#include "hash.h"
#include <objc/objc.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>

/* This function is called when a message is sent to a nil object. */
static id __objc_nil_method(id receiver, SEL selector OBJC_UNUSED)
{
	return receiver;
}

static IMP __objc_msg_lookup(objc_class_t *cls, SEL selector)
{
	if (cls == Nil || selector == NULL) {
		return NULL;
	}

#ifdef OBJCDEBUG
	printk("objc_msg_lookup %c[%s %s]\n", cls->info & objc_class_flag_meta ? '+' : '-',
	       cls->name, sel_getName(selector));
#endif

	/* Descend through the classes looking for the method */
	while (cls != Nil) {
#ifdef OBJCDEBUG
		printk("  %c[%s %s] types=%s\n", cls->info & objc_class_flag_meta ? '+' : '-',
		       cls->name, sel_getName(selector), selector->types);
#endif
		struct objc_hashitem *item =
			__objc_hash_lookup(cls, selector->name, selector->types);
		if (item != NULL) {
			return item->imp;
		}
		cls = cls->superclass;
	}

	return NULL;
}

/**
 * Send +initialize to a metaclass.
 *
 * ObjC convention: +initialize receives the CLASS object as self, not
 * the metaclass.  We look up the instance class via __objc_lookup_class
 * so that `self == [ClassName class]` works correctly in +initialize.
 */
static void __objc_send_initialize(objc_class_t *metacls)
{
	if (metacls == Nil) {
		return;
	}

	if (metacls->info & objc_class_flag_initialized) {
		return;
	}

	metacls->info |= objc_class_flag_initialized;

	if (metacls->superclass) {
		__objc_send_initialize(metacls->superclass);
	}

	static struct objc_selector initialize = {
		.name = "initialize",
		.types = NULL,
	};
	IMP imp = __objc_msg_lookup(metacls, &initialize);
	if (imp != NULL) {
		objc_class_t *class_obj = __objc_lookup_class(metacls->name);
		id receiver = class_obj ? (id)class_obj : (id)metacls;

#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wcast-function-type"
		((void (*)(id, SEL))imp)(receiver, &initialize);
#pragma GCC diagnostic pop
	}
}

/**
 * Message dispatch function. Returns the implementation pointer for
 * the specified selector. Returns the nil_method if the receiver is nil,
 * and panics if the selector is not found.
 */
IMP objc_msg_lookup(id receiver, SEL selector)
{
	if (receiver == NULL) {
		return (IMP)__objc_nil_method;
	}

	/* Load categories on first message send */
	static BOOL init = NO;
	if (init == NO) {
		init = YES;
		__objc_category_load();
	}

	objc_class_t *cls = receiver->isa;
	if (cls == Nil) {
		printk("objc_msg_lookup: receiver @%p class is Nil (selector=%s)", receiver,
		       sel_getName(selector));
		return NULL;
	}

	IMP imp = __objc_msg_lookup(cls, selector);
	if (imp == NULL) {
		printk("objc_msg_lookup: class=%c[%s %s] selector->types=%s cannot send "
		       "message\n",
		       receiver->isa->info & objc_class_flag_meta ? '+' : '-',
		       receiver->isa->name, sel_getName(selector), selector->types);
	} else {
#ifdef OBJCDEBUG
		printk("    => IMP @%p\n", imp);
#endif
	}

	/* Send +initialize if not yet done */
	objc_class_t *meta_cls = cls->info & objc_class_flag_meta ? cls : cls->metaclass;
	if (!(meta_cls->info & objc_class_flag_initialized)) {
#ifdef OBJCDEBUG
		printk("  +[%s initialize] \n", cls->name);
#endif
		__objc_send_initialize(meta_cls);
	}

	return imp;
}

/**
 * Message superclass dispatch function.
 */
IMP objc_msg_lookup_super(struct objc_super *super, SEL selector)
{
	if (super == NULL || super->receiver == nil) {
		return NULL;
	}
	IMP imp = __objc_msg_lookup(super->superclass, selector);
	if (imp == NULL) {
		printk("objc_msg_lookup: class=%c[%s %s] selector->types=%s not found\n",
		       super->receiver->isa->info & objc_class_flag_meta ? '+' : '-',
		       super->receiver->isa->name, sel_getName(selector), selector->types);
	}
	return imp;
}

BOOL class_respondsToSelector(Class cls, SEL selector)
{
	if (cls == Nil) {
		return NO;
	}
	if (selector == NULL) {
		printk("class_respondsToSelector: SEL is NULL");
		return NO;
	}
#ifdef OBJCDEBUG
	printk("class_respondsToSelector %c[%s %s] types=%s\n",
	       cls->info & objc_class_flag_meta ? '+' : '-', cls->name, sel_getName(selector),
	       selector->types);
#endif
	return __objc_msg_lookup(cls, selector) == NULL ? NO : YES;
}

BOOL object_respondsToSelector(id object, SEL selector)
{
	if (object == NULL) {
		return NO;
	}
	if (selector == NULL) {
		printk("object_respondsToSelector: SEL is NULL");
		return NO;
	}
	Class cls = object_getClass(object);
#ifdef OBJCDEBUG
	printk("object_respondsToSelector %c[%s %s] types=%s\n",
	       cls->info & objc_class_flag_meta ? '+' : '-', cls->name, sel_getName(selector),
	       selector->types);
#endif
	return __objc_msg_lookup(cls, selector) == NULL ? NO : YES;
}

BOOL class_metaclassRespondsToSelector(Class cls, SEL selector)
{
	if (cls == Nil) {
		return NO;
	}
	if (!(cls->info & objc_class_flag_meta)) {
		cls = cls->metaclass;
	}
	return class_respondsToSelector(cls, selector);
}

const char *sel_getName(SEL sel)
{
	if (sel == NULL) {
		return NULL;
	}
	return sel->name;
}
