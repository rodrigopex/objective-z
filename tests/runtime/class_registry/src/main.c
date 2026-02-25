/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for class registry (class.c) and introspection APIs.
 */
#include <string.h>
#include <zephyr/ztest.h>
#include <objc/runtime.h>

/* ── External ABI structures (from api.h) ───────────────────────── */

struct objc_selector {
	void *sel_id;
	char *sel_type;
};

/* ── ObjC helpers (defined in helpers.m) ────────────────────────── */

extern id test_create_vehicle(void);
extern id test_create_car(void);
extern id test_create_bike(void);
extern void test_dealloc(id obj);
extern int test_call_wheels(id obj);

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(class_registry, NULL, NULL, NULL, NULL, NULL);

/* objc_lookupClass with a known class returns non-Nil */
ZTEST(class_registry, test_lookup_existing)
{
	Class cls = objc_lookupClass("Object");

	zassert_not_null(cls, "objc_lookupClass(\"Object\") should return non-Nil");
}

/* objc_lookupClass with an unknown name returns Nil */
ZTEST(class_registry, test_lookup_missing)
{
	Class cls = objc_lookupClass("NoSuchClass");

	zassert_is_null(cls, "objc_lookupClass(\"NoSuchClass\") should return Nil");
}

/* objc_lookupClass with NULL returns Nil */
ZTEST(class_registry, test_lookupClass_null)
{
	Class cls = objc_lookupClass(NULL);

	zassert_is_null(cls, "objc_lookupClass(NULL) should return Nil");
}

/* class_getName returns the correct name for a valid class and NULL for Nil */
ZTEST(class_registry, test_class_getName)
{
	Class cls = objc_lookupClass("Object");
	const char *name = class_getName(cls);

	zassert_not_null(name, "class_getName should return non-NULL for valid class");
	zassert_mem_equal(name, "Object", 6, "class_getName should return \"Object\"");

	const char *nil_name = class_getName(Nil);

	zassert_is_null(nil_name, "class_getName(Nil) should return NULL");
}

/* object_getClassName with a valid object and nil */
ZTEST(class_registry, test_object_getClassName)
{
	id car = test_create_car();

	zassert_not_null(car, "alloc should succeed");

	const char *name = object_getClassName(car);

	zassert_not_null(name, "object_getClassName should return non-NULL");
	zassert_mem_equal(name, "TestCar", 7,
			  "object_getClassName should return \"TestCar\"");

	const char *nil_name = object_getClassName(nil);

	zassert_is_null(nil_name, "object_getClassName(nil) should return NULL");

	test_dealloc(car);
}

/* object_getClass with a valid object and nil */
ZTEST(class_registry, test_object_getClass)
{
	id car = test_create_car();

	zassert_not_null(car, "alloc should succeed");

	Class cls = object_getClass(car);

	zassert_not_null(cls, "object_getClass should return non-Nil");

	const char *name = class_getName(cls);

	zassert_mem_equal(name, "TestCar", 7,
			  "object_getClass should return TestCar class");

	Class nil_cls = object_getClass(nil);

	zassert_is_null(nil_cls, "object_getClass(nil) should return Nil");

	test_dealloc(car);
}

/* object_setClass swaps the class of an object */
ZTEST(class_registry, test_object_setClass)
{
	id vehicle = test_create_vehicle();

	zassert_not_null(vehicle, "alloc should succeed");

	Class car_cls = objc_lookupClass("TestCar");

	zassert_not_null(car_cls, "TestCar class should exist");

	object_setClass(vehicle, car_cls);

	Class new_cls = object_getClass(vehicle);

	zassert_equal(new_cls, car_cls,
		      "object_getClass should return TestCar after setClass");

	test_dealloc(vehicle);
}

/* object_setClass with NULL arguments doesn't crash */
ZTEST(class_registry, test_object_setClass_null)
{
	object_setClass(NULL, NULL);
	/* If we reach here without crashing, the test passes */
}

/* object_isKindOfClass: direct class match */
ZTEST(class_registry, test_isKindOfClass_direct)
{
	id car = test_create_car();
	Class car_cls = objc_lookupClass("TestCar");

	zassert_true(object_isKindOfClass(car, car_cls),
		     "car should be kind of TestCar");

	test_dealloc(car);
}

/* object_isKindOfClass: superclass match */
ZTEST(class_registry, test_isKindOfClass_super)
{
	id car = test_create_car();
	Class vehicle_cls = objc_lookupClass("TestVehicle");

	zassert_true(object_isKindOfClass(car, vehicle_cls),
		     "car should be kind of TestVehicle (superclass)");

	test_dealloc(car);
}

