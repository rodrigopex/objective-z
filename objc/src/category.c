#include "category.h"
#include "api.h"
#include "class.h"
#include <objc/objc.h>
#include <zephyr/kernel.h>

static struct objc_category *category_table[CONFIG_OBJZ_CATEGORY_TABLE_SIZE + 1];

void __objc_category_init(void)
{
	static BOOL init = NO;
	if (init) {
		return;
	}
	init = YES;
	for (int i = 0; i <= CONFIG_OBJZ_CATEGORY_TABLE_SIZE; i++) {
		category_table[i] = NULL;
	}
}

void __objc_category_register(struct objc_category *category)
{
	if (category == NULL || category->name == NULL || category->class_name == NULL) {
		return;
	}
#ifdef OBJCDEBUG
	printk("__objc_category_register [%s+%s]\n", category->class_name, category->name);
#endif
	for (int i = 0; i < CONFIG_OBJZ_CATEGORY_TABLE_SIZE; i++) {
		if (category_table[i] == category) {
			return;
		}
		if (category_table[i] == NULL) {
			category_table[i] = category;
			return;
		}
	}
	printk("Category table is full, cannot register category: %s\n", category->name);
}

static void __objc_category_load_category(struct objc_category *category)
{
	objc_class_t *cls = objc_lookup_class(category->class_name);
	if (cls == Nil) {
		return;
	}

#ifdef OBJCDEBUG
	printk("  __objc_category_load_category [%s+%s]\n", cls->name, category->name);
#endif

	if (category->instance_methods != NULL) {
		for (struct objc_method_list *ml = category->instance_methods; ml != NULL;
		     ml = ml->next) {
			__objc_class_register_method_list(cls, ml);
		}
	}

	if (category->class_methods != NULL && cls->metaclass != NULL) {
		for (struct objc_method_list *ml = category->class_methods; ml != NULL;
		     ml = ml->next) {
			__objc_class_register_method_list(cls->metaclass, ml);
		}
	}
}

BOOL __objc_category_load(void)
{
	static BOOL init = NO;
	if (init) {
		return NO;
	}
	init = YES;

	for (int i = 0; i < CONFIG_OBJZ_CATEGORY_TABLE_SIZE; i++) {
		struct objc_category *category = category_table[i];
		if (category == NULL) {
			continue;
		}
		__objc_category_load_category(category);
	}

	return YES;
}

int __objc_category_count(void)
{
	int count = 0;
	for (int i = 0; i < CONFIG_OBJZ_CATEGORY_TABLE_SIZE; i++) {
		if (category_table[i] != NULL) {
			count++;
		}
	}
	return count;
}
