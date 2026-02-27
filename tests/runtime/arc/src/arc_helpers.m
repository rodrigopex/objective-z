/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file arc_helpers.m
 * @brief ARC-compiled helpers for ARC tests.
 *
 * Compiled with -fobjc-arc.  When the local variable goes out of
 * scope, ARC inserts an objc_release that triggers dealloc.
 */
#import <objc/objc.h>

/* Dealloc tracking counter (defined in helpers.m) */
extern int g_arc_dealloc_count;

/* Forward-declare the class defined in helpers.m (MRR side) */
@interface ArcTestObj : Object
@end

void test_arc_scope_cleanup(void)
{
	/* ARC retains the return value of alloc/init, then releases
	 * at end of scope — triggering dealloc. */
	ArcTestObj *obj = [[ArcTestObj alloc] init];
	(void)obj;
}

/* ── Atomic property integration test ──────────────────────────── */

@interface PropHolder : Object
@property (atomic, strong) id thing;
@end

@implementation PropHolder

- (void)dealloc
{
	g_arc_dealloc_count++;
}

@end

void test_arc_atomic_property(void)
{
	/*
	 * ARC scope: create a PropHolder, assign an ArcTestObj to its
	 * atomic property (exercises Clang-emitted objc_setProperty),
	 * then let both go out of scope.  Both deallocs should fire.
	 */
	PropHolder *holder = [[PropHolder alloc] init];
	ArcTestObj *val = [[ArcTestObj alloc] init];
	holder.thing = val;
	(void)holder;
}
