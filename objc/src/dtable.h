/*
 * Per-class dispatch table cache.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#pragma once
#include "class.h"

#ifdef CONFIG_OBJZ_DISPATCH_CACHE

/**
 * Lookup an IMP in the class dispatch table cache.
 * Returns NULL on miss.
 */
IMP __objc_dtable_lookup(objc_class_t *cls, const char *sel_name);

/**
 * Insert a {sel_name, IMP} entry into the class dispatch table.
 * Allocates the dtable lazily on first insert (static pool first,
 * heap fallback). Returns false on allocation failure.
 */
bool __objc_dtable_insert(objc_class_t *cls, const char *sel_name, IMP imp);

/**
 * Flush all entries in a class dispatch table.
 */
void __objc_dtable_flush(objc_class_t *cls);

/**
 * Flush dispatch tables for all registered classes.
 * Called after category loading to invalidate stale entries.
 */
void __objc_dtable_flush_all(void);

#endif /* CONFIG_OBJZ_DISPATCH_CACHE */
