/**
 * @file runtime-gcc/objc/assert.h
 * @brief Defines a custom assertion macro.
 * @details This file provides an `assert` macro that can be used for debugging
 * purposes. The assertion is only active when the `DEBUG` preprocessor macro is
 * defined.
 */

#pragma once
#include <zephyr/kernel.h>

/**
 * @def objc_assert(condition)
 * @ingroup objc
 * @brief Asserts that a condition is true.
 * @param condition The condition to check.
 * @details If the `DEBUG` macro is defined, this macro will check the given
 * condition. If the condition is false, it will call `panicf` with an assertion
 * failure message, including the condition, file name, and line number. If
 * `DEBUG` is not defined, this macro does nothing.
 */
#define objc_assert(condition) __ASSERT(condition)
