/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Heap allocation demo — allocWithHeap: vs slab alloc.
 * Transpiled to plain C — no ObjC runtime needed.
 */

#import <Foundation/Foundation.h>
#import "App.h"

@interface Sensor: OZObject {
	int _value;
}
- (void)setValue:(int)v;
- (int)value;
@end

@implementation Sensor

- (id)init
{
	self = [super init];
	return self;
}

- (void)setValue:(int)v
{
	_value = v;
}

- (int)value
{
	return _value;
}

- (void)dealloc
{
	OZLog("Sensor dealloc (value=%d)", _value);
}

@end

static char sHeap_buffer[2048];
static OZHeap *sHeap;

int main(void)
{
	sHeap = [[OZHeap alloc] initWithBuffer:sHeap_buffer size:sizeof(sHeap_buffer)];
	OZLog("Local heap initialized (%d bytes)", (int)sizeof(sHeap_buffer));

	OZLog("=== Heap Allocation Demo ===");

	OZHeap *appHeap = [App sharedInstance].heap;
	OZLog("app heap before: %zu bytes used", [appHeap usedBytes]);
	OZLog("local heap before: %zu bytes used", [sHeap usedBytes]);

	/* Allocate from user-provided heap */
	@autoreleasepool {
		Sensor *s = [[Sensor allocWithHeap:appHeap] init];
		[s setValue:42];
		OZLog("Sensor allocated from app heap, value=%d", [s value]);
		OZLog("app heap after alloc: %zu bytes used", [appHeap usedBytes]);
		Sensor *s2 = [[Sensor allocWithHeap:sHeap] init];
		[s2 setValue:84];
		OZLog("Sensor allocated from local heap, value=%d", [s2 value]);
		OZLog("local heap after alloc: %zu bytes used", [sHeap usedBytes]);
	}

	OZLog("app heap after free: %zu bytes used", [appHeap usedBytes]);
	OZLog("local heap after free: %zu bytes used", [sHeap usedBytes]);

	/* Allocate from system heap (nil = k_malloc on Zephyr) */
	@autoreleasepool {
		Sensor *s = [[Sensor allocWithHeap:nil] init];
		[s setValue:99];
		OZLog("Sensor allocated from system heap, value=%d", [s value]);
	}

	/* Regular slab allocation still works */
	@autoreleasepool {
		Sensor *s = [[Sensor alloc] init];
		[s setValue:7];
		OZLog("Slab sensor value=%d", [s value]);
	}

	OZLog("=== Demo complete ===");
	return 0;
}
