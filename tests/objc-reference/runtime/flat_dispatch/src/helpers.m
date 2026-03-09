/*
 * SPDX-License-Identifier: Apache-2.0
 *
 * Test helpers for flat dispatch table validation.
 * Defines a class hierarchy: FDBase → FDChild → FDGrandChild,
 * an independent FDPeer, and a category on FDBase.
 */
#import <Foundation/Foundation.h>

/* ── FDBase ──────────────────────────────────────────────────────── */

@interface FDBase : Object
- (int)value;
- (int)shared;
+ (int)classValue;
@end

@implementation FDBase
- (int)value
{
	return 10;
}
- (int)shared
{
	return 100;
}
+ (int)classValue
{
	return 42;
}
@end

/* ── FDChild : FDBase ────────────────────────────────────────────── */

@interface FDChild : FDBase
- (int)childOnly;
@end

@implementation FDChild
- (int)childOnly
{
	return 20;
}
@end

/* ── FDGrandChild : FDChild ──────────────────────────────────────── */

@interface FDGrandChild : FDChild
@end

@implementation FDGrandChild
@end

/* ── FDPeer (independent root) ───────────────────────────────────── */

@interface FDPeer : Object
- (int)value;
- (int)shared;
@end

@implementation FDPeer
- (int)value
{
	return 77;
}
- (int)shared
{
	return 200;
}
@end

/* ── Category: FDBase (Override) ──────────────────────────────────── */

@interface FDBase (Override)
@end

@implementation FDBase (Override)
- (int)shared
{
	return 999;
}
@end

/* ── C-callable wrappers ─────────────────────────────────────────── */

extern void objc_release(id);

__attribute__((ns_returns_retained))
id test_fd_create_base(void)
{
	return [[FDBase alloc] init];
}

__attribute__((ns_returns_retained))
id test_fd_create_child(void)
{
	return [[FDChild alloc] init];
}

__attribute__((ns_returns_retained))
id test_fd_create_grandchild(void)
{
	return [[FDGrandChild alloc] init];
}

__attribute__((ns_returns_retained))
id test_fd_create_peer(void)
{
	return [[FDPeer alloc] init];
}

void test_fd_dealloc(id obj)
{
	objc_release(obj);
}

int test_fd_call_value(id obj)
{
	return [obj value];
}

int test_fd_call_shared(id obj)
{
	return [obj shared];
}

int test_fd_call_class_value(void)
{
	return [FDBase classValue];
}

int test_fd_call_child_only(id obj)
{
	return [obj childOnly];
}

Class test_fd_get_base_class(void)
{
	return [FDBase class];
}
