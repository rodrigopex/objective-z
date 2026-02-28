/**
 * @file helpers.m
 * @brief ObjC helper classes for refcount tests.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>

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
	[super dealloc];
}

@end

/* ── C-callable helpers ─────────────────────────────────────────── */

id test_create_sensor(int tag)
{
	return [[TestSensor alloc] initWithTag:tag];
}

id test_retain(id obj)
{
	return [obj retain];
}

void test_release(id obj)
{
	[obj release];
}

id test_autorelease(id obj)
{
	return [obj autorelease];
}

unsigned int test_retainCount(id obj)
{
	return [obj retainCount];
}

void test_dealloc_obj(id obj)
{
	[obj release];
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
	return [[OZAutoreleasePool alloc] init];
}

void test_pool_pop(void *pool)
{
	[(OZAutoreleasePool *)pool drain];
}

int test_get_tag(id obj)
{
	return [(TestSensor *)obj tag];
}
