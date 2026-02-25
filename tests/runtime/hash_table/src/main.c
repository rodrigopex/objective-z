/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for hash table (hash.c) via method dispatch and introspection.
 *
 * The method hash table is internal, so we test it indirectly by
 * defining classes with many methods and verifying correct dispatch
 * and respondsToSelector results.
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>

/* ── ObjC helpers (defined in helpers.m) ────────────────────────── */

extern id test_hash_create_calc(int val);
extern id test_hash_create_calc_sub(int val);
extern int test_hash_calc_value(id obj);
extern int test_hash_calc_add(id obj, int n);
extern int test_hash_calc_sub(id obj, int n);
extern int test_hash_calc_mul(id obj, int n);
extern int test_hash_calc_negate(id obj);
extern int test_hash_calc_double(id obj);
extern int test_hash_calc_triple(id obj);
extern int test_hash_calc_quadruple(id obj);
extern int test_hash_class_version(void);
extern int test_hash_class_max(void);
extern void test_hash_dealloc(id obj);

extern BOOL test_hash_instance_responds_value(void);
extern BOOL test_hash_instance_responds_classVersion(void);
extern BOOL test_hash_metaclass_responds_classVersion(void);
extern BOOL test_hash_metaclass_responds_value(void);
extern BOOL test_hash_responds_to_add(void);
extern BOOL test_hash_responds_to_sub(void);
extern BOOL test_hash_responds_to_mul(void);
extern BOOL test_hash_responds_to_negate(void);
extern BOOL test_hash_responds_to_doubleValue(void);
extern BOOL test_hash_responds_to_tripleValue(void);
extern BOOL test_hash_responds_to_nonexistent(void);

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(hash_table, NULL, NULL, NULL, NULL, NULL);

/* All instance methods dispatch correctly (hash lookup works) */
ZTEST(hash_table, test_instance_methods_dispatch)
{
	id calc = test_hash_create_calc(10);

	zassert_not_null(calc, "alloc should succeed");
	zassert_equal(test_hash_calc_value(calc), 10, "value should be 10");
	zassert_equal(test_hash_calc_add(calc, 5), 15, "10+5=15");
	zassert_equal(test_hash_calc_sub(calc, 3), 7, "10-3=7");
	zassert_equal(test_hash_calc_mul(calc, 4), 40, "10*4=40");
	zassert_equal(test_hash_calc_negate(calc), -10, "negate(10)=-10");
	zassert_equal(test_hash_calc_double(calc), 20, "double(10)=20");
	zassert_equal(test_hash_calc_triple(calc), 30, "triple(10)=30");

	test_hash_dealloc(calc);
}

/* Class methods dispatch correctly (metaclass hash lookup) */
ZTEST(hash_table, test_class_methods_dispatch)
{
	zassert_equal(test_hash_class_version(), 42, "classVersion should be 42");
	zassert_equal(test_hash_class_max(), 9999, "maxValue should be 9999");
}

/* Instance responds to 'value' but not 'classVersion'; metaclass opposite */
ZTEST(hash_table, test_instance_vs_class_method)
{
	zassert_true(test_hash_instance_responds_value(),
		     "instance should respond to 'value'");
	zassert_false(test_hash_instance_responds_classVersion(),
		      "instance should NOT respond to 'classVersion'");
	zassert_true(test_hash_metaclass_responds_classVersion(),
		     "metaclass should respond to 'classVersion'");
	zassert_false(test_hash_metaclass_responds_value(),
		      "metaclass should NOT respond to 'value'");
}

/* Subclass inherits all parent methods via hash lookup */
ZTEST(hash_table, test_subclass_inherits_methods)
{
	id sub = test_hash_create_calc_sub(7);

	zassert_not_null(sub, "alloc should succeed");

	/* Inherited methods */
	zassert_equal(test_hash_calc_value(sub), 7, "inherited value should be 7");
	zassert_equal(test_hash_calc_add(sub, 3), 10, "inherited add: 7+3=10");
	zassert_equal(test_hash_calc_double(sub), 14, "inherited double: 7*2=14");

	/* Own method */
	zassert_equal(test_hash_calc_quadruple(sub), 28, "own quadruple: 7*4=28");

	test_hash_dealloc(sub);
}

/* Many methods all resolve correctly (collision resolution works) */
ZTEST(hash_table, test_many_methods_no_collision_loss)
{
	id calc = test_hash_create_calc(5);

	/* Exercise all 7 instance methods in sequence */
	zassert_equal(test_hash_calc_value(calc), 5, "value");
	zassert_equal(test_hash_calc_add(calc, 1), 6, "add");
	zassert_equal(test_hash_calc_sub(calc, 1), 4, "sub");
	zassert_equal(test_hash_calc_mul(calc, 2), 10, "mul");
	zassert_equal(test_hash_calc_negate(calc), -5, "negate");
	zassert_equal(test_hash_calc_double(calc), 10, "double");
	zassert_equal(test_hash_calc_triple(calc), 15, "triple");

	test_hash_dealloc(calc);
}

/* respondsToSelector YES for all known instance methods */
ZTEST(hash_table, test_responds_to_all_methods)
{
	zassert_true(test_hash_responds_to_add(), "should respond to add:");
	zassert_true(test_hash_responds_to_sub(), "should respond to sub:");
	zassert_true(test_hash_responds_to_mul(), "should respond to mul:");
	zassert_true(test_hash_responds_to_negate(), "should respond to negate");
	zassert_true(test_hash_responds_to_doubleValue(), "should respond to doubleValue");
	zassert_true(test_hash_responds_to_tripleValue(), "should respond to tripleValue");
}

/* respondsToSelector NO for unknown method */
ZTEST(hash_table, test_responds_no_for_unknown)
{
	zassert_false(test_hash_responds_to_nonexistent(),
		      "TestCalc should NOT respond to unknown method");
}
