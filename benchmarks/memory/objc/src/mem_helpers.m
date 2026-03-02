/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * ObjC memory benchmark helpers.
 * Compiled with ARC via objz_target_sources().
 */

#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include <objc/runtime.h>
#include <objc/arc.h>

/* ── MemBase: isa (4) + _refcount (4) = 8 bytes ──────────────── */

@interface MemBase : Object
- (void)nop;
- (int)getValue;
@end

@implementation MemBase

- (void)nop
{
}

- (int)getValue
{
	return 0;
}

@end

/* ── MemChild: MemBase (8) + _field_a (4) = 12 bytes ─────────── */

@interface MemChild : MemBase {
	int _field_a;
}
@end

@implementation MemChild
@end

/* ── MemGrandChild: MemChild (12) + _field_b (4) = 16 bytes ──── */

@interface MemGrandChild : MemChild {
	int _field_b;
}
@end

@implementation MemGrandChild
@end

/* ── C-callable wrappers ──────────────────────────────────────── */

__attribute__((ns_returns_retained))
id mem_create_base(void)
{
	return [[MemBase alloc] init];
}

__attribute__((ns_returns_retained))
id mem_create_child(void)
{
	return [[MemChild alloc] init];
}

__attribute__((ns_returns_retained))
id mem_create_grandchild(void)
{
	return [[MemGrandChild alloc] init];
}

void mem_release(id obj)
{
	objc_release(obj);
}

size_t mem_sizeof_base(void)
{
	return class_getInstanceSize([MemBase class]);
}

size_t mem_sizeof_child(void)
{
	return class_getInstanceSize([MemChild class]);
}

size_t mem_sizeof_grandchild(void)
{
	return class_getInstanceSize([MemGrandChild class]);
}