/* object_isKindOfClass: root class match */
ZTEST(class_registry, test_isKindOfClass_root)
{
	id car = test_create_car();
	Class obj_cls = objc_lookupClass("Object");

	zassert_true(object_isKindOfClass(car, obj_cls),
		     "car should be kind of Object (root class)");

	test_dealloc(car);
}

/* object_isKindOfClass: nil object returns NO */
ZTEST(class_registry, test_isKindOfClass_nil)
{
	Class car_cls = objc_lookupClass("TestCar");

	zassert_false(object_isKindOfClass(nil, car_cls),
		      "nil object should not be kind of any class");
}

/* object_isKindOfClass: unrelated class returns NO */
ZTEST(class_registry, test_isKindOfClass_unrelated)
{
	id bike = test_create_bike();
	Class car_cls = objc_lookupClass("TestCar");

	zassert_false(object_isKindOfClass(bike, car_cls),
		      "bike should not be kind of TestCar");

	test_dealloc(bike);
}

/* class_getInstanceSize: valid class returns > 0, Nil returns 0 */
ZTEST(class_registry, test_getInstanceSize)
{
	Class cls = objc_lookupClass("TestCar");
	size_t size = class_getInstanceSize(cls);

	zassert_true(size > 0, "class_getInstanceSize should return > 0 for valid class");

	size_t nil_size = class_getInstanceSize(Nil);

	zassert_equal(nil_size, 0, "class_getInstanceSize(Nil) should return 0");
}

/* class_getSuperclass: TestCar -> TestVehicle, Object -> Nil */
ZTEST(class_registry, test_getSuperclass)
{
	Class car_cls = objc_lookupClass("TestCar");
	Class vehicle_cls = objc_lookupClass("TestVehicle");
	Class obj_cls = objc_lookupClass("Object");

	Class super_of_car = class_getSuperclass(car_cls);

	zassert_equal(super_of_car, vehicle_cls,
		      "superclass of TestCar should be TestVehicle");

	Class super_of_object = class_getSuperclass(obj_cls);

	zassert_is_null(super_of_object,
			"superclass of Object should be Nil");
}

/* object_getSuperclass: car instance -> TestVehicle class */
ZTEST(class_registry, test_object_getSuperclass)
{
	id car = test_create_car();
	Class vehicle_cls = objc_lookupClass("TestVehicle");

	Class super_cls = object_getSuperclass(car);

	zassert_equal(super_cls, vehicle_cls,
		      "object_getSuperclass of car should be TestVehicle");

	test_dealloc(car);
}

/* objc_copyPropertyStruct / objc_getPropertyStruct / objc_setPropertyStruct */
ZTEST(class_registry, test_property_struct_copy)
{
	struct test_point {
		int x;
		int y;
		int z;
	};

	struct test_point src = { .x = 10, .y = 20, .z = 30 };
	struct test_point dest;

	/* Test objc_copyPropertyStruct (atomic) */
	memset(&dest, 0, sizeof(dest));
	objc_copyPropertyStruct(&dest, &src, sizeof(struct test_point), YES, NO);
	zassert_mem_equal(&dest, &src, sizeof(struct test_point),
			  "atomic copyPropertyStruct should produce identical content");

	/* Test objc_copyPropertyStruct (non-atomic) */
	memset(&dest, 0, sizeof(dest));
	objc_copyPropertyStruct(&dest, &src, sizeof(struct test_point), NO, NO);
	zassert_mem_equal(&dest, &src, sizeof(struct test_point),
			  "non-atomic copyPropertyStruct should produce identical content");

	/* Test objc_setPropertyStruct + objc_getPropertyStruct (atomic) */
	struct test_point storage;

	memset(&storage, 0, sizeof(storage));
	objc_setPropertyStruct(&storage, &src, sizeof(struct test_point), YES, NO);
	zassert_mem_equal(&storage, &src, sizeof(struct test_point),
			  "atomic setPropertyStruct should store correct content");

	memset(&dest, 0, sizeof(dest));
	objc_getPropertyStruct(&dest, &storage, sizeof(struct test_point), YES, NO);
	zassert_mem_equal(&dest, &src, sizeof(struct test_point),
			  "atomic getPropertyStruct should retrieve correct content");

	/* Test objc_setPropertyStruct + objc_getPropertyStruct (non-atomic) */
	memset(&storage, 0, sizeof(storage));
	objc_setPropertyStruct(&storage, &src, sizeof(struct test_point), NO, NO);

	memset(&dest, 0, sizeof(dest));
	objc_getPropertyStruct(&dest, &storage, sizeof(struct test_point), NO, NO);
	zassert_mem_equal(&dest, &src, sizeof(struct test_point),
			  "non-atomic get/setPropertyStruct should round-trip correctly");
}
