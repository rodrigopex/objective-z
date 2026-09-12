/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Platform Abstraction Layer — Assert macros
 */
#ifndef OZ_ASSERT_H
#define OZ_ASSERT_H

#ifdef OZ_PLATFORM_ZEPHYR

#include <zephyr/sys/__assert.h>

#define oz_assert(expr)              __ASSERT_NO_MSG(expr)
/*
 * Zephyr's __ASSERT(test, fmt, ...) is variadic, so passing only `msg` leaves
 * the "..." empty -- an ISO C99 constraint violation that `just test-pedantic`
 * reports as "requires at least one argument for the '...'". Passing the
 * message through "%s" supplies that argument, and stops a message containing
 * a '%' being read as a conversion specifier at the same time.
 */
#define oz_assert_msg(expr, msg)     __ASSERT(expr, "%s", msg)
#define oz_assert_unreachable()      __ASSERT(0, "%s", "unreachable")

#elif defined(OZ_PLATFORM_HOST)

#include <assert.h>
#include <stdio.h>

#define oz_assert(expr)              assert(expr)
#define oz_assert_msg(expr, msg)                                               \
        do {                                                                   \
                if (!(expr)) {                                                 \
                        fprintf(stderr, "Assertion failed: %s\n", msg);        \
                        assert(0);                                             \
                }                                                              \
        } while (0)
#define oz_assert_unreachable()      assert(0 && "unreachable")

#else
#error "Define OZ_PLATFORM_ZEPHYR or OZ_PLATFORM_HOST"
#endif

#endif /* OZ_ASSERT_H */
