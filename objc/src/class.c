#include "class.h"
#include "api.h"
#include "hash.h"
#include "protocol.h"
#include "zephyr/spinlock.h"
#include <objc/objc.h>
#include <string.h>
#include <zephyr/kernel.h>

objc_class_t *class_table[CONFIG_OBJZ_CLASS_TABLE_SIZE + 1];

/**
 * Fix up ivar offsets for a gnustep-2.0 class.
 *
 * The compiler emits per-ivar offset globals initialised to zero and
 * stores instance_size as a negative value for non-fragile classes.
 * The runtime computes actual offsets based on the resolved superclass
 * instance size and each ivar's explicit size field.
 *
 * Writes the correct offset into *(ivar->offset) and updates
 * cls->instance_size to the total.
 */
static void __objc_fixup_ivar_offsets(objc_class_t *cls)
{
	if (cls == NULL || (cls->info & objc_class_flag_meta)) {
		return;
	}

	/* Determine superclass instance size.
	 * Immortal classes (OZString, Protocol) have statically-emitted
	 * instances whose layout assumes Object has only { isa }.  Use
	 * the isa-only size so their ivar offsets match the compiler. */
	size_t offset = 0;
	if (cls->superclass != NULL) {
		if (cls->info & objc_class_flag_immortal) {
			offset = sizeof(struct objc_object);
		} else {
			offset = cls->superclass->instance_size;
		}
	}

	struct objc_ivar_list *il = cls->ivars;
	if (il != NULL) {
		for (int i = 0; i < il->count; i++) {
			struct objc_ivar *ivar = &il->ivars[i];
			uint32_t ivar_size = ivar->size;

			/* Natural alignment = min(ivar_size, sizeof(void*)) */
			size_t align = ivar_size;
			if (align > sizeof(void *)) {
				align = sizeof(void *);
			}
			if (align == 0) {
				align = 1;
			}

			/* Align the current offset */
			offset = (offset + align - 1) & ~(align - 1);

			/* Write offset into the global variable */
			if (ivar->offset != NULL) {
				*(ivar->offset) = (int)offset;
			}

			offset += ivar_size;
		}
	}

	/* Update total instance size */
	cls->instance_size = (long)offset;
}

void __objc_class_init(void)
{
	static BOOL init = NO;
	if (init) {
		return;
	}
	init = YES;

	for (int i = 0; i <= CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
		class_table[i] = NULL;
	}
}

void __objc_class_register(objc_class_t *p)
{
	if (p == NULL || p->name == NULL) {
		return;
	}

	/*
	 * gnustep-2.0 encodes instance size as negative to indicate
	 * non-fragile ivar support. Convert to positive.
	 */
	if ((long)p->instance_size < 0) {
		p->instance_size = -(long)p->instance_size;
	}

#ifdef OBJCDEBUG
	printk("__objc_class_register %c[%s] @%p size=%ld\n",
	       p->info & objc_class_flag_meta ? '+' : '-', p->name, p, p->instance_size);
#endif
	for (int i = 0; i < CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
		if (class_table[i] == p) {
			return;
		}
		if (class_table[i] == NULL) {
			class_table[i] = p;

			if (p->protocols != NULL) {
				__objc_protocol_list_register(p->protocols);
			}
			return;
		}

		if (strcmp(class_table[i]->name, p->name) == 0) {
			/*
			 * In gnustep-2.0, metaclass and instance class share
			 * the same name.  Only warn if both are the same kind.
			 */
			BOOL same_kind =
				(class_table[i]->info & objc_class_flag_meta) ==
				(p->info & objc_class_flag_meta);
			if (same_kind) {
				printk("Duplicate class named: %s", p->name);
			}
		}
	}
	printk("Class table is full, cannot register class: %s", p->name);
}

objc_class_t *__objc_lookup_class(const char *name)
{
	if (name == NULL) {
		return Nil;
	}
	for (int i = 0; i < CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
		if (class_table[i] == NULL || class_table[i]->name == NULL) {
			continue;
		}
		if (strcmp(class_table[i]->name, name) == 0) {
			return class_table[i];
		}
	}
	return Nil;
}

/**
 * Register a list of methods for a class.
 *
 * In gnustep-2.0 each method has { imp, selector, types } where
 * selector points to a struct objc_selector { name, types }.
 */
void __objc_class_register_method_list(objc_class_t *cls, struct objc_method_list *ml)
{
	if (ml == NULL) {
		return;
	}
	for (int i = 0; i < ml->count; i++) {
		struct objc_method *method = &ml->methods[i];
		if (method == NULL || method->selector == NULL || method->imp == NULL) {
			continue;
		}

		const char *method_name = method->selector->name;
		if (method_name == NULL) {
			continue;
		}

#ifdef OBJCDEBUG
		printk("    %c[%s %s] types=%s imp=%p\n",
		       cls->info & objc_class_flag_meta ? '+' : '-', cls->name, method_name,
		       method->types, method->imp);
#endif
		/* Register WITH type */
		struct objc_hashitem *item =
			__objc_hash_register(cls, method_name, method->types, method->imp);
		if (item == NULL) {
			printk("TODO: Failed to register method %s in class %s\n", method_name,
			       cls->name);
			return;
		}

		/* Register WITHOUT type (for type-agnostic lookup) */
		item = __objc_hash_register(cls, method_name, NULL, method->imp);
		if (item == NULL) {
			printk("TODO: Failed to register method %s in class %s\n", method_name,
			       cls->name);
			return;
		}
	}
}

