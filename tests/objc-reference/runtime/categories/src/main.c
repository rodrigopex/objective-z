/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for category loading (category.c).
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>

/* ── ObjC helpers (defined in helpers.m) ────────────────────────── */

extern id test_cat_create_shape(int sides);
extern int test_cat_sides(id obj);
extern int test_cat_base_value(id obj);
extern int test_cat_perimeter(id obj);
extern BOOL test_cat_is_triangle(id obj);
extern int test_cat_default_sides(void);
extern void test_cat_dealloc(id obj);
extern BOOL test_cat_responds_perimeter(void);
extern BOOL test_cat_responds_isTriangle(void);

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(categories, NULL, NULL, NULL, NULL, NULL);

/* Category adds a new instance method (perimeter) */
ZTEST(categories, test_category_adds_instance_method)
{
	id shape = test_cat_create_shape(5);

	zassert_not_null(shape, "alloc should succeed");
	zassert_equal(test_cat_perimeter(shape), 50,
		      "perimeter should be sides*10 = 50");

	test_cat_dealloc(shape);
}

/* Category adds another instance method (isTriangle) */
ZTEST(categories, test_category_adds_bool_method)
{
	id tri = test_cat_create_shape(3);
	id quad = test_cat_create_shape(4);

	zassert_true(test_cat_is_triangle(tri),
		     "3-sided shape should be triangle");
	zassert_false(test_cat_is_triangle(quad),
		      "4-sided shape should not be triangle");

	test_cat_dealloc(tri);
	test_cat_dealloc(quad);
}

/* Category overrides existing instance method (baseValue) */
ZTEST(categories, test_category_overrides_instance_method)
{
	id shape = test_cat_create_shape(4);

	/*
	 * Original baseValue returns 100, but the Override category
	 * replaces it with 999.
	 */
	zassert_equal(test_cat_base_value(shape), 999,
		      "category should override baseValue to 999");

	test_cat_dealloc(shape);
}

/* Category overrides existing class method (defaultSides) */
ZTEST(categories, test_category_overrides_class_method)
{
	/*
	 * Original defaultSides returns 4, but the Override category
	 * replaces it with 6.
	 */
	zassert_equal(test_cat_default_sides(), 6,
		      "category should override defaultSides to 6");
}

/* Base class methods still work alongside category methods */
ZTEST(categories, test_base_methods_still_work)
{
	id shape = test_cat_create_shape(8);

	zassert_equal(test_cat_sides(shape), 8,
		      "base method 'sides' should still work");

	test_cat_dealloc(shape);
}

/* Category methods are discoverable via respondsToSelector */
ZTEST(categories, test_category_responds_to_selector)
{
	zassert_true(test_cat_responds_perimeter(),
		     "should respond to category method 'perimeter'");
	zassert_true(test_cat_responds_isTriangle(),
		     "should respond to category method 'isTriangle'");
}
