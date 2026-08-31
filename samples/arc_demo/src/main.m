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
@interface AppConfig: OZObject <SingletonProtocol> {
	int _refreshRate;
}
+ (instancetype)sharedInstance;
- (int)refreshRate;
@end

static AppConfig *_sharedConfig;

@implementation AppConfig
+ (void)initialize
{
	_sharedConfig = [[AppConfig alloc] init];
}
+ (instancetype)sharedInstance
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
	AppConfig *c1 = [AppConfig sharedInstance];
	AppConfig *c2 = [AppConfig sharedInstance];
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

	/*
	 * Reassignment test: storing into a strong local releases whatever it
	 * held, so this loop holds one live Sensor however many times it runs.
	 * The release happens before the next allocation, which is what lets a
	 * single slab slot serve the whole loop — with no release, the third
	 * iteration would get nil from an exhausted slab.
	 *
	 * main() is a plain C function, so this also exercises the static bar
	 * over a free function body: the loop is accepted because ARC bounds
	 * it, where accumulating into an array would still be rejected.
	 */
	OZLog("reassign loop");
	{
		/*
		 * Written `= nil` rather than left bare so this sample still
		 * builds under CONFIG_OBJZ_BACKEND_PYTHON: that backend has no
		 * emission rule for the implicit nil Clang gives a bare strong
		 * local (OZ003, ImplicitValueInitExpr). oz_static treats the
		 * two spellings identically.
		 */
		Sensor *r = nil;
		for (int i = 0; i < 3; i++) {
			r = createSensor(100 + i);
			OZLog("reassigned value=%d", [r value]);
		}
	}
	/* r is released here → the last Sensor deallocs */

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
