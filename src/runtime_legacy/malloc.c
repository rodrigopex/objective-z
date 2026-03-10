#include <objc/malloc.h>
#include <zephyr/kernel.h>
#include <zephyr/sys/sys_heap.h>

#ifdef CONFIG_OBJZ_MEMORY_POOL_CUSTOM_SECTION
#define HEAP_MEM_ATTRIBUTES Z_GENERIC_SECTION(.objz_heap) __aligned(8)
#elif defined(CONFIG_ZEPHYR_OBJC_MEMORY_POOL_ZEPHYR_REGION)
#define HEAP_MEM_ATTRIBUTES Z_GENERIC_SECTION(CONFIG_OBJZ_MEMORY_POOL_ZEPHYR_REGION_NAME)
__aligned(8)
#else
#define HEAP_MEM_ATTRIBUTES __aligned(8)
#endif /* CONFIG_OBJZ_MEMORY_POOL_CUSTOM_SECTION */

static char _objc_heap_mem[CONFIG_OBJZ_MEM_POOL_SIZE] HEAP_MEM_ATTRIBUTES;
static struct sys_heap objc_heap;
static struct k_spinlock objc_heap_lock;

void *objc_malloc(size_t size)
{
	k_spinlock_key_t key;
	void *ret;

	key = k_spin_lock(&objc_heap_lock);
	ret = sys_heap_alloc(&objc_heap, size);
	k_spin_unlock(&objc_heap_lock, key);

	return ret;
}

void *objc_realloc(void *ptr, size_t size)
{
	k_spinlock_key_t key;
	void *ret;

	key = k_spin_lock(&objc_heap_lock);
	ret = sys_heap_realloc(&objc_heap, ptr, size);
	k_spin_unlock(&objc_heap_lock, key);

	return ret;
}

void objc_free(void *ptr)
{
	k_spinlock_key_t key;

	key = k_spin_lock(&objc_heap_lock);
	sys_heap_free(&objc_heap, ptr);
	k_spin_unlock(&objc_heap_lock, key);
}

void objc_print_heap_info(bool dump_chunks)
{
	k_spinlock_key_t key;

	key = k_spin_lock(&objc_heap_lock);
	sys_heap_print_info(&objc_heap, dump_chunks);
	k_spin_unlock(&objc_heap_lock, key);
}

void objc_stats(struct sys_memory_stats *stats)
{
#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
	k_spinlock_key_t key;

	key = k_spin_lock(&objc_heap_lock);
	sys_heap_runtime_stats_get(&objc_heap, stats);
	k_spin_unlock(&objc_heap_lock, key);
#else
	ARG_UNUSED(stats);
	printk("Enable CONFIG_SYS_HEAP_RUNTIME_STATS to use the mem monitor feature\n");
#endif /* CONFIG_SYS_HEAP_RUNTIME_STATS */
}

void objc_heap_init(void)
{
	sys_heap_init(&objc_heap, &_objc_heap_mem[0], CONFIG_OBJZ_MEM_POOL_SIZE);
}
