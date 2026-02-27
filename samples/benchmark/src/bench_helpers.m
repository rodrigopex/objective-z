/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Benchmark helper classes and C-callable wrappers.
 * Compiled with Clang (non-ARC) for manual retain/release control.
 */

#import <objc/objc.h>
#include <objc/runtime.h>

extern IMP objc_msg_lookup(id receiver, SEL selector);

/* ── BenchBase: direct methods (depth=0) ──────────────────────────── */

@interface BenchBase : Object {
	int _x;
}
- (void)nop;
- (int)getValue;
+ (void)classNop;
@end

@implementation BenchBase

- (void)nop
{
}

- (int)getValue
{
	return _x;
}

+ (void)classNop
{
}

- (void)dealloc
{
	[super dealloc];
}

@end

/* ── BenchChild: inherits from BenchBase (depth=1) ────────────────── */

@interface BenchChild : BenchBase
@end

@implementation BenchChild
@end

/* ── BenchGrandChild: inherits from BenchChild (depth=2) ──────────── */

@interface BenchGrandChild : BenchChild
@end

@implementation BenchGrandChild
@end

/* ── PooledObj: uses static pool allocation ────────────────────────── */

@interface PooledObj : Object {
	int _x;
}
- (void)nop;
@end

@implementation PooledObj

- (void)nop
{
}

- (void)dealloc
{
	[super dealloc];
}

@end

/* ── C-callable wrapper functions ─────────────────────────────────── */

id bench_create_base(void)
{
	return [[BenchBase alloc] init];
}

id bench_create_child(void)
{
	return [[BenchChild alloc] init];
}

id bench_create_grandchild(void)
{
	return [[BenchGrandChild alloc] init];
}

id bench_create_pooled(void)
{
	return [[PooledObj alloc] init];
}

void bench_nop(id obj)
{
	[obj nop];
}

int bench_get_value(id obj)
{
	return [obj getValue];
}

void bench_class_nop(void)
{
	[BenchBase classNop];
}

void bench_retain(id obj)
{
	[obj retain];
}

void bench_release(id obj)
{
	[obj release];
}

void bench_dealloc(id obj)
{
	[obj release];
}

/*
 * Get the IMP for -nop on the given object's class.
 * main.c calls this once to cache the IMP, then invokes it
 * directly in the timing loop as the C function call baseline.
 */
void (*bench_get_nop_imp(id obj))(id, SEL)
{
	SEL sel = @selector(nop);
	IMP imp = objc_msg_lookup(obj, sel);
	return (void (*)(id, SEL))imp;
}

/*
 * Return the -nop selector for use with direct IMP calls.
 */
SEL bench_get_nop_sel(void)
{
	return @selector(nop);
}

/*
 * class_respondsToSelector hit (YES) — selector exists.
 */
BOOL bench_responds_to_nop(id obj)
{
	return class_respondsToSelector(object_getClass(obj), @selector(nop));
}

/*
 * class_respondsToSelector miss (NO) — selector does not exist.
 * Measures worst-case hash table walk.
 */
BOOL bench_responds_to_missing(id obj)
{
	return class_respondsToSelector(object_getClass(obj),
	                                @selector(nonExistentMethod));
}

Class bench_get_class(id obj)
{
	return object_getClass(obj);
}

/*
 * Flush the dispatch cache for the given object's class.
 * Used to measure cold-cache dispatch overhead.
 */
void bench_flush_cache(id obj)
{
	(void)obj;
#ifdef CONFIG_OBJZ_DISPATCH_CACHE
	extern void __objc_dtable_flush(struct objc_class *cls);
	__objc_dtable_flush(object_getClass(obj));
#endif
}
