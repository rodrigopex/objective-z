/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

#import <Foundation/Foundation.h>
#import "GPIOInput.h"
#include <zephyr/kernel.h>

// static const struct gpio_dt_spec kLedSpec = GPIO_DT_SPEC_GET(DT_ALIAS(led0), gpios);
static const struct gpio_dt_spec kBtnSpec = GPIO_DT_SPEC_GET(DT_ALIAS(sw0), gpios);

// static GPIOOutput *led;
static GPIOInput *btn;

int main(void)
{
	printk("=== GPIO Demo ===\n");

	// led = [[OZGPIOOutput alloc] initWithDTSpec:&kLedSpec flags:0];
	// if (led != nil) {
	// 	printk("LED configured\n");
	// }

	/* Block callback variant */
	btn = [[GPIOInput alloc] initWithDTSpec:&kBtnSpec
					  flags:GPIO_INT_EDGE_TO_ACTIVE
				  blockCallback:^(const struct device *port,
						  struct gpio_callback *cb, gpio_port_pins_t pins) {
				    printk("Button pressed (%lld)!\n", k_uptime_get());
				    // [led toggle];
				  }];

	/* C callback variant (uncomment to use instead):
	btn = [[OZGPIOInput alloc]
		initWithDTSpec:&kBtnSpec
			 flags:GPIO_INT_EDGE_TO_ACTIVE
		      callback:btn_callback];
	*/

	if (btn != nil) {
		printk("Button configured\n");
	}

	// [led setActive:YES];
	// printk("LED on\n");

	// k_msleep(1000);

	// [led setActive:NO];
	// printk("LED off\n");

	// k_msleep(1000);

	// [led toggle];
	// printk("LED toggled\n");

	printk("=== GPIO Demo complete ===\n");

	return 0;
}
