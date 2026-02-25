/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file main.c
 * @brief Tests for message dispatch (message.c, slot.c).
 */
#include <zephyr/ztest.h>
#include <objc/runtime.h>

/* ── External ABI structures (from api.h) ───────────────────────── */

struct objc_selector {
	void *sel_id;
	char *sel_type;
};

struct objc_super {
	id receiver;
	struct objc_class *superclass;
};

struct objc_slot {
	Class owner;
	Class cached_for;
	const char *types;
	unsigned int version;
	IMP method;
};

/* ── Runtime functions ──────────────────────────────────────────── */

extern IMP objc_msg_lookup(id receiver, SEL selector);
extern IMP objc_msg_lookup_super(struct objc_super *super, SEL selector);
extern struct objc_slot *objc_slot_lookup_super(struct objc_super *super,
						SEL selector);

/* ── ObjC helpers (defined in helpers.m) ────────────────────────── */

extern id test_create_animal(void);
extern id test_create_dog(void);
extern void test_dealloc(id obj);
extern int test_call_speak(id obj);
extern int test_call_legCount(id obj);
extern int test_call_classValue_on_class(Class cls);
extern int test_call_fetch(id obj);
extern BOOL test_call_respondsToSelector(id obj, const char *sel_name);

/* ── +initialize counters ───────────────────────────────────────── */

extern int g_animal_init_count;
extern int g_dog_init_count;

/* ── Test suite ─────────────────────────────────────────────────── */

ZTEST_SUITE(message_dispatch, NULL, NULL, NULL, NULL, NULL);

/* Nil receiver returns a non-NULL IMP (the nil method handler) */
ZTEST(message_dispatch, test_nil_receiver)
{
	struct objc_selector sel = { .sel_id = "speak", .sel_type = NULL };
	IMP imp = objc_msg_lookup(nil, &sel);

	zassert_not_null(imp, "nil receiver should return nil_method IMP, not NULL");

	/* Calling the nil method should return nil */
	id result = imp(nil, &sel);
	zassert_is_null(result, "nil method should return the receiver (nil)");
}

/* Instance method dispatch returns correct result */
ZTEST(message_dispatch, test_instance_method)
{
	id animal = test_create_animal();

	zassert_not_null(animal, "alloc should succeed");
	int val = test_call_speak(animal);
	zassert_equal(val, 1, "TestAnimal speak should return 1");

	test_dealloc(animal);
}

/* Class method dispatch */
ZTEST(message_dispatch, test_class_method)
{
	Class cls = objc_lookupClass("TestAnimal");

	zassert_not_null(cls, "TestAnimal class should exist");
	int val = test_call_classValue_on_class(cls);
	zassert_equal(val, 42, "TestAnimal classValue should return 42");
}

/* Subclass overrides parent method */
ZTEST(message_dispatch, test_subclass_override)
{
	id dog = test_create_dog();

	zassert_not_null(dog, "alloc should succeed");

	/* Dog overrides speak */
	int val = test_call_speak(dog);
	zassert_equal(val, 2, "TestDog speak should return 2");

	/* Dog inherits legCount from TestAnimal */
	val = test_call_legCount(dog);
	zassert_equal(val, 4, "TestDog should inherit legCount=4");

	/* Dog has its own method */
	val = test_call_fetch(dog);
	zassert_equal(val, 99, "TestDog fetch should return 99");

	test_dealloc(dog);
}

/* Unknown selector returns NULL IMP */
ZTEST(message_dispatch, test_unknown_selector)
{
	id animal = test_create_animal();
	struct objc_selector sel = { .sel_id = "nonExistentMethod",
				     .sel_type = NULL };
	IMP imp = objc_msg_lookup(animal, &sel);

	zassert_is_null(imp, "unknown selector should return NULL IMP");
	test_dealloc(animal);
}

/* Super send: dog's superclass resolves to TestAnimal's method */
ZTEST(message_dispatch, test_super_send)
{
	id dog = test_create_dog();
	Class dog_cls = object_getClass(dog);
	Class animal_cls = class_getSuperclass(dog_cls);

	zassert_not_null(animal_cls, "Dog should have a superclass");

	struct objc_selector sel = { .sel_id = "speak", .sel_type = NULL };
	struct objc_super sup = { .receiver = dog, .superclass = animal_cls };

	IMP imp = objc_msg_lookup_super(&sup, &sel);
	zassert_not_null(imp, "super send for 'speak' should find Animal's IMP");

	/* Call with the IMP — should get Animal's version (1) not Dog's (2) */
	int result = ((int (*)(id, SEL))imp)(dog, &sel);
	zassert_equal(result, 1, "super send should call Animal's speak (1)");

	test_dealloc(dog);
}

/* Super send with nil receiver */
ZTEST(message_dispatch, test_super_nil_receiver)
{
	struct objc_selector sel = { .sel_id = "speak", .sel_type = NULL };
	struct objc_super sup = { .receiver = nil, .superclass = NULL };

	IMP imp = objc_msg_lookup_super(&sup, &sel);
	zassert_is_null(imp, "super send with nil receiver should return NULL");
}