/**
 * Register methods in the class for lookup via objc_msg_lookup.
 */
void __objc_class_register_methods(objc_class_t *p)
{
	if (p->info & objc_class_flag_resolved) {
		return;
	}

	p->info |= objc_class_flag_resolved;

#ifdef OBJCDEBUG
	printk("  __objc_class_register_methods %c[%s] @%p size=%ld\n",
	       p->info & objc_class_flag_meta ? '+' : '-', p->name, p, p->instance_size);
#endif

	for (struct objc_method_list *ml = p->methods; ml != NULL; ml = ml->next) {
		__objc_class_register_method_list(p, ml);
	}

	/*
	 * Resolve the superclass.
	 *
	 * gnustep-2.0: the superclass field is already a class pointer
	 * (resolved at link time), not a name string.  Just ensure the
	 * superclass's methods are registered.
	 */
	if (p->superclass != NULL && !(p->info & objc_class_flag_meta)) {
		objc_class_t *super = p->superclass;
		if (!(super->info & objc_class_flag_resolved)) {
			objc_lookup_class(super->name);
		}
	}
}

/**
 * Lookup the class with the specified name, resolve methods and ivar offsets.
 */
Class objc_lookup_class(const char *name)
{
	objc_class_t *cls = __objc_lookup_class(name);
	if (cls == Nil) {
		return Nil;
	}
#ifdef OBJCDEBUG
	printk("objc_lookup_class %c[%s] @%p\n", cls->info & objc_class_flag_meta ? '+' : '-',
	       name, cls);
#endif

	if (cls->info & objc_class_flag_resolved) {
		if (cls->metaclass != NULL && !(cls->metaclass->info & objc_class_flag_resolved)) {
			/* Need to resolve the metaclass */
		} else {
			return (Class)cls;
		}
	}

	__objc_class_register_methods(cls);

	/* Set up the metaclass superclass BEFORE registering metaclass methods */
	if (cls->metaclass != NULL && cls->superclass != NULL) {
		cls->metaclass->superclass = cls->superclass->metaclass;
	}

	if (cls->metaclass != NULL) {
		__objc_class_register_methods(cls->metaclass);
	}

	/* gnustep-2.0 non-fragile ivar fixup */
	__objc_fixup_ivar_offsets(cls);

	return (Class)cls;
}

Class objc_get_class(const char *name)
{
	Class cls = objc_lookup_class(name);
	if (cls == Nil) {
		printk("objc_get_class: class %s not found", name);
		return Nil;
	}
	return cls;
}

Class objc_lookupClass(const char *name)
{
	return name ? objc_lookup_class(name) : Nil;
}

const char *class_getName(Class cls)
{
	return cls ? cls->name : NULL;
}

const char *object_getClassName(id obj)
{
	return obj ? obj->isa->name : NULL;
}

Class object_getClass(id object)
{
	return object ? object->isa : Nil;
}

void object_setClass(id object, Class cls)
{
	if (object == NULL || cls == NULL) {
		printk("object_setClass: object or class is NULL");
		return;
	}
	if (cls->info & objc_class_flag_meta) {
		printk("object_setClass: cannot set class to a metaclass");
		return;
	}
	object->isa = cls;
}

BOOL object_isKindOfClass(id object, Class cls)
{
	if (object == nil) {
		return NO;
	}
	if (cls == Nil) {
		printk("object_isKindOfClass: class is Nil");
		return NO;
	}
	Class objClass = object->isa;
	while (objClass != Nil) {
		if (objClass == cls) {
			return YES;
		}
		objClass = objClass->superclass;
	}
	return NO;
}

size_t class_getInstanceSize(Class cls)
{
	return cls ? cls->instance_size : 0;
}

Class object_getSuperclass(id obj)
{
	return obj ? obj->isa->superclass : Nil;
}

Class class_getSuperclass(Class cls)
{
	return cls ? cls->superclass : Nil;
}

void objc_copyPropertyStruct(void *dest, void *src, ptrdiff_t size, BOOL atomic, BOOL strong)
{
	if (atomic) {
		static struct k_spinlock lck;
		K_SPINLOCK(&lck) {
			memcpy(dest, src, size);
		}
	} else {
		memcpy(dest, src, size);
	}
}

void objc_getPropertyStruct(void *dest, void *src, ptrdiff_t size, BOOL atomic, BOOL strong)
{
	if (atomic) {
		static struct k_spinlock lck;
		K_SPINLOCK(&lck) {
			memcpy(dest, src, size);
		}
	} else {
		memcpy(dest, src, size);
	}
}

void objc_setPropertyStruct(void *dest, void *src, ptrdiff_t size, BOOL atomic, BOOL strong)
{
	if (atomic) {
		static struct k_spinlock lck;
		K_SPINLOCK(&lck) {
			memcpy(dest, src, size);
		}
	} else {
		memcpy(dest, src, size);
	}
}
