/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file OZGPIOOutput.h
 * @brief GPIO output pin wrapper.
 *
 * Provides set, get, and toggle operations for a GPIO output pin
 * configured from a Zephyr devicetree spec.
 */
#pragma once
#import "OZGPIOPin.h"

/**
 * @brief GPIO output pin.
 * @headerfile OZGPIOOutput.h Foundation/Foundation.h
 * @ingroup objc
 */
@interface OZGPIOOutput : OZGPIOPin {
@private
	BOOL _active;
}

/**
 * @brief Initialize a GPIO output pin.
 * @param spec Pointer to the devicetree GPIO spec (copied by value).
 * @param flags Extra GPIO flags (ORed with GPIO_OUTPUT).
 * @return Initialized instance, or nil on configuration failure.
 */
- (id)initWithDTSpec:(const struct gpio_dt_spec *)spec
	       flags:(gpio_flags_t)flags;

/**
 * @brief Get the logical active state of the pin.
 * @return YES if pin is logically active, NO otherwise.
 */
- (BOOL)isActive;

/**
 * @brief Set the logical active state of the pin.
 * @param active YES to set active, NO to set inactive.
 */
- (void)setActive:(BOOL)active;

/**
 * @brief Toggle the pin output.
 */
- (void)toggle;

@end
