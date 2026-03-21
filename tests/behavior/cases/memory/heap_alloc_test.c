/* Behavior test: heap allocation via allocWithHeap: with usage tracking.
 * Verifies user heap and system heap paths, and stress tests for leaks. */
#include "unity.h"
#include "Widget_ozh.h"
#include "Foundation/OZObject_ozh.h"
#include "OZHeap_ozh.h"

static char heap_buffer[4096];
static struct OZHeap *test_heap;

void setUp(void)
{
	test_heap = OZHeap_alloc();
	TEST_ASSERT_NOT_NULL(test_heap);
	test_heap = (struct OZHeap *)OZHeap_initWithBuffer_size_(
		test_heap, heap_buffer, (int)sizeof(heap_buffer));
	TEST_ASSERT_NOT_NULL(test_heap);
}

void tearDown(void)
{
	OZObject_release((struct OZObject *)test_heap);
}

void test_heap_alloc_returns_valid(void)
{
	struct Widget *w = Widget_allocWithHeap_((struct OZObject *)test_heap);
	TEST_ASSERT_NOT_NULL(w);
	TEST_ASSERT_EQUAL_INT(OZ_CLASS_Widget, w->base._meta.class_id);
	TEST_ASSERT_EQUAL_INT(1, w->base._meta.heap_allocated);
	OZObject_release((struct OZObject *)w);
}

void test_heap_usage_tracks_alloc_free(void)
{
	size_t before = oz_heap_used_bytes(&test_heap->_inner);
	TEST_ASSERT_EQUAL_UINT(0, before);

	struct Widget *w = Widget_allocWithHeap_((struct OZObject *)test_heap);
	TEST_ASSERT_NOT_NULL(w);
	size_t after_alloc = oz_heap_used_bytes(&test_heap->_inner);
	TEST_ASSERT_GREATER_THAN(0, after_alloc);

	OZObject_release((struct OZObject *)w);
	size_t after_free = oz_heap_used_bytes(&test_heap->_inner);
	TEST_ASSERT_EQUAL_UINT(0, after_free);
}

void test_heap_stress_no_leak(void)
{
	size_t before = oz_heap_used_bytes(&test_heap->_inner);

	for (int cycle = 0; cycle < 10; cycle++) {
		struct Widget *objs[8];
		for (int i = 0; i < 8; i++) {
			objs[i] = Widget_allocWithHeap_((struct OZObject *)test_heap);
			TEST_ASSERT_NOT_NULL(objs[i]);
			Widget_setTag_(objs[i], cycle * 100 + i);
		}
		size_t mid = oz_heap_used_bytes(&test_heap->_inner);
		TEST_ASSERT_GREATER_THAN(0, mid);

		for (int i = 0; i < 8; i++) {
			TEST_ASSERT_EQUAL_INT(cycle * 100 + i, Widget_tag(objs[i]));
			OZObject_release((struct OZObject *)objs[i]);
		}
	}

	size_t after = oz_heap_used_bytes(&test_heap->_inner);
	TEST_ASSERT_EQUAL_UINT(before, after);
}

void test_slab_alloc_still_works(void)
{
	struct Widget *w = Widget_alloc();
	TEST_ASSERT_NOT_NULL(w);
	TEST_ASSERT_EQUAL_INT(OZ_CLASS_Widget, w->base._meta.class_id);
	TEST_ASSERT_EQUAL_INT(0, w->base._meta.heap_allocated);
	OZObject_release((struct OZObject *)w);
}

void test_sys_heap_nil(void)
{
	struct Widget *w = Widget_allocWithHeap_((struct OZObject *)0);
	TEST_ASSERT_NOT_NULL(w);
	TEST_ASSERT_EQUAL_INT(1, w->base._meta.heap_allocated);
	OZObject_release((struct OZObject *)w);
}
