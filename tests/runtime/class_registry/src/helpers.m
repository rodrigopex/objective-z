/**
 * @file helpers.m
 * @brief ObjC helper classes for class registry tests.
 */
#import <Foundation/Foundation.h>
#import <objc/objc.h>

/* ── TestVehicle: root subclass of Object ───────────────────────── */

@interface TestVehicle : Object
- (int)wheels;
@end

@implementation TestVehicle

- (int)wheels
{
	return 0;
}

@end

/* ── TestCar: subclass of TestVehicle ───────────────────────────── */

@interface TestCar : TestVehicle
- (int)wheels;
@end

@implementation TestCar

- (int)wheels
{
	return 4;
}

@end

/* ── TestBike: subclass of TestVehicle ──────────────────────────── */

@interface TestBike : TestVehicle
- (int)wheels;
@end

@implementation TestBike

- (int)wheels
{
	return 2;
}

@end

/* ── C-callable helpers ─────────────────────────────────────────── */

id test_create_vehicle(void)
{
	return [[TestVehicle alloc] init];
}

id test_create_car(void)
{
	return [[TestCar alloc] init];
}

id test_create_bike(void)
{
	return [[TestBike alloc] init];
}

void test_dealloc(id obj)
{
	[obj dealloc];
}

int test_call_wheels(id obj)
{
	return [obj wheels];
}
