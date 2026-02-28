/**
 * @file helpers.m
 * @brief ObjC helper classes for refcount tests.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>
#include <objc/arc.h>
#include <objc/runtime.h>

/* ── Global dealloc tracking ────────────────────────────────────── */

int g_dealloc_count = 0;
int g_dealloc_order[16];
int g_dealloc_order_idx = 0;

/* ── TestSensor: Object subclass with tag ─────────────────────── */

@interface TestSensor : Object {
	int _tag;
}
- (id)initWithTag:(int)tag;
- (int)tag;
@end

@implementation TestSensor

- (id)initWithTag:(int)tag
{
	self = [super init];
	if (self) {
		_tag = tag;
	}
	return self;
}

- (int)tag
{
	return _tag;
}

- (void)dealloc
{
	g_dealloc_count++;
	if (g_dealloc_order_idx < 16) {
		g_dealloc_order[g_dealloc_order_idx++] = _tag;
	}
}

@end

/* ── C-callable helpers ─────────────────────────────────────────── */

__attribute__((ns_returns_retained)) id test_create_sensor(int tag)
{
	return [[TestSensor alloc] initWithTag:tag];
}

id test_retain(__unsafe_unretained id obj)
{
	return objc_retain(obj);
}

void test_release(__unsafe_unretained id obj)
{
	objc_release(obj);
}

id test_autorelease(__unsafe_unretained id obj)
{
	return objc_autorelease(obj);
}

unsigned int test_retainCount(__unsafe_unretained id obj)
{
	return __objc_refcount_get(obj);
}

void test_dealloc_obj(__unsafe_unretained id obj)
{
	objc_release(obj);
}

void test_reset_dealloc_tracking(void)
{
	g_dealloc_count = 0;
	g_dealloc_order_idx = 0;
	for (int i = 0; i < 16; i++) {
		g_dealloc_order[i] = 0;
	}
}

void *test_pool_push(void)
{
	return objc_autoreleasePoolPush();
}

void test_pool_pop(void *pool)
{
	objc_autoreleasePoolPop(pool);
}

int test_get_tag(id obj)
{
	return [(TestSensor *)obj tag];
}
