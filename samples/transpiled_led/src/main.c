/* SPDX-License-Identifier: Apache-2.0
 *
 * Transpiled LED demo — pure C, no ObjC runtime.
 * Uses generated C code from oz_transpile.
 */
#include <zephyr/kernel.h>
#include <zephyr/sys/printk.h>
#include "generated/OZLed.h"
#include "generated/oz_mem_slabs.h"
#include "generated/oz_dispatch.h"

/* Register vtable entries at boot */
static void oz_register_vtables(void)
{
	OZ_vtable_init[OZ_CLASS_OZObject] = (OZ_fn_init)OZObject_init;
	OZ_vtable_init[OZ_CLASS_OZLed] = (OZ_fn_init)OZObject_init;
	OZ_vtable_dealloc[OZ_CLASS_OZObject] = (OZ_fn_dealloc)OZObject_dealloc;
	OZ_vtable_dealloc[OZ_CLASS_OZLed] = (OZ_fn_dealloc)OZLed_dealloc;
	OZ_vtable_toggle[OZ_CLASS_OZLed] = (OZ_fn_toggle)OZLed_toggle;
}

int main(void)
{
	oz_register_vtables();

	struct OZLed *led = OZLed_alloc();
	if (!led) {
		printk("Failed to allocate OZLed\n");
		return 1;
	}

	led = OZLed_initWithPin_(led, 13);
	printk("LED on pin %d, state=%d\n", OZLed_pin(led), OZLed_state(led));

	OZLed_turnOn(led);
	printk("After turnOn: state=%d\n", OZLed_state(led));

	OZLed_toggle(led);
	printk("After toggle: state=%d\n", OZLed_state(led));

	OZLed_toggle(led);
	printk("After toggle: state=%d\n", OZLed_state(led));

	OZLed_free(led);
	printk("Transpiled LED demo complete\n");
	return 0;
}
