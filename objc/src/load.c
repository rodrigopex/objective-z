/*
 * gnustep-2.0 ABI entry point.
 *
 * Clang places a pointer to .objcv2_load_function in .init_array.
 * That function calls __objc_load() with a struct objc_init containing
 * section boundary pointers for all ObjC metadata.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

#include "api.h"
#include "category.h"
#include "class.h"
#include "hash.h"
#include "protocol.h"
#include <stdint.h>
#include <string.h>
#include <zephyr/kernel.h>

/**
 * gnustep-2.0 module loader.
 *
 * Called once per compilation unit via .init_array.  Each unit gets its
 * own objc_init struct with pointers into the ELF sections that hold
 * selectors, classes, categories, protocols, class aliases, and
 * constant strings emitted by Clang.
 */
void __objc_load(struct objc_init *init)
{
	if (init == NULL) {
		return;
	}

	/* Idempotent: skip if already loaded (version set to sentinel) */
	if (init->version == UINT64_MAX) {
		return;
	}

	/* Initialize subsystems (each is idempotent) */
	__objc_class_init();
	__objc_hash_init();
	__objc_category_init();
	__objc_protocol_init();

	/* Register classes */
	for (struct objc_class **cls = init->cls_begin; cls < init->cls_end; cls++) {
		if (*cls != NULL) {
			__objc_class_register(*cls);

			/* Also register the metaclass */
			if ((*cls)->metaclass != NULL) {
				__objc_class_register((*cls)->metaclass);
			}
		}
	}

	/* Register categories.
	 * __objc_cats contains struct objc_category directly (not pointers). */
	for (struct objc_category *cat = init->cat_begin; cat < init->cat_end; cat++) {
		__objc_category_register(cat);
	}

	/* Register protocols.
	 * __objc_protocols contains struct objc_protocol directly (not pointers). */
	for (struct objc_protocol *proto = init->proto_begin; proto < init->proto_end;
	     proto++) {
		__objc_protocol_register(proto);
	}

	/* Fix constant string isa pointers.
	 * Clang emits @"..." literals with isa pointing to the external
	 * class symbol, but on our embedded target the class address is
	 * only known after __objc_class_register.  Walk the constant
	 * string section and patch each isa to the resolved OZString class. */
	objc_class_t *string_cls = __objc_lookup_class("OZString");
	if (string_cls != NULL) {
		for (struct objc_constant_string *str = init->str_begin;
		     str < init->str_end; str++) {
			str->isa = (id)string_cls;
		}
	}

	/* Class aliases: the compiler resolves these at compile time
	 * via @compatibility_alias. Skip for embedded (no dynamic lookup). */

	/* Mark as loaded */
	init->version = UINT64_MAX;
}
