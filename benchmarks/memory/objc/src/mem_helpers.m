/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 *
 * ObjC memory benchmark helpers.
 * Transpiled to plain C via objz_transpile_sources().
 */

#import "OZObject.h"

/* ── MemBase: OZObject (8) = 8 bytes ──────────────────────────────── */

@interface MemBase : OZObject
- (void)nop;
- (int)getValue;
@end

@implementation MemBase

- (void)nop
{
}

- (int)getValue
{
	return 0;
}

@end

/* ── MemChild: MemBase (8) + _field_a (4) = 12 bytes ─────────────── */

@interface MemChild : MemBase {
	int _field_a;
}
@end

@implementation MemChild
@end

/* ── MemGrandChild: MemChild (12) + _field_b (4) = 16 bytes ──────── */

@interface MemGrandChild : MemChild {
	int _field_b;
}
@end

@implementation MemGrandChild
@end
