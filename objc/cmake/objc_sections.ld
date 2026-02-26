/*
 * Linker sections for gnustep-2.0 ObjC metadata.
 *
 * All ObjC metadata is placed inside a single data section so that
 * Zephyr's ROM-to-RAM copy initializes it at boot.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

#include <zephyr/linker/linker-defs.h>

SECTION_DATA_PROLOGUE(__objc_metadata,,)
{
	__start___objc_selectors = .;
	KEEP(*(__objc_selectors))
	__stop___objc_selectors = .;

	__start___objc_cats = .;
	KEEP(*(__objc_cats))
	__stop___objc_cats = .;

	__start___objc_protocols = .;
	KEEP(*(__objc_protocols))
	__stop___objc_protocols = .;

	__start___objc_protocol_refs = .;
	KEEP(*(__objc_protocol_refs))
	__stop___objc_protocol_refs = .;

	__start___objc_class_aliases = .;
	KEEP(*(__objc_class_aliases))
	__stop___objc_class_aliases = .;

	__start___objc_constant_string = .;
	KEEP(*(__objc_constant_string))
	__stop___objc_constant_string = .;

	__start___objc_classes = .;
	KEEP(*(__objc_classes))
	__stop___objc_classes = .;

	__start___objc_class_refs = .;
	KEEP(*(__objc_class_refs))
	__stop___objc_class_refs = .;
} GROUP_DATA_LINK_IN(RAMABLE_REGION, ROMABLE_REGION)
