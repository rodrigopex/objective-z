/*
 * Global flat dispatch table.
 *
 * Single 1D table indexed by (class_id << SEL_BITS) | sel_id.
 * Inheritance is flattened at init time: parent rows are copied
 * to child rows before own methods overwrite.  O(1) lookup,
 * deterministic timing, no superclass chain walk.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#include "dispatch.h"

#ifdef CONFIG_OBJZ_FLAT_DISPATCH

#include "hash.h"
#include <objc/dispatch.h>
#include <objc/objc.h>
#include <objc/table_sizes.h>
#include <string.h>
#include <zephyr/kernel.h>
#include <zephyr/logging/log.h>
#include <zephyr/sys/printk.h>

LOG_MODULE_DECLARE(objz, CONFIG_OBJZ_LOG_LEVEL);

/* ── Generated selector init table (from dispatch_init.c) ───────── */

extern const struct objz_sel_init_entry __objz_sel_init_table[];
extern const uint16_t __objz_sel_init_count;

/* ── External tables ────────────────────────────────────────────── */

extern objc_class_t *class_table[];
extern struct objc_hashitem hash_table[];

/* ── Flat dispatch table (BSS) ──────────────────────────────────── */

static IMP __objz_dispatch_table[CONFIG_OBJZ_FLAT_DISPATCH_TABLE_SIZE];

BUILD_ASSERT(sizeof(__objz_dispatch_table) <= CONFIG_OBJZ_FLAT_DISPATCH_MAX_BYTES,
	     "Flat dispatch table exceeds CONFIG_OBJZ_FLAT_DISPATCH_MAX_BYTES");

/* ── Selector name → sel_id hash table (BSS) ────────────────────── */

#define SEL_MAP_SIZE (CONFIG_OBJZ_SEL_COUNT * 2)

struct sel_id_entry {
	const char *sel_name;
	uint16_t sel_id;
};

static struct sel_id_entry __objz_sel_map[SEL_MAP_SIZE];

static inline uint32_t __objc_sel_hash(const char *name)
{
	/* djb2 string hash — content-based for open addressing */
	uint32_t h = 5381;

	for (const char *p = name; *p != '\0'; p++) {
		h = ((h << 5) + h) ^ (uint8_t)*p;
	}
	return h;
}

static void __objc_sel_map_insert(const char *name, uint16_t sel_id)
{
	uint32_t mask = SEL_MAP_SIZE - 1;
	uint32_t h = __objc_sel_hash(name) & mask;

	for (uint32_t i = 0; i < SEL_MAP_SIZE; i++) {
		uint32_t idx = (h + i) & mask;

		if (__objz_sel_map[idx].sel_name == NULL) {
			__objz_sel_map[idx].sel_name = name;
			__objz_sel_map[idx].sel_id = sel_id;
			return;
		}
	}
	LOG_WRN("sel_map full, cannot insert %s", name);
}

static uint16_t __objc_sel_to_id(const char *name)
{
	if (name == NULL) {
		return UINT16_MAX;
	}

	uint32_t mask = SEL_MAP_SIZE - 1;
	uint32_t h = __objc_sel_hash(name) & mask;

	for (uint32_t i = 0; i < SEL_MAP_SIZE; i++) {
		uint32_t idx = (h + i) & mask;

		if (__objz_sel_map[idx].sel_name == NULL) {
			return UINT16_MAX;
		}
		if (__objz_sel_map[idx].sel_name == name ||
		    strcmp(__objz_sel_map[idx].sel_name, name) == 0) {
			return __objz_sel_map[idx].sel_id;
		}
	}
	return UINT16_MAX;
}

/* ── Lookup ─────────────────────────────────────────────────────── */

IMP __objc_dispatch_lookup(objc_class_t *cls, const char *sel_name)
{
	uint16_t sel_id = __objc_sel_to_id(sel_name);
	if (sel_id == UINT16_MAX) {
		return NULL;
	}

	size_t class_id = (size_t)(uintptr_t)cls->dtable;
	size_t idx = (class_id << CONFIG_OBJZ_SEL_BITS) | sel_id;

	return __objz_dispatch_table[idx];
}

