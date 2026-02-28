#include "category.h"
#include "class.h"
#include "hash.h"
#include "protocol.h"
#include <objc/malloc.h>
#include <objc/version.h>
#include <zephyr/init.h>
#include <zephyr/sys/printk.h>

#include <zephyr/logging/log.h>
LOG_MODULE_REGISTER(objz, CONFIG_OBJZ_LOG_LEVEL);

/**
 * GNUstep ObjC exception personality stub.
 *
 * Clang emits references to this in .ARM.extab sections for ObjC code
 * even with -fno-exceptions (e.g. collection literal cleanup).
 * We provide a no-op stub since this runtime does not support @try/@catch.
 */
int __gnustep_objc_personality_v0(int version, int actions, long long exn_class,
				  void *exn_info, void *context)
{
	(void)version;
	(void)actions;
	(void)exn_class;
	(void)exn_info;
	(void)context;
	return 0;
}

int objz_init(void)
{
	objc_heap_init();

#if defined(CONFIG_OBJZ_BOOT_BANNER)
	printk("*** " CONFIG_OBJZ_BOOT_BANNER_STRING " v" OBJZ_VERSION_STRING " ***\n");
#endif

	return 0;
}

SYS_INIT(objz_init, APPLICATION, 99);

extern objc_class_t *class_table[];
extern objc_protocol_t *protocol_table[];
extern struct objc_hashitem hash_table[];

void objc_print_table_stats(void)
{
	int class_used = 0;
	int protocol_used = 0;
	int hash_used = 0;

	for (int i = 0; i < CONFIG_OBJZ_CLASS_TABLE_SIZE; i++) {
		if (class_table[i] != NULL) {
			class_used++;
		}
	}
	for (int i = 0; i < CONFIG_OBJZ_PROTOCOL_TABLE_SIZE; i++) {
		if (protocol_table[i] != NULL) {
			protocol_used++;
		}
	}
	for (int i = 0; i < CONFIG_OBJZ_HASH_TABLE_SIZE; i++) {
		if (hash_table[i].cls != NULL) {
			hash_used++;
		}
	}

	printk("Objective-C Runtime Table Stats:\n");
	printk("  %-12s %5s %5s\n", "Table", "Size", "Used");
	printk("  %-12s %5d %5d\n", "class", CONFIG_OBJZ_CLASS_TABLE_SIZE, class_used);
	printk("  %-12s %5d %5d\n", "category", CONFIG_OBJZ_CATEGORY_TABLE_SIZE,
	       __objc_category_count());
	printk("  %-12s %5d %5d\n", "protocol", CONFIG_OBJZ_PROTOCOL_TABLE_SIZE, protocol_used);
	printk("  %-12s %5d %5d\n", "hash", CONFIG_OBJZ_HASH_TABLE_SIZE, hash_used);
}
