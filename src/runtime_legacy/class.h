#pragma once
#include "api.h"
#include <objc/objc.h>

typedef struct objc_class objc_class_t;

/*
 * Initializes the Objective-C runtime class table
 */
void __objc_class_init(void);

/*
 * Register a class in the Objective-C runtime.
 */
void __objc_class_register(objc_class_t *cls);

/*
 * Look up a class by name in the class table (no method resolution).
 * Returns NULL if the class is not found.
 */
objc_class_t *__objc_lookup_class(const char *name);

/*
 * Lookup a class by name in the Objective-C runtime.
 * Resolves methods and ivar offsets if not yet done.
 * Returns Nil if the class is not found.
 */
Class objc_lookup_class(const char *name);

/*
 * Register a list of methods for a class.
 */
void __objc_class_register_method_list(objc_class_t *cls,
				       struct objc_method_list *ml);
