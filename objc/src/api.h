/*
 * gnustep-2.0 ABI structures for Objective-Z.
 *
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */
#pragma once
#include <objc/objc.h>
#include <stddef.h>
#include <stdint.h>

/*
 * gnustep-2.0 selector.
 *
 * Clang emits one { name, types } pair per unique (selector, type) combo
 * into the __objc_selectors section.  The runtime matches by name only.
 */
struct objc_selector {
	const char *name;  /* Selector name string (e.g. "init") */
	const char *types; /* Type encoding (e.g. "@8@0:4") or NULL */
};

struct objc_super {
	id receiver;
	struct objc_class *superclass;
};

struct objc_object {
	struct objc_class *isa;
};

/*
 * gnustep-2.0 ivar.
 *
 * Each ivar has a pointer to a global offset variable
 * (__objc_ivar_offset_ClassName.ivarName.typeCode) that the runtime
 * fills in during class resolution.
 */
struct objc_ivar {
	const char *name;
	const char *type;
	int *offset;      /* Pointer to the runtime-writable offset global */
	uint32_t size;    /* Size of the ivar in bytes */
	uint32_t flags;
};

struct objc_ivar_list {
	int count;
	int element_size; /* sizeof(struct objc_ivar) — for ABI versioning */
	struct objc_ivar ivars[];
};

/*
 * gnustep-2.0 method.
 *
 * Order differs from v1: { imp, selector, types } instead of { name, types, imp }.
 * The selector field points to a selector struct in __objc_selectors.
 */
struct objc_method {
	IMP imp;
	SEL selector;      /* Pointer to objc_selector in __objc_selectors */
	const char *types;
};

struct objc_method_list {
	struct objc_method_list *next;
	int count;
	int element_size; /* sizeof(struct objc_method) — for ABI versioning */
	struct objc_method methods[];
};

/*
 * gnustep-2.0 class.
 *
 * 17-field layout confirmed from Clang IR output.  Compared to v1:
 *   - Added cxx_construct / cxx_destruct (C++ ctor/dtor for ObjC++ ivars)
 *   - Removed ivar_offsets array (per-ivar offset globals instead)
 *   - Removed strong_pointers / weak_pointers (GC bitmaps)
 *   - Subclass/sibling and protocols shifted down
 */
struct objc_class {
	struct objc_class *metaclass;
	struct objc_class *superclass;
	const char *name;
	long version;
	unsigned long info;
	long instance_size;            /* Negative if non-fragile (runtime computes) */
	struct objc_ivar_list *ivars;
	struct objc_method_list *methods;
	void **dtable;
	IMP cxx_construct;             /* C++ ivar constructor (NULL for pure ObjC) */
	IMP cxx_destruct;              /* C++ ivar destructor  (NULL for pure ObjC) */
	struct objc_class *subclass_list;
	struct objc_class *sibling_cls;
	struct objc_protocol_list *protocols;
	void *extra_data;
	long abi_version;
	struct objc_property_list *properties;
};

enum objc_class_flags {
	objc_class_flag_meta = (1 << 0),        /* 0x01 — metaclass marker */
	objc_class_flag_resolved = (1 << 1),    /* 0x02 — runtime: methods registered */
	objc_class_flag_initialized = (1 << 2), /* 0x04 — runtime: +initialize sent */
};

/*
 * gnustep-2.0 category.
 *
 * Two extra fields (instance_properties, class_properties) compared to v1.
 */
struct objc_category {
	const char *name;
	const char *class_name;
	struct objc_method_list *instance_methods;
	struct objc_method_list *class_methods;
	struct objc_protocol_list *protocols;
	struct objc_property_list *instance_properties;
	struct objc_property_list *class_properties;
};

/*
 * gnustep-2.0 protocol method description.
 */
struct objc_protocol_method_description {
	SEL selector;
	const char *types;
};

struct objc_protocol_method_description_list {
	int count;
	int element_size;
	struct objc_protocol_method_description methods[];
};

/*
 * gnustep-2.0 protocol.
 *
 * isa is set to (void*)4 as a magic marker (not a real class pointer).
 * Layout: 11 fields confirmed from Clang IR.
 */
struct objc_protocol {
	void *isa;                     /* Magic: (void*)4 or (void*)1 */
	const char *name;
	struct objc_protocol_list *protocol_list;
	struct objc_protocol_method_description_list *required_instance_methods;
	struct objc_protocol_method_description_list *optional_class_methods;
	struct objc_protocol_method_description_list *required_class_methods;
	struct objc_protocol_method_description_list *optional_instance_methods;
	struct objc_property_list *optional_properties;
	struct objc_property_list *required_properties;
	void *reserved1;
	void *reserved2;
};

struct objc_protocol_list {
	struct objc_protocol_list *next;
	size_t count;
	struct objc_protocol *protocols[];
};

/*
 * gnustep-2.0 property (placeholder — runtime does not use these yet).
 */
struct objc_property {
	const char *name;
	const char *attributes;
};

struct objc_property_list {
	int count;
	int element_size;
	struct objc_property properties[];
};

/*
 * gnustep-2.0 constant string.
 *
 * Layout for @"..." literals with -fconstant-string-class=OZString.
 * Clang emits these into __objc_constant_string section.
 */
struct objc_constant_string {
	id isa;
	uint32_t flags;
	uint32_t length;
	uint32_t size;
	uint32_t hash;
	const char *data;
};

/*
 * gnustep-2.0 class alias.
 *
 * Maps alias names to class references (e.g. NSString -> OZString).
 * Placed in __objc_class_aliases section.
 */
struct objc_class_alias {
	const char *alias_name;
	struct objc_class **class_ref;
};

/*
 * gnustep-2.0 init structure.
 *
 * Passed to __objc_load() by the compiler-generated .objcv2_load_function.
 * Contains begin/end pointers for each ELF metadata section.
 * version is set to 0 initially and to UINT64_MAX after loading.
 */
struct objc_init {
	uint64_t version;
	struct objc_selector *sel_begin;
	struct objc_selector *sel_end;
	struct objc_class **cls_begin;
	struct objc_class **cls_end;
	struct objc_class **cls_ref_begin;
	struct objc_class **cls_ref_end;
	struct objc_category *cat_begin;
	struct objc_category *cat_end;
	struct objc_protocol *proto_begin;
	struct objc_protocol *proto_end;
	struct objc_protocol **proto_ref_begin;
	struct objc_protocol **proto_ref_end;
	struct objc_class_alias *alias_begin;
	struct objc_class_alias *alias_end;
	struct objc_constant_string *str_begin;
	struct objc_constant_string *str_end;
};
