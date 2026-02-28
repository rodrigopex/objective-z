/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Benchmark helper classes and C-callable wrappers.
 * Compiled with ARC.
 */

#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include <objc/runtime.h>
#include <objc/arc.h>

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

@end

/* ── C-callable wrapper functions ─────────────────────────────────── */

__attribute__((ns_returns_retained))
id bench_create_base(void)
{
	return [[BenchBase alloc] init];
}

__attribute__((ns_returns_retained))
id bench_create_child(void)
{
	return [[BenchChild alloc] init];
}

__attribute__((ns_returns_retained))
id bench_create_grandchild(void)
{
	return [[BenchGrandChild alloc] init];
}

__attribute__((ns_returns_retained))
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

void bench_retain(__unsafe_unretained id obj)
{
	objc_retain(obj);
}

void bench_release(__unsafe_unretained id obj)
{
	objc_release(obj);
}

void bench_dealloc(__unsafe_unretained id obj)
{
	objc_release(obj);
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
	__objc_dtable_flush((__bridge struct objc_class *)object_getClass(obj));
#endif
}

/* ── Blocks benchmark helpers ─────────────────────────────────────── */

#ifdef CONFIG_OBJZ_BLOCKS
#import <objc/blocks.h>

typedef int (^IntBlock)(void);
typedef void (^VoidBlock)(void);

/* C function pointer baseline */
static int c_func_return_42(void)
{
	return 42;
}

int (*bench_get_c_func(void))(void)
{
	return c_func_return_42;
}

/* Global block: no captures */
void *bench_get_global_block(void)
{
	IntBlock blk = ^{ return 42; };
	return (__bridge void *)blk;
}

int bench_invoke_int_block(void *blk)
{
	return ((__bridge IntBlock)blk)();
}

/* Block descriptor sizes for memory comparison */
unsigned long bench_block_size_int_capture(void)
{
	int val = 99;
	IntBlock blk = ^{ return val; };
	struct Block_layout *layout = (__bridge struct Block_layout *)blk;
	return layout->descriptor->size;
}

unsigned long bench_block_size_obj_capture(void)
{
	id obj = [[BenchBase alloc] init];
	IntBlock blk = ^{ return [obj getValue]; };
	struct Block_layout *layout = (__bridge struct Block_layout *)blk;
	return layout->descriptor->size;
}

unsigned long bench_block_size_byref(void)
{
	__block int counter = 0;
	VoidBlock blk = ^{ counter++; };
	struct Block_layout *layout = (__bridge struct Block_layout *)blk;
	(void)blk;
	return layout->descriptor->size;
}

/* Copy/release wrappers */
void *bench_copy_int_block(int value)
{
	IntBlock blk = ^{ return value; };
	return _Block_copy((__bridge const void *)blk);
}

void *bench_copy_obj_block(id obj)
{
	IntBlock blk = ^{ return [obj getValue]; };
	return _Block_copy((__bridge const void *)blk);
}

void *bench_copy_byref_block(void)
{
	__block int counter = 0;
	VoidBlock blk = ^{ counter++; };
	return _Block_copy((__bridge const void *)blk);
}

void bench_release_block(void *blk)
{
	_Block_release((const void *)blk);
}

void *bench_block_copy(void *blk)
{
	return _Block_copy((const void *)blk);
}

#endif /* CONFIG_OBJZ_BLOCKS */

/* ── Logging benchmark helpers ────────────────────────────────────── */

void bench_ozlog_simple(void)
{
	OZLog("Hello benchmark");
}

void bench_ozlog_int(void)
{
	OZLog("Value: %d", 42);
}

void bench_ozlog_string(void)
{
	OZLog("Name: %s", "test");
}

void bench_ozlog_objat(id obj)
{
	OZLog("Object: %@", obj);
}
