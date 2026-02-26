/*
 * Linker sections for gnustep-2.0 ObjC metadata.
 *
 * Clang emits ObjC metadata (selectors, classes, categories, protocols,
 * constant strings) into named ELF sections.  The gnustep-2.0 runtime
 * expects __start_ / __stop_ symbols at each section boundary, which
 * are passed to __objc_load() via struct objc_init.
 *
 * ROM sections: read-only metadata (selectors, categories, protocols,
 *               protocol refs, constant strings, class aliases).
 * RAM sections: runtime-writable pointers (classes, class refs).
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

#include <zephyr/linker/linker-defs.h>

/* ── Selectors (ROM) ─────────────────────────────────────────────── */

SECTION_PROLOGUE(__objc_selectors,,)
{
	__start___objc_selectors = .;
	KEEP(*(__objc_selectors))
	__stop___objc_selectors = .;
} GROUP_ROM_LINK_IN(RAMABLE_REGION, ROMABLE_REGION)

/* ── Categories (ROM) ────────────────────────────────────────────── */

SECTION_PROLOGUE(__objc_cats,,)
{
	__start___objc_cats = .;
	KEEP(*(__objc_cats))
	__stop___objc_cats = .;
} GROUP_ROM_LINK_IN(RAMABLE_REGION, ROMABLE_REGION)

/* ── Protocols (ROM) ─────────────────────────────────────────────── */

SECTION_PROLOGUE(__objc_protocols,,)
{
	__start___objc_protocols = .;
	KEEP(*(__objc_protocols))
	__stop___objc_protocols = .;
} GROUP_ROM_LINK_IN(RAMABLE_REGION, ROMABLE_REGION)

/* ── Protocol references (ROM) ───────────────────────────────────── */

SECTION_PROLOGUE(__objc_protocol_refs,,)
{
	__start___objc_protocol_refs = .;
	KEEP(*(__objc_protocol_refs))
	__stop___objc_protocol_refs = .;
} GROUP_ROM_LINK_IN(RAMABLE_REGION, ROMABLE_REGION)

/* ── Class aliases (ROM) ─────────────────────────────────────────── */

SECTION_PROLOGUE(__objc_class_aliases,,)
{
	__start___objc_class_aliases = .;
	KEEP(*(__objc_class_aliases))
	__stop___objc_class_aliases = .;
} GROUP_ROM_LINK_IN(RAMABLE_REGION, ROMABLE_REGION)

/* ── Constant strings (ROM) ──────────────────────────────────────── */

SECTION_PROLOGUE(__objc_constant_string,,)
{
	__start___objc_constant_string = .;
	KEEP(*(__objc_constant_string))
	__stop___objc_constant_string = .;
} GROUP_ROM_LINK_IN(RAMABLE_REGION, ROMABLE_REGION)

/* ── Classes (RAM — runtime modifies these) ──────────────────────── */

SECTION_DATA_PROLOGUE(__objc_classes,,)
{
	__start___objc_classes = .;
	KEEP(*(__objc_classes))
	__stop___objc_classes = .;
} GROUP_DATA_LINK_IN(RAMABLE_REGION, ROMABLE_REGION)

/* ── Class references (RAM — runtime modifies these) ─────────────── */

SECTION_DATA_PROLOGUE(__objc_class_refs,,)
{
	__start___objc_class_refs = .;
	KEEP(*(__objc_class_refs))
	__stop___objc_class_refs = .;
} GROUP_DATA_LINK_IN(RAMABLE_REGION, ROMABLE_REGION)