/* ── Table fill (one-shot) ──────────────────────────────────────── */

void __objc_dispatch_table_fill(void)
{
	/* Phase 1: Build sel_name → sel_id hash table from generated init table */
	uint16_t next_sel_id = __objz_sel_init_count;

	for (uint16_t i = 0; i < __objz_sel_init_count; i++) {
		__objc_sel_map_insert(__objz_sel_init_table[i].name,
				      __objz_sel_init_table[i].sel_id);
	}

	/* Phase 2: Resolve all classes (registers methods in hash table) */
	for (int i = 0; i < CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
		if (class_table[i] != NULL &&
		    !(class_table[i]->info & objc_class_flag_resolved)) {
			objc_lookup_class(class_table[i]->name);
		}
	}

	/*
	 * Phase 2.5: Register any runtime selectors not in the init table.
	 * Handles synthesized property accessors and #ifdef-guarded methods
	 * that tree-sitter cannot see at build time.
	 */
	for (int j = 0; j < CONFIG_OBJZ_HASH_TABLE_SIZE; j++) {
		if (hash_table[j].method == NULL) {
			continue;
		}
		if (__objc_sel_to_id(hash_table[j].method) == UINT16_MAX) {
			if (next_sel_id < CONFIG_OBJZ_SEL_COUNT) {
				__objc_sel_map_insert(hash_table[j].method, next_sel_id);
				next_sel_id++;
			} else {
				LOG_WRN("flat_dispatch: no room for runtime sel '%s'",
					hash_table[j].method);
			}
		}
	}

	/* Phase 3: Assign class_id = class_table index */
	for (int i = 0; i < CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
		if (class_table[i] != NULL) {
			class_table[i]->dtable = (void **)(uintptr_t)i;
		}
	}

	/*
	 * Phase 4: Topological fill — process root classes before children.
	 * Iterative: repeat until every class is processed.
	 * Each pass fills classes whose superclass is already filled
	 * (or root classes with no superclass).
	 */
	bool filled[CONFIG_OBJZ_CLASS_TABLE_SIZE];

	memset(filled, 0, sizeof(filled));

	/* Mark empty slots as filled so they don't block progress */
	for (int i = 0; i < CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
		if (class_table[i] == NULL) {
			filled[i] = true;
		}
	}

	bool progress = true;

	while (progress) {
		progress = false;
		for (int i = 0; i < CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
			if (filled[i]) {
				continue;
			}

			objc_class_t *cls = class_table[i];
			size_t row = (size_t)i << CONFIG_OBJZ_SEL_BITS;

			/* Check if parent is already filled */
			if (cls->superclass != NULL) {
				size_t pid = (size_t)(uintptr_t)cls->superclass->dtable;

				if (!filled[pid]) {
					continue; /* parent not ready */
				}

				/* Copy parent row (inherit all methods) */
				size_t prow = pid << CONFIG_OBJZ_SEL_BITS;

				memcpy(&__objz_dispatch_table[row],
				       &__objz_dispatch_table[prow],
				       CONFIG_OBJZ_SEL_COUNT * sizeof(IMP));
			}

			/* Fill own methods from global hash table */
			for (int j = 0; j < CONFIG_OBJZ_HASH_TABLE_SIZE; j++) {
				if (hash_table[j].cls != cls) {
					continue;
				}

				uint16_t sid = __objc_sel_to_id(hash_table[j].method);

				if (sid == UINT16_MAX) {
					continue;
				}
				__objz_dispatch_table[row | sid] = hash_table[j].imp;
			}

			filled[i] = true;
			progress = true;
		}
	}

	/* Boot stats */
	int n_classes = 0;

	for (int i = 0; i < CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
		if (class_table[i] != NULL) {
			n_classes++;
		}
	}
	printk("flat_dispatch: %u B (%d classes, %u selectors)\n",
	       (unsigned)sizeof(__objz_dispatch_table), n_classes,
	       (unsigned)next_sel_id);
}

#endif /* CONFIG_OBJZ_FLAT_DISPATCH */