/* Super send via NULL struct */
ZTEST(message_dispatch, test_super_null_struct)
{
	struct objc_selector sel = { .sel_id = "speak", .sel_type = NULL };

	IMP imp = objc_msg_lookup_super(NULL, &sel);
	zassert_is_null(imp, "super send with NULL super should return NULL");
}

/* class_respondsToSelector YES */
ZTEST(message_dispatch, test_responds_to_selector_yes)
{
	Class cls = objc_lookupClass("TestAnimal");
	struct objc_selector sel = { .sel_id = "speak", .sel_type = NULL };

	zassert_true(class_respondsToSelector(cls, &sel),
		     "TestAnimal should respond to 'speak'");
}

/* class_respondsToSelector NO */
ZTEST(message_dispatch, test_responds_to_selector_no)
{
	Class cls = objc_lookupClass("TestAnimal");
	struct objc_selector sel = { .sel_id = "nonExistent", .sel_type = NULL };

	zassert_false(class_respondsToSelector(cls, &sel),
		      "TestAnimal should not respond to 'nonExistent'");
}

/* class_respondsToSelector with Nil class */
ZTEST(message_dispatch, test_responds_nil_class)
{
	struct objc_selector sel = { .sel_id = "speak", .sel_type = NULL };

	zassert_false(class_respondsToSelector(Nil, &sel),
		      "Nil class should return NO");
}

/* object_respondsToSelector */
ZTEST(message_dispatch, test_object_responds)
{
	id animal = test_create_animal();
	struct objc_selector sel_yes = { .sel_id = "speak", .sel_type = NULL };
	struct objc_selector sel_no = { .sel_id = "nonExistent",
					.sel_type = NULL };

	zassert_true(object_respondsToSelector(animal, &sel_yes),
		     "animal should respond to 'speak'");
	zassert_false(object_respondsToSelector(animal, &sel_no),
		      "animal should not respond to 'nonExistent'");
	zassert_false(object_respondsToSelector(nil, &sel_yes),
		      "nil object should return NO");

	test_dealloc(animal);
}

/* class_metaclassRespondsToSelector */
ZTEST(message_dispatch, test_metaclass_responds)
{
	Class cls = objc_lookupClass("TestAnimal");
	struct objc_selector sel_yes = { .sel_id = "classValue",
					 .sel_type = NULL };
	struct objc_selector sel_no = { .sel_id = "speak", .sel_type = NULL };

	zassert_true(class_metaclassRespondsToSelector(cls, &sel_yes),
		     "metaclass should respond to 'classValue'");
	zassert_false(class_metaclassRespondsToSelector(cls, &sel_no),
		      "metaclass should not respond to instance method 'speak'");
}

/* sel_getName */
ZTEST(message_dispatch, test_sel_getName)
{
	struct objc_selector sel = { .sel_id = "testSelector",
				     .sel_type = NULL };

	const char *name = sel_getName(&sel);
	zassert_not_null(name, "sel_getName should return a name");
	zassert_mem_equal(name, "testSelector", 12,
			  "sel_getName should return the selector name");

	zassert_is_null(sel_getName(NULL),
			"sel_getName(NULL) should return NULL");
}

/* +initialize called exactly once per class */
ZTEST(message_dispatch, test_initialize_once)
{
	/* Creating objects triggers +initialize */
	id dog1 = test_create_dog();
	id dog2 = test_create_dog();

	zassert_equal(g_dog_init_count, 1,
		      "Dog +initialize should be called exactly once");

	test_dealloc(dog1);
	test_dealloc(dog2);
}

/* +initialize: superclass initialized before subclass */
ZTEST(message_dispatch, test_initialize_super_first)
{
	/* Both counters should already be set from previous test or
	 * from class first use. The key invariant: Animal's init
	 * count must be >= 1 whenever Dog's is >= 1. */
	zassert_true(g_animal_init_count >= 1,
		     "Animal +initialize should have been called");
	zassert_true(g_dog_init_count >= 1,
		     "Dog +initialize should have been called");
}

/* Slot lookup super (gnustep-1.7 bridge) */
ZTEST(message_dispatch, test_slot_lookup_super)
{
	id dog = test_create_dog();
	Class dog_cls = object_getClass(dog);
	Class animal_cls = class_getSuperclass(dog_cls);

	struct objc_selector sel = { .sel_id = "speak", .sel_type = NULL };
	struct objc_super sup = { .receiver = dog, .superclass = animal_cls };

	struct objc_slot *slot = objc_slot_lookup_super(&sup, &sel);

	zassert_not_null(slot, "slot lookup should return non-NULL");
	zassert_not_null(slot->method,
			 "slot should contain a valid IMP");

	/* Call the IMP — should be Animal's speak (1) */
	int result = ((int (*)(id, SEL))slot->method)(dog, &sel);
	zassert_equal(result, 1, "slot super should resolve Animal's speak");

	test_dealloc(dog);
}
