/*
 * Per-class dispatch table cache.
 *
 * Per-class sized dtables: static registry populated by OZ_DEFINE_DTABLE
 * at SYS_INIT priority 97, heap fallback for unregistered classes.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#include "dtable.h"

#ifdef CONFIG_OBJZ_DISPATCH_CACHE

#include <objc/dtable.h>
#include <objc/malloc.h>
#include <objc/table_sizes.h>
#include <string.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>

LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

#define DTABLE_DEFAULT_SIZE 8
#define DTABLE_DEFAULT_MASK (DTABLE_DEFAULT_SIZE - 1)

/* ── Registry: maps class names to pre-allocated dtables ────────── */

struct dtable_registry_entry {
	const char *class_name;
	struct objc_dtable *cls_dt;
	struct objc_dtable *meta_dt;
};

static struct dtable_registry_entry
	_dtable_registry[CONFIG_OBJZ_DISPATCH_CACHE_REGISTRY_SIZE];
static int _dtable_registry_count;

/* Class table (defined in class.c) */
extern objc_class_t *class_table[];

void __objc_dtable_register(const char *class_name,
			    struct objc_dtable *cls_dt,
			    struct objc_dtable *meta_dt)
{
	if (_dtable_registry_count >= CONFIG_OBJZ_DISPATCH_CACHE_REGISTRY_SIZE) {
		LOG_WRN("dtable registry full, class %s not registered", class_name);
		return;
	}
	_dtable_registry[_dtable_registry_count].class_name = class_name;
	_dtable_registry[_dtable_registry_count].cls_dt = cls_dt;
	_dtable_registry[_dtable_registry_count].meta_dt = meta_dt;
	_dtable_registry_count++;
}

static struct objc_dtable *__objc_dtable_find_static(objc_class_t *cls)
{
	for (int i = 0; i < _dtable_registry_count; i++) {
		if (strcmp(_dtable_registry[i].class_name, cls->name) == 0) {
			if (cls->info & objc_class_flag_meta) {
				return _dtable_registry[i].meta_dt;
			}
			return _dtable_registry[i].cls_dt;
		}
	}
	return NULL;
}

/* ── Hash function ──────────────────────────────────────────────── */

static inline uint32_t __objc_dtable_hash(const char *sel_name, uint32_t mask)
{
	uintptr_t p = (uintptr_t)sel_name;
	return (uint32_t)((p >> 2) ^ (p >> 11)) & mask;
}

/* ── Heap fallback allocation ───────────────────────────────────── */

static struct objc_dtable *__objc_dtable_alloc_heap(void)
{
	size_t sz = offsetof(struct objc_dtable, entries) +
		    sizeof(struct objc_dtable_entry) * DTABLE_DEFAULT_SIZE;
	struct objc_dtable *dt = objc_malloc(sz);

	if (dt == NULL) {
		return NULL;
	}
	memset(dt, 0, sz);
	dt->mask = DTABLE_DEFAULT_MASK;
	return dt;
}

/* ── Public API ─────────────────────────────────────────────────── */

IMP __objc_dtable_lookup(objc_class_t *cls, const char *sel_name)
{
	if (cls->dtable == NULL || sel_name == NULL) {
		return NULL;
	}

	struct objc_dtable *dt = (struct objc_dtable *)cls->dtable;
	uint32_t mask = dt->mask;
	uint32_t size = mask + 1;
	uint32_t hash = __objc_dtable_hash(sel_name, mask);

	for (uint32_t i = 0; i < size; i++) {
		uint32_t idx = (hash + i) & mask;
		struct objc_dtable_entry *e = &dt->entries[idx];

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

	/* Lazy allocation: static registry first, heap fallback */
	if (cls->dtable == NULL) {
		struct objc_dtable *dt = __objc_dtable_find_static(cls);

		if (dt == NULL) {
			LOG_WRN("dispatch cache: heap fallback for class %s",
				cls->name);
			dt = __objc_dtable_alloc_heap();
		}
		if (dt == NULL) {
			return false;
		}
		/*
		 * Write the dtable pointer with a barrier so that
		 * concurrent readers on the lookup path see either
		 * NULL (miss) or a fully initialized table.
		 */
		__DMB();
		cls->dtable = (void **)dt;
	}

	struct objc_dtable *dt = (struct objc_dtable *)cls->dtable;
	uint32_t mask = dt->mask;
	uint32_t size = mask + 1;
	uint32_t hash = __objc_dtable_hash(sel_name, mask);

	for (uint32_t i = 0; i < size; i++) {
		uint32_t idx = (hash + i) & mask;
		struct objc_dtable_entry *e = &dt->entries[idx];

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
		struct objc_dtable *dt = (struct objc_dtable *)cls->dtable;
		uint32_t size = dt->mask + 1;

		memset(dt->entries, 0,
		       sizeof(struct objc_dtable_entry) * size);
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
