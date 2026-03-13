/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZGPIOInput.h
 * @brief GPIO input pin wrapper with interrupt callbacks.
 *
 * Wraps a Zephyr GPIO input pin and registers an ISR callback
 * via either a plain C function pointer or a global block
 * (no captured variables).
 */
#pragma once
#import "GPIOPin.h"
#include <zephyr/kernel.h>
#include <zephyr/drivers/gpio.h>
/**
 * @brief GPIO input pin with interrupt support.
 * @headerfile OZGPIOInput.h Foundation/Foundation.h
 * @ingroup objc
 */
@interface GPIOInput: GPIOPin
/**
 * @brief Initialize with a global block callback.
 * @param spec          Pointer to the devicetree GPIO spec (copied by value).
 * @param flags         Interrupt flags (e.g. GPIO_INT_EDGE_TO_ACTIVE).
 * @param blockCallback Global block (must not capture local variables).
 * @return Initialized instance, or nil on configuration failure
 *         or if the block captures variables.
 */
- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec
		flags:(gpio_flags_t)flags
	blockCallback:(void (^)(const struct device *port, struct gpio_callback *cb,
				gpio_port_pins_t pins))blockCallback;

/**
 * @brief Get the logical active state of the pin.
 * @return YES if pin is logically active, NO otherwise.
 */
- (BOOL)isActive;

@end
