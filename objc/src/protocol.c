#include "protocol.h"
#include "api.h"
#include <objc/objc.h>
#include <string.h>
#include <zephyr/kernel.h>

objc_protocol_t *protocol_table[CONFIG_OBJZ_PROTOCOL_TABLE_SIZE + 1];

void __objc_protocol_init(void)
{
	static BOOL init = NO;
	if (init) {
		return;
	}
	init = YES;

	for (int i = 0; i <= CONFIG_OBJZ_PROTOCOL_TABLE_SIZE; i++) {
		protocol_table[i] = NULL;
	}
}

void __objc_protocol_register(objc_protocol_t *p)
{
	if (p == NULL || p->name == NULL) {
		return;
	}
#ifdef OBJCDEBUG
	printk("__objc_protocol_register <%s>\n", p->name);
#endif
	for (int i = 0; i < CONFIG_OBJZ_PROTOCOL_TABLE_SIZE; i++) {
		if (protocol_table[i] == p || protocol_table[i] == NULL) {
			protocol_table[i] = p;
			return;
		}
		if (strcmp(protocol_table[i]->name, p->name) == 0) {
			printk("Warning: Duplicate protocol named: %s. Registration skipped.\n",
			       p->name);
			return;
		}
	}
	printk("Protocol table is full, cannot register protocol: %s", p->name);
}

void __objc_protocol_list_register(struct objc_protocol_list *list)
{
	if (list == NULL) {
		return;
	}
	for (size_t i = 0; i < list->count; i++) {
		objc_protocol_t *protocol = list->protocols[i];
		if (protocol != NULL && protocol->name != NULL) {
			__objc_protocol_register(protocol);
		}
	}
	if (list->next != NULL) {
		__objc_protocol_list_register(list->next);
	}
}

/**
 * Returns the name of a protocol.
 */
const char *proto_getName(objc_protocol_t *protocol)
{
	if (protocol == NULL || protocol->name == NULL) {
		return NULL;
	}
	return protocol->name;
}

/**
 * Checks if a protocol conforms to another protocol.
 */
BOOL proto_conformsTo(objc_protocol_t *protocol, objc_protocol_t *otherProtocol)
{
	if (protocol == NULL || otherProtocol == NULL) {
		return NO;
	}
	if (protocol == otherProtocol) {
		return YES;
	}
	if (strcmp(protocol->name, otherProtocol->name) == 0) {
		return YES;
	}
	struct objc_protocol_list *proto = protocol->protocol_list;
	while (proto != NULL) {
		for (size_t i = 0; i < proto->count; i++) {
			if (proto_conformsTo(proto->protocols[i], otherProtocol)) {
				return YES;
			}
		}
		proto = proto->next;
	}
	return NO;
}

/**
 * Checks if a class conforms to a protocol.
 */
BOOL class_conformsTo(Class cls, objc_protocol_t *otherProtocol)
{
	if (cls == Nil || otherProtocol == NULL) {
		return NO;
	}

	struct objc_protocol_list *proto = cls->protocols;
	while (proto != NULL) {
		for (size_t i = 0; i < proto->count; i++) {
			if (proto_conformsTo(proto->protocols[i], otherProtocol)) {
				return YES;
			}
		}
		proto = proto->next;
	}

	Class superclass = class_getSuperclass(cls);
	if (superclass != Nil) {
		return class_conformsTo(superclass, otherProtocol);
	}

	return NO;
}
