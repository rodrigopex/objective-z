/**
 * @file pool.h
 * @brief Static allocation pools for Objective-C classes.
 *
 * OZ_DEFINE_POOL(ClassName, count) creates a fixed-size memory slab
 * for instances of ClassName using Zephyr's K_MEM_SLAB_DEFINE.
 * The runtime checks the pool registry in +alloc before falling
 * back to the sys_heap allocator.
 *
 * Usage (pool definition — must be in a .c file, not .m):
 *   OZ_DEFINE_POOL(Sensor, 16, 8, 4)
 *   // 8 blocks of 16 bytes each, 4-byte aligned
 *
 * Usage (compile-time pool access — avoids runtime string lookup):
 *   OBJZ_POOL_DECLARE(Sensor);
 *   uint32_t used = k_mem_slab_num_used_get(&OBJZ_POOL(Sensor));
 *
 * The block_size parameter is the instance size of the class
 * (sizeof(isa) + ivars). Use class_getInstanceSize() to determine
 * this, or compute manually from the class structure.
 */
#pragma once

#include <zephyr/kernel.h>

/**
 * @brief Register a static memory pool for a class at runtime.
 *
 * Called automatically by OZ_DEFINE_POOL via SYS_INIT.
 *
 * @param class_name  Name of the ObjC class (C string).
 * @param slab        Pointer to the k_mem_slab struct.
 * @param block_size  Size of each block in bytes.
 */
void __objc_pool_register(const char *class_name, struct k_mem_slab *slab,
			  size_t block_size);

/**
 * @brief Allocate a block from the typed pool for a class.
 *
 * @param class_name  Name of the ObjC class.
 * @return Pointer to zeroed memory, or NULL if pool full or no pool.
 */
void *__objc_pool_alloc(const char *class_name);

/**
 * @brief Free a block back to its typed pool.
 *
 * @param ptr  Pointer to the memory block.
 * @return true if the block was returned to a pool, false otherwise.
 */
bool __objc_pool_free(void *ptr);

/**
 * @brief Get the slab backing a class pool.
 *
 * @param class_name  Name of the ObjC class.
 * @return Pointer to the k_mem_slab, or NULL if no pool registered.
 */
struct k_mem_slab *__objc_pool_get_slab(const char *class_name);

/**
 * @brief Resolve the pool slab symbol for a class at compile time.
 *
 * Expands to the slab variable name directly, avoiding the O(n) string
 * lookup of __objc_pool_get_slab(). Use with & to get a pointer:
 *   k_mem_slab_num_used_get(&OBJZ_POOL(MyClass));
 *
 * If the class has no pool, the build fails with a link error (safer
 * than a silent runtime NULL).
 */
#define OBJZ_POOL(cls) _objz_pool_##cls

/**
 * @brief Forward-declare the pool slab for a class defined elsewhere.
 *
 * Place in any C file that needs compile-time pool access:
 *   OBJZ_POOL_DECLARE(MyClass);
 */
#define OBJZ_POOL_DECLARE(cls) extern struct k_mem_slab _objz_pool_##cls

/**
 * @brief Define a static allocation pool for an Objective-C class.
 *
 * @param cls    Class name (unquoted identifier).
 * @param bsz    Block size in bytes (must be multiple of align).
 * @param cnt    Maximum number of instances.
 * @param align  Block alignment (power of 2, minimum 4).
 *
 * Example:
 *   OZ_DEFINE_POOL(Sensor, 16, 8, 4)
 *   // 8 blocks of 16 bytes each, 4-byte aligned
 */
#define OZ_DEFINE_POOL(cls, bsz, cnt, align)                                                       \
	K_MEM_SLAB_DEFINE(_objz_pool_##cls, bsz, cnt, align);                                     \
	static int _objz_pool_init_##cls(void)                                                     \
	{                                                                                          \
		__objc_pool_register(#cls, &_objz_pool_##cls, bsz);                                \
		return 0;                                                                          \
	}                                                                                          \
	SYS_INIT(_objz_pool_init_##cls, APPLICATION, 98)
