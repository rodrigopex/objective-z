/**
 * @file pool.c
 * @brief Static allocation pool registry for Objective-C classes.
 *
 * Maps class names to K_MEM_SLAB instances.  Provides alloc/free
 * that +[Object alloc] and -[Object dealloc] call before falling
 * back to the sys_heap allocator.
 */
#include <objc/pool.h>
#include "api.h"
#include <string.h>
#include <zephyr/kernel.h>

#ifndef CONFIG_OBJZ_STATIC_POOL_TABLE_SIZE
#define CONFIG_OBJZ_STATIC_POOL_TABLE_SIZE 16
#endif

struct pool_entry {
	const char *class_name;
	struct k_mem_slab *slab;
	size_t block_size;
};

static struct pool_entry _pool_table[CONFIG_OBJZ_STATIC_POOL_TABLE_SIZE];
static int _pool_count;

void __objc_pool_register(const char *class_name, struct k_mem_slab *slab,
			  size_t block_size)
{
	if (_pool_count >= CONFIG_OBJZ_STATIC_POOL_TABLE_SIZE) {
		return;
	}
	_pool_table[_pool_count].class_name = class_name;
	_pool_table[_pool_count].slab = slab;
	_pool_table[_pool_count].block_size = block_size;
	_pool_count++;
}

static struct pool_entry *__objc_pool_find(const char *class_name)
{
	for (int i = 0; i < _pool_count; i++) {
		if (strcmp(_pool_table[i].class_name, class_name) == 0) {
			return &_pool_table[i];
		}
	}
	return NULL;
}

void *__objc_pool_alloc(Class cls)
{
	struct objc_class *c = (struct objc_class *)cls;
	struct k_mem_slab *slab = (struct k_mem_slab *)c->extra_data;
	if (slab == NULL) {
		return NULL;
	}
	void *block = NULL;
	int ret = k_mem_slab_alloc(slab, &block, K_NO_WAIT);
	if (ret != 0) {
		return NULL;
	}
	memset(block, 0, slab->info.block_size);
	return block;
}

struct k_mem_slab *__objc_pool_get_slab(const char *class_name)
{
	struct pool_entry *e = __objc_pool_find(class_name);
	if (e == NULL) {
		return NULL;
	}
	return e->slab;
}

bool __objc_pool_free(id obj)
{
	if (obj == NULL) {
		return false;
	}
	struct objc_class *cls = ((struct objc_object *)obj)->isa;
	struct k_mem_slab *slab = (struct k_mem_slab *)cls->extra_data;
	if (slab == NULL) {
		return false;
	}
	char *buf = slab->buffer;
	size_t total = slab->info.num_blocks * slab->info.block_size;
	if ((char *)obj >= buf && (char *)obj < buf + total) {
		k_mem_slab_free(slab, obj);
		return true;
	}
	return false;
}
