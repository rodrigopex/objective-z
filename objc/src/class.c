#include "class.h"
#include "api.h"
#include "hash.h"
#include "protocol.h"
#include "zephyr/spinlock.h"
#include <objc/objc.h>
#include <string.h>
#include <zephyr/kernel.h>

#define CLASS_TABLE_SIZE 32
objc_class_t *class_table[CLASS_TABLE_SIZE + 1];

///////////////////////////////////////////////////////////////////////////////

void __objc_class_init()
{
	static BOOL init = NO;
	if (init) {
		return; // Already initialized
	}
	init = YES;

	for (int i = 0; i <= CLASS_TABLE_SIZE; i++) {
		class_table[i] = NULL;
	}
}

void __objc_class_register(objc_class_t *p)
{
	if (p == NULL || p->name == NULL) {
		return;
	}
#ifdef OBJCDEBUG
	printk("__objc_class_register %c[%s] @%p size=%lu\n",
	       p->info & objc_class_flag_meta ? '+' : '-', p->name, p, p->size);
#endif
	for (int i = 0; i < CLASS_TABLE_SIZE; i++) {
		if (class_table[i] == p) {
			// Class is already registered, nothing to do
			return;
		}
		if (class_table[i] == NULL) {
			// Found empty slot, register the class
			class_table[i] = p;

			// Register protocols for the class
			if (p->protocols != NULL) {
				__objc_protocol_list_register(p->protocols);
			}
			return;
		}

		// Check for duplicate class names
		if (strcmp(class_table[i]->name, p->name) == 0) {
			printk("Duplicate class named: %s", p->name);
		}
	}
	printk("Class table is full, cannot register class: %s", p->name);
}

/**
 * Lookup the class in the class table by name.
 * Returns the class, or Nil if not found.
 */
objc_class_t *__objc_lookup_class(const char *name)
{
	if (name == NULL) {
		return Nil;
	}
	for (int i = 0; i < CLASS_TABLE_SIZE; i++) {
		if (class_table[i] == NULL || class_table[i]->name == NULL) {
			continue; // Skip empty slots and continue searching
		}
		if (strcmp(class_table[i]->name, name) == 0) {
			return class_table[i];
		}
	}
	return Nil; // No class found
}

/**
 * Register a list of methods for a class.
 * This function registers all methods in the method list, for the named class.
 */
void __objc_class_register_method_list(objc_class_t *cls, struct objc_method_list *ml)
{
	if (ml == NULL) {
		return; // Nothing to register
	}
	for (int i = 0; i < ml->count; i++) {
		struct objc_method *method = &ml->methods[i];
		if (method == NULL || method->name == NULL || method->imp == NULL) {
			continue; // Skip invalid methods
		}
#ifdef OBJCDEBUG
		sys_printf("    %c[%s %s] types=%s imp=%p\n",
			   cls->info & objc_class_flag_meta ? '+' : '-', cls->name, method->name,
			   method->types, method->imp);
#endif
		// We register the version WITH the type
		struct objc_hashitem *item =
			__objc_hash_register(cls, method->name, method->types, method->imp);
		if (item == NULL) {
			printk("TODO: Failed to register method %s in class %s\n", method->name,
			       cls->name);
			return;
		}

		// We register the version WITHOUT the type
		item = __objc_hash_register(cls, method->name, NULL, method->imp);
		if (item == NULL) {
			printk("TODO: Failed to register method %s in class %s\n", method->name,
			       cls->name);
			return;
		}
	}
}

/**
 * Register methods in the class for lookup objc_msg_lookup and
 * objc_msg_lookup_super.
 */
void __objc_class_register_methods(objc_class_t *p)
{
	// Check if the class is already resolved
	if (p->info & objc_class_flag_resolved) {
		return; // Already resolved
	}

	// Mark the class as being resolved to prevent circular resolution
	p->info |= objc_class_flag_resolved;

#ifdef OBJCDEBUG
	sys_printf("  __objc_class_register_methods %c[%s] @%p size=%lu\n",
		   p->info & objc_class_flag_meta ? '+' : '-', p->name, p, p->size);
#endif

	// Enumerate the class's methods and resolve them
	for (struct objc_method_list *ml = p->methods; ml != NULL; ml = ml->next) {
		__objc_class_register_method_list(p, ml);
	}

	// Resolve the superclass
	if (p->superclass != NULL) {
		// Check if superclass is already a resolved class pointer vs a string
		// If it's a metaclass and superclass looks like a class pointer, skip
		// string resolution
		if (p->info & objc_class_flag_meta) {
			// For metaclasses, superclass should already be set by objc_lookup_class
			// Skip string-based resolution
		} else {
			// For instance classes, resolve the string-based superclass
			Class superclass = objc_lookup_class((const char *)p->superclass);
			if (superclass == Nil) {
				printk("Superclass %s not found for class %s",
				       (const char *)p->superclass, p->name);
				return;
			}
			p->superclass = superclass; // Update the superclass pointer
		}
	}
}

