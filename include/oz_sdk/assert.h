/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Assert stub for OZ transpiler AST dumps.
 * Declares __ASSERT_NO_MSG as a function so Clang preserves the call in the AST.
 * The generated C includes <zephyr/kernel.h> which provides the real macro.
 */
#pragma once

void __ASSERT_NO_MSG(int test);

#define assert(expr) __ASSERT_NO_MSG(expr)
