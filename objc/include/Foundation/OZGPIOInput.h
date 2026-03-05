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
#import "OZGPIOPin.h"

/**
 * @brief Block type for GPIO ISR callbacks.
 *
 * Uses Zephyr's native callback signature. Must be a global block
 * (no captured variables) — use file-scope variables to share
 * state between the ISR block and application code.
 */
typedef void (^OZGPIOISRBlock)(const struct device *port,
			       struct gpio_callback *cb,
			       gpio_port_pins_t pins);

/**
 * @brief Wrapper struct for CONTAINER_OF recovery in ISR trampoline.
 *
 * Groups the Zephyr gpio_callback with the user block so the static
 * trampoline can recover the block via CONTAINER_OF on the callback
 * pointer (safe with non-fragile ivar offsets).
 */
struct ozgpio_isr_ctx {
	struct gpio_callback cb;
	OZGPIOISRBlock blockCallback;
};

/**
 * @brief GPIO input pin with interrupt support.
 * @headerfile OZGPIOInput.h Foundation/Foundation.h
 * @ingroup objc
 */
@interface OZGPIOInput : OZGPIOPin {
@private
	struct ozgpio_isr_ctx _isr;
}

/**
 * @brief Initialize with a C function pointer callback.
 * @param spec     Pointer to the devicetree GPIO spec (copied by value).
 * @param flags    Interrupt flags (e.g. GPIO_INT_EDGE_TO_ACTIVE).
 * @param callback Zephyr gpio_callback_handler_t (registered directly).
 * @return Initialized instance, or nil on configuration failure.
 */
- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec
	       flags:(gpio_flags_t)flags
	    callback:(gpio_callback_handler_t)callback;

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
       blockCallback:(OZGPIOISRBlock)blockCallback;

/**
 * @brief Get the logical active state of the pin.
 * @return YES if pin is logically active, NO otherwise.
 */
- (BOOL)isActive;

@end
