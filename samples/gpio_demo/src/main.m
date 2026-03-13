/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

#import <Foundation/Foundation.h>
#import "GPIOOutput.h"
#import "GPIOInput.h"
#include <zephyr/kernel.h>

static const struct gpio_dt_spec kLedSpec = GPIO_DT_SPEC_GET(DT_ALIAS(led0), gpios);
static const struct gpio_dt_spec kBtnSpec = GPIO_DT_SPEC_GET(DT_ALIAS(sw0), gpios);

static GPIOOutput *led;
static GPIOInput *btn;

int main(void)
{
	OZLog("=== GPIO Demo ===");

	led = [[GPIOOutput alloc] initWithDTSpec:&kLedSpec flags:0];
	if (led != nil) {
		OZLog("LED configured");
	}

	btn = [[GPIOInput alloc] initWithDTSpec:&kBtnSpec
					  flags:GPIO_INT_EDGE_TO_ACTIVE
				  blockCallback:^(const struct device *port,
						  struct gpio_callback *cb, gpio_port_pins_t pins) {
				    OZLog("Button pressed (%lld)!", k_uptime_get());
				    [led toggle];
				  }];

	if (btn != nil) {
		OZLog("Button configured");
	}

	[led setActive:YES];
	OZLog("LED on");

	k_msleep(1000);

	[led setActive:NO];
	OZLog("LED off");

	k_msleep(1000);

	[led toggle];
	OZLog("LED toggled");

	OZLog("=== GPIO Demo complete ===");

	return 0;
}