///////////////////////////////////////////////////////////////////////////////

/**
 * Lookup the class with the specified name, and resolve all the methods for the
 * class and metaclass if that has not yet been done. If the class is not found,
 * Nil is returned.
 */
Class objc_lookup_class(const char *name)
{
	// Lookup the class by name
	objc_class_t *cls = __objc_lookup_class(name);
	if (cls == Nil) {
		return Nil;
	}
#ifdef OBJCDEBUG
	sys_printf("objc_lookup_class %c[%s] @%p\n", cls->info & objc_class_flag_meta ? '+' : '-',
		   name, cls);
#endif

	// Resolve the class
	if (cls->info & objc_class_flag_resolved) {
		// Check if metaclass also needs to be resolved
		if (cls->metaclass != NULL && !(cls->metaclass->info & objc_class_flag_resolved)) {
			// Need to resolve the metaclass
		} else {
			return (Class)cls; // Both class and metaclass are resolved
		}
	}

	// Resolve the class methods (including superclass methods)
	__objc_class_register_methods(cls);

	// Set up the metaclass superclass BEFORE registering metaclass methods
	if (cls->metaclass != NULL && cls->superclass != NULL) {
		cls->metaclass->superclass = cls->superclass->metaclass;
	}

	// Resolve the metaclass methods (now that superclass is set)
	if (cls->metaclass != NULL) {
		__objc_class_register_methods(cls->metaclass);
	}

	// Return the class pointer
	return (Class)cls;
}

///////////////////////////////////////////////////////////////////////////////

/**
 * Class lookup function. Panics if the class is not found.
 */
Class objc_get_class(const char *name)
{
	Class cls = objc_lookup_class(name);
	if (cls == Nil) {
		printk("objc_get_class: class %s not found", name);
		return Nil;
	}
	return cls;
}

/**
 * Looks up the class with the specified name. Returns Nil if the class is not
 * found.
 */
Class objc_lookupClass(const char *name)
{
	return name ? objc_lookup_class(name) : Nil;
}

/**
 * Returns the name of the class, or NULL if the class is Nil.
 */
const char *class_getName(Class cls)
{
	return cls ? cls->name : NULL;
}

/**
 * Returns the name of the class of the object, or NULL if the object is nil.
 */
const char *object_getClassName(id obj)
{
	return obj ? obj->isa->name : NULL;
}

/**
 * Returns the class of the object. Returns Nil if the object is nil.
 */
Class object_getClass(id object)
{
	return object ? object->isa : Nil;
}

/**
 * Sets the class of the object to the specified class.
 */
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
	object->isa = cls; // Set the class of the object
}

/**
 * Checks if an instance class matches, or subclass of another class.
 */
BOOL object_isKindOfClass(id object, Class cls)
{
	if (object == nil) {
		return NO;
	}
	if (cls == Nil) {
		printk("object_isKindOfClass: class is Nil");
		return NO;
	}
	Class objClass = object->isa; // Get the class of the object
	while (objClass != Nil) {
		if (objClass == cls) {
			return YES; // Found a match
		}
		objClass = objClass->superclass; // Move up the superclass chain
	}
	return NO; // No match found
}

/**
 * Returns the size of an instance of the named class, in bytes. Returns 0 if
 * the class is Nil
 */
size_t class_getInstanceSize(Class cls)
{
	return cls ? cls->size : 0;
}

/**
 * Returns the superclass of an instance, or Nil if it is a root class
 */
Class object_getSuperclass(id obj)
{
	return obj ? obj->isa->superclass : Nil;
}

/**
 * Returns the superclass of a class, or Nil if it is a root class
 */
Class class_getSuperclass(Class cls)
{
	return cls ? cls->superclass : Nil;
}

/*
 * Structure copy function.  This is provided for compatibility with the Apple
 * APIs (it's an ABI function, so it's semi-public), but it's a bad design so
 * it's not used. The problem is that it does not identify which of the
 * pointers corresponds to the object, which causes some excessive locking to
 * be needed.
 * source
 * https://github.com/charlieMonroe/libobjc-kern/blob/a649de414d83145555e4ef635bdcd0affb5fbb1f/property.c#L166
 */
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

/*
 * Get property structure function.  Copies a structure from an ivar to another
 * variable.  Locks on the address of src.
 */
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

/*
 * Set property structure function.  Copes a structure to an ivar.  Locks on
 * dest.
 */
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
