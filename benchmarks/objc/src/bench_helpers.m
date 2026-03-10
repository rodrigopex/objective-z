/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * Benchmark helper classes for transpiler benchmarks.
 * Transpiled to plain C via objz_transpile_sources().
 */

#import "OZObject.h"

/* ── Protocol to force vtable (PROTOCOL) dispatch ─────────────────── */

@protocol Benchable
- (void)nop;
- (int)getValue;
@end

/* ── BenchBase: direct methods (depth=0) ──────────────────────────── */

@interface BenchBase : OZObject <Benchable> {
	int _x;
}
- (void)nop;
- (int)getValue;
+ (void)classNop;
@end

@implementation BenchBase

- (void)nop
{
}

- (int)getValue
{
	return _x;
}

+ (void)classNop
{
}

@end

/* ── BenchChild: inherits from BenchBase (depth=1) ────────────────── */

@interface BenchChild : BenchBase
@end

@implementation BenchChild
@end

/* ── BenchGrandChild: inherits from BenchChild (depth=2) ──────────── */

@interface BenchGrandChild : BenchChild
@end

@implementation BenchGrandChild
@end

