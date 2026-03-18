/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * ARC (Automatic Reference Counting) demo.
 * Transpiled to plain C — no ObjC runtime needed.
 */

#import <Foundation/Foundation.h>

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
	/* ARC calls [super dealloc] automatically */
}

@end

static Sensor *createSensor(int v)
{
	Sensor *s = [[Sensor alloc] init];
	[s setValue:v];
	return s;
}

/* Singleton via +initialize — auto-called before main() */
@interface AppConfig: OZObject {
	int _refreshRate;
}
+ (instancetype)shared;
- (int)refreshRate;
@end

static AppConfig *_sharedConfig;

@implementation AppConfig
+ (void)initialize
{
	_sharedConfig = [[AppConfig alloc] init];
}
+ (instancetype)shared
{
	return _sharedConfig;
}
- (id)init
{
	self = [super init];
	_refreshRate = 60;
	return self;
}
- (int)refreshRate
{
	return _refreshRate;
}
@end

@interface Driver: OZObject {
	Sensor *_sensor;
}
- (id)init:(int)newValue;
- (Sensor *)sensor;
@end

@implementation Driver
- (id)init:(int)newValue
{
	self = [super init];
	_sensor = createSensor(newValue);
	OZLog("Driver created (sensor value=%d)", [_sensor value]);
	return self;
}
- (Sensor *)sensor
{
	return _sensor;
}
- (void)dealloc
{
	OZLog("Driver dealloc (sensor value=%d)", [_sensor value]);
}
@end

int main(void)
{
	OZLog("=== ARC Memory Management Demo ===");

	/* Singleton test: +initialize already ran via SYS_INIT */
	AppConfig *c1 = [AppConfig shared];
	AppConfig *c2 = [AppConfig shared];
	OZLog("singleton refreshRate=%d same=%s", [c1 refreshRate],
	      c1 == c2 ? "yes" : "no");

	/* Scope test: ARC releases s when it goes out of scope */
	{
		Sensor *s = createSensor(42);
		OZLog("Sensor created, value=%d", [s value]);
	}
	/* s is released here by ARC → dealloc fires */

	/* @autoreleasepool test */
	OZLog("@autoreleasepool test");
	@autoreleasepool {
		Sensor *a = createSensor(99);
		OZLog("pool sensor value=%d", [a value]);
	}
	/* pool drains, a is released → dealloc fires */

	OZLog("=== Demo main complete ===");
	return 0;
}

void arc_demo_extra_thread_entry(void *p1, void *p2, void *p3)
{
	(void)p1;
	(void)p2;
	(void)p3;
	OZLog("=== Demo Extra thread started ===");
	Driver *d = [[Driver alloc] init:250];
	[[d sensor] setValue:100];
}

K_THREAD_DEFINE(arc_demo_thread, 1024, arc_demo_extra_thread_entry,
		NULL, NULL, NULL, 7, 0, 0);
