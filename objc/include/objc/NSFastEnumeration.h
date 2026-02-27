/**
 * @file NSFastEnumeration.h
 * @brief Fast enumeration support for Objective-C for...in loops.
 *
 * Defines NSFastEnumerationState used by Clang's for...in codegen.
 * Collections implement countByEnumeratingWithState:objects:count:
 * to support the for (id obj in collection) syntax.
 */
#pragma once

#include <objc/runtime.h>

/**
 * @brief State structure for fast enumeration.
 *
 * Clang stack-allocates this struct and passes it to
 * countByEnumeratingWithState:objects:count: in a loop.
 */
struct NSFastEnumerationState {
	unsigned long state;
	id *itemsPtr;
	unsigned long *mutationsPtr;
	unsigned long extra[5];
};

/**
 * @brief Called by Clang when a collection is mutated during for...in.
 * @param object The collection that was mutated.
 *
 * For immutable collections this is never reached. Provided as a
 * linker stub required by Clang's fast enumeration codegen.
 */
void objc_enumerationMutation(id object);
