/*
 * Global flat dispatch table — internal header.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#pragma once
#include "class.h"

#ifdef CONFIG_OBJZ_FLAT_DISPATCH

/**
 * Look up an IMP in the global flat dispatch table.
 * O(1) via (class_id << SEL_BITS) | sel_id.
 * Returns NULL if the selector is unknown or not
 * implemented by this class.
 */
IMP __objc_dispatch_lookup(objc_class_t *cls, const char *sel_name);

/**
 * Build the flat dispatch table (one-shot).
 * Called on first message send after category loading.
 * Resolves all classes, assigns class IDs, builds the
 * sel_name→sel_id hash table, and flattens inheritance
 * by copying parent rows before filling own methods.
 */
void __objc_dispatch_table_fill(void);

#endif /* CONFIG_OBJZ_FLAT_DISPATCH */
