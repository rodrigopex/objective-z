/**
 * @file dtable.h
 * @brief Per-class dispatch table sizing for Objective-C classes.
 *
 * OZ_DEFINE_DTABLE(ClassName, cls_size, meta_size) creates
 * per-class static dispatch tables with individually sized hash
 * maps. The runtime checks the dtable registry on first message
 * send before falling back to heap allocation.
 *
 * Usage (must be in a .c file, not .m):
 *   OZ_DEFINE_DTABLE(Object, 32, 8)
 *   // 32-entry instance dtable, 8-entry metaclass dtable
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#pragma once

#include <objc/runtime.h>
#include <stdint.h>
#include <zephyr/init.h>

/**
 * @brief Single entry in a dispatch table cache.
 */
struct objc_dtable_entry {
	const char *sel_name;
	IMP imp;
};

/**
 * @brief Per-class dispatch table with embedded mask.
 *
 * Layout: [uint32_t mask][entries...].
 * The mask is (table_size - 1) for power-of-2 hash indexing.
 */
struct objc_dtable {
	uint32_t mask;
	struct objc_dtable_entry entries[];
};

/**
 * @brief Register static dtable arrays for a class and its metaclass.
 *
 * Called by OZ_DEFINE_DTABLE via SYS_INIT at priority 97.
 *
 * @param class_name  Name of the ObjC class (C string).
 * @param cls_dt      Pointer to the class dispatch table.
 * @param meta_dt     Pointer to the metaclass dispatch table.
 */
void __objc_dtable_register(const char *class_name,
			    struct objc_dtable *cls_dt,
			    struct objc_dtable *meta_dt);

/**
 * @brief Define per-class static dispatch tables.
 *
 * Creates statically-allocated dispatch tables for a class and its
 * metaclass, each sized independently. Registered at SYS_INIT
 * priority 97 (before pools at 98 and heap at 99).
 *
 * @param cls        Class name (unquoted identifier).
 * @param cls_size   Instance method cache entries (power of 2).
 * @param meta_size  Class method cache entries (power of 2).
 */
#define OZ_DEFINE_DTABLE(cls, cls_size, meta_size)                                                  \
	static struct {                                                                             \
		uint32_t mask;                                                                      \
		struct objc_dtable_entry entries[cls_size];                                          \
	} _objz_dt_##cls;                                                                           \
	static struct {                                                                             \
		uint32_t mask;                                                                      \
		struct objc_dtable_entry entries[meta_size];                                         \
	} _objz_dt_meta_##cls;                                                                      \
	static int _objz_dt_init_##cls(void)                                                        \
	{                                                                                           \
		_objz_dt_##cls.mask = (cls_size) - 1;                                               \
		_objz_dt_meta_##cls.mask = (meta_size) - 1;                                         \
		__objc_dtable_register(#cls, (struct objc_dtable *)&_objz_dt_##cls,                 \
				       (struct objc_dtable *)&_objz_dt_meta_##cls);                 \
		return 0;                                                                           \
	}                                                                                           \
	SYS_INIT(_objz_dt_init_##cls, APPLICATION, 97)
