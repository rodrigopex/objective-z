/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Foundation stub for OZ transpiler AST dumps.
 * Maps Foundation types to their OZ transpiler equivalents.
 */
#pragma once

#import "OZObject.h"
#import "OZString.h"
#import "OZMutableString.h"
#import "OZQ31.h"
#import "OZArray.h"
#import "OZDictionary.h"
#import "OZHeap.h"
#import "OZDefer.h"
#import "OZMacro.h"
#import "OZLog.h"
#import "OZSpinLock.h"
#import "Singleton+Protocol.h"

/*
 * `oz_assert` and friends, as `static inline` stubs. Not a Foundation class,
 * but every `.m` that asserts needs them declared or Clang reports
 * `call to undeclared function 'oz_assert'` -- which lands in the AST dump
 * oz2c reads as its ivar-ownership oracle, and in clangd (#304).
 * `../assert.h` rather than `<assert.h>`: `-I include/oz_sdk` is prepended to
 * the dump flags, so the unqualified spelling is ambiguous with libc's.
 */
#import "../assert.h"
