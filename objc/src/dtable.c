/*
 * Per-class dispatch table cache.
 *
 * Two-tier allocation: static BSS pool for the first N classes,
 * heap fallback (objc_malloc) for overflow.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#include "dtable.h"

#ifdef CONFIG_OBJZ_DISPATCH_CACHE

#include <objc/malloc.h>
#include <string.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>

LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

#define DTABLE_MASK (CONFIG_OBJZ_DISPATCH_TABLE_SIZE - 1)

struct objc_dtable_entry {
	const char *sel_name;
	IMP imp;
};

/* Tier 1: static pool in BSS */
static struct objc_dtable_entry
	_dtable_pool[CONFIG_OBJZ_DISPATCH_CACHE_STATIC_COUNT]
		    [CONFIG_OBJZ_DISPATCH_TABLE_SIZE];
static int _dtable_pool_next;

/* Class table (defined in class.c) */
extern objc_class_t *class_table[];

static inline uint32_t __objc_dtable_hash(const char *sel_name)
{
	uintptr_t p = (uintptr_t)sel_name;
	return (uint32_t)((p >> 2) ^ (p >> 11)) & DTABLE_MASK;
}

static struct objc_dtable_entry *__objc_dtable_alloc(void)
{
	if (_dtable_pool_next < CONFIG_OBJZ_DISPATCH_CACHE_STATIC_COUNT) {
		struct objc_dtable_entry *block = _dtable_pool[_dtable_pool_next];

		_dtable_pool_next++;
		return block;
	}

	LOG_WRN("dispatch cache static pool exhausted, falling back to heap");
	struct objc_dtable_entry *block =
		objc_malloc(sizeof(struct objc_dtable_entry) * CONFIG_OBJZ_DISPATCH_TABLE_SIZE);
	return block;
}

IMP __objc_dtable_lookup(objc_class_t *cls, const char *sel_name)
{
	if (cls->dtable == NULL || sel_name == NULL) {
		return NULL;
	}

	struct objc_dtable_entry *entries = (struct objc_dtable_entry *)cls->dtable;
	uint32_t hash = __objc_dtable_hash(sel_name);

	for (int i = 0; i < CONFIG_OBJZ_DISPATCH_TABLE_SIZE; i++) {
		uint32_t idx = (hash + (uint32_t)i) & DTABLE_MASK;
		struct objc_dtable_entry *e = &entries[idx];

		if (e->sel_name == NULL) {
			return NULL;
		}
		if (e->sel_name == sel_name) {
			return e->imp;
		}
		if (strcmp(e->sel_name, sel_name) == 0) {
			return e->imp;
		}
	}
	return NULL;
}

bool __objc_dtable_insert(objc_class_t *cls, const char *sel_name, IMP imp)
{
	if (sel_name == NULL || imp == NULL) {
		return false;
	}

	/* Lazy allocation via tiered allocator */
	if (cls->dtable == NULL) {
		struct objc_dtable_entry *block = __objc_dtable_alloc();

		if (block == NULL) {
			return false;
		}
		memset(block, 0, sizeof(struct objc_dtable_entry) * CONFIG_OBJZ_DISPATCH_TABLE_SIZE);
		/*
		 * Write the dtable pointer with a barrier so that
		 * concurrent readers on the lookup path see either
		 * NULL (miss) or a fully zeroed table.
		 */
		__DMB();
		cls->dtable = (void **)block;
	}

	struct objc_dtable_entry *entries = (struct objc_dtable_entry *)cls->dtable;
	uint32_t hash = __objc_dtable_hash(sel_name);

	for (int i = 0; i < CONFIG_OBJZ_DISPATCH_TABLE_SIZE; i++) {
		uint32_t idx = (hash + (uint32_t)i) & DTABLE_MASK;
		struct objc_dtable_entry *e = &entries[idx];

		if (e->sel_name == NULL) {
			/* Write IMP before sel_name so readers never see a
			 * non-NULL sel_name with a stale IMP. */
			e->imp = imp;
			__DMB();
			e->sel_name = sel_name;
			return true;
		}
		if (e->sel_name == sel_name || strcmp(e->sel_name, sel_name) == 0) {
			e->imp = imp;
			return true;
		}
	}

	LOG_WRN("dispatch cache full for class %s", cls->name);
	return false;
}

void __objc_dtable_flush(objc_class_t *cls)
{
	if (cls->dtable != NULL) {
		memset(cls->dtable, 0,
		       sizeof(struct objc_dtable_entry) * CONFIG_OBJZ_DISPATCH_TABLE_SIZE);
	}
}

void __objc_dtable_flush_all(void)
{
	for (int i = 0; i < CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
		if (class_table[i] != NULL) {
			__objc_dtable_flush(class_table[i]);
		}
	}
}

#endif /* CONFIG_OBJZ_DISPATCH_CACHE */
