/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Assert stubs for OZ transpiler AST dumps.
 * Declares oz_assert functions so Clang preserves calls in the AST.
 * The generated C includes platform/oz_assert.h which provides the real macros.
 */
#pragma once

static inline void oz_assert(int expr)
{
        (void)expr;
}

static inline void oz_assert_msg(int expr, const char *msg)
{
        (void)expr;
        (void)msg;
}

static inline void oz_assert_unreachable(void)
{
}
