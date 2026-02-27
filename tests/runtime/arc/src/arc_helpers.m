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
