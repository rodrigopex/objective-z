/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for protocol conformance (protocol.c, Protocol.m).
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>

/* ── ObjC helpers (defined in helpers.m) ────────────────────────── */

extern id test_proto_create_widget(int wid);
extern id test_proto_create_button(int wid);
extern id test_proto_create_label(int text);
extern int test_proto_draw(id obj);
extern int test_proto_resize(id obj, int factor);
extern int test_proto_widget_id(id obj);
extern BOOL test_proto_widget_conforms_drawable(id obj);
extern BOOL test_proto_widget_conforms_resizable(id obj);
extern BOOL test_proto_class_conforms_drawable(const char *name);
extern BOOL test_proto_class_conforms_resizable(const char *name);
extern void test_proto_dealloc(id obj);

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(protocols, NULL, NULL, NULL, NULL, NULL);

/* Class that directly adopts a protocol conforms to it */
ZTEST(protocols, test_class_conforms_direct)
{
	zassert_true(test_proto_class_conforms_drawable("TestWidget"),
		     "TestWidget should conform to TestDrawable");
}

/* Class that does NOT adopt a protocol does not conform */
ZTEST(protocols, test_class_not_conforms)
{
	zassert_false(test_proto_class_conforms_drawable("TestLabel"),
		      "TestLabel should NOT conform to TestDrawable");
}

/* Subclass inherits protocol conformance from parent */
ZTEST(protocols, test_subclass_inherits_protocol)
{
	zassert_true(test_proto_class_conforms_drawable("TestButton"),
		     "TestButton should inherit TestDrawable from TestWidget");
}

/* Subclass has its own protocol conformance */
ZTEST(protocols, test_subclass_own_protocol)
{
	zassert_true(test_proto_class_conforms_resizable("TestButton"),
		     "TestButton should conform to TestResizable");
}

/* Parent does not acquire child's protocol */
ZTEST(protocols, test_parent_not_child_protocol)
{
	zassert_false(test_proto_class_conforms_resizable("TestWidget"),
		      "TestWidget should NOT conform to TestResizable");
}

/* Instance conformsTo: on direct adopter */
ZTEST(protocols, test_instance_conforms_direct)
{
	id widget = test_proto_create_widget(1);

	zassert_true(test_proto_widget_conforms_drawable(widget),
		     "widget instance should conform to TestDrawable");
	zassert_false(test_proto_widget_conforms_resizable(widget),
		      "widget instance should NOT conform to TestResizable");

	test_proto_dealloc(widget);
}

/* Instance conformsTo: on subclass */
ZTEST(protocols, test_instance_conforms_subclass)
{
	id button = test_proto_create_button(2);

	zassert_true(test_proto_widget_conforms_drawable(button),
		     "button should conform to TestDrawable (inherited)");
	zassert_true(test_proto_widget_conforms_resizable(button),
		     "button should conform to TestResizable (own)");

	test_proto_dealloc(button);
}

/* Protocol methods are callable */
ZTEST(protocols, test_protocol_method_callable)
{
	id widget = test_proto_create_widget(5);

	zassert_equal(test_proto_draw(widget), 50,
		      "draw should return id*10 = 50");

	test_proto_dealloc(widget);
}

/* Subclass protocol method callable */
ZTEST(protocols, test_subclass_protocol_method)
{
	id button = test_proto_create_button(3);

	zassert_equal(test_proto_draw(button), 30,
		      "inherited draw should return id*10 = 30");
	zassert_equal(test_proto_resize(button, 5), 15,
		      "resize should return id*factor = 15");

	test_proto_dealloc(button);
}

/* Class that does not exist does not conform */
ZTEST(protocols, test_unknown_class_not_conforms)
{
	zassert_false(test_proto_class_conforms_drawable("NoSuchClass"),
		      "nonexistent class should not conform");
}
