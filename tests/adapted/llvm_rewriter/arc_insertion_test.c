/*
 * Adapted from: clang/test/CodeGenObjC/arc-precise-lifetime.m
 * License: Apache 2.0 with LLVM Exception
 * Verifies ARC retain/release at scope boundaries via refcount checks.
 */
#include "unity.h"
#include "Resource_ozh.h"
#include "Manager_ozh.h"

void test_object_ivar_lifetime(void)
{
	struct Manager *m = Manager_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)m);

	struct Resource *r = Resource_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)r);
	Resource_setValue_(r, 42);

	Manager_setResource_(m, r);

	TEST_ASSERT_EQUAL_INT(42, Manager_resourceValue(m));

	OZObject_release((struct OZObject *)m);
	OZObject_release((struct OZObject *)r);
}

void test_scope_cleanup(void)
{
	struct Resource *r = Resource_alloc();
	OZ_PROTOCOL_SEND_init((struct OZObject *)r);

	uint32_t rc = OZObject_retainCount((struct OZObject *)r);
	TEST_ASSERT_EQUAL_UINT32(1, rc);

	OZObject_release((struct OZObject *)r);
}
