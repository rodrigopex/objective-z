/*
 * Copyright (c) 2025 Rodrigo Peixoto <rodrigopex@gmail.com>
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * @file arc_helpers.m
 * @brief ARC-compiled helper classes and test functions.
 *
 * Compiled with -fobjc-arc. Clang generates objc_retain, objc_release,
 * .cxx_destruct, objc_storeStrong, and objc_autoreleaseReturnValue calls.
 */
#import <Foundation/Foundation.h>

/* ── Tracking globals (defined in helpers.m) ───────────────────── */

extern int g_dealloc_count;
extern int g_dealloc_tags[];
extern int g_dealloc_tag_idx;

/* ── Forward-declare MRR classes (defined in helpers.m) ────────── */

@interface TrackedObj : Object
- (id)initWithTag:(int)tag;
- (int)tag;
@end

@interface ArcPoolObj : Object
- (id)initWithTag:(int)tag;
- (int)tag;
@end

/* ══════════════════════════════════════════════════════════════════
 *  ARC Classes
 * ══════════════════════════════════════════════════════════════════ */

/* ── IvarOwner: single strong ivar (.cxx_destruct test) ────────── */

@interface IvarOwner : Object {
	id _child;
}
@property (nonatomic, strong) id child;
@end

@implementation IvarOwner

@synthesize child = _child;

- (void)dealloc
{
	g_dealloc_count++;
	if (g_dealloc_tag_idx < 64) {
		g_dealloc_tags[g_dealloc_tag_idx++] = -1;
	}
}

@end

/* ── MultiIvarOwner: two strong ivars ──────────────────────────── */

@interface MultiIvarOwner : Object {
	id _childA;
	id _childB;
}
@property (nonatomic, strong) id childA;
@property (nonatomic, strong) id childB;
@end

@implementation MultiIvarOwner

@synthesize childA = _childA;
@synthesize childB = _childB;

- (void)dealloc
{
	g_dealloc_count++;
	if (g_dealloc_tag_idx < 64) {
		g_dealloc_tags[g_dealloc_tag_idx++] = -2;
	}
}

@end

/* ── SubIvarOwner: extends IvarOwner, adds extra strong ivar ───── */

@interface SubIvarOwner : IvarOwner {
	id _extra;
}
@property (nonatomic, strong) id extra;
@end

@implementation SubIvarOwner

@synthesize extra = _extra;

- (void)dealloc
{
	g_dealloc_count++;
	if (g_dealloc_tag_idx < 64) {
		g_dealloc_tags[g_dealloc_tag_idx++] = -3;
	}
}

@end

/* ── ChainNode: strong next pointer for graph tests ────────────── */

@interface ChainNode : Object {
	ChainNode *_next;
	int _tag;
}
@property (nonatomic, strong) ChainNode *next;
@property (nonatomic, assign) int tag;
@end

@implementation ChainNode

@synthesize next = _next;
@synthesize tag = _tag;

- (void)dealloc
{
	g_dealloc_count++;
	if (g_dealloc_tag_idx < 64) {
		g_dealloc_tags[g_dealloc_tag_idx++] = _tag;
	}
}

@end

/* ── PropHolderAtomic: @property(atomic, strong) ───────────────── */

@interface PropHolderAtomic : Object {
	id _thing;
}
@property (atomic, strong) id thing;
@end

@implementation PropHolderAtomic

@synthesize thing = _thing;

- (void)dealloc
{
	g_dealloc_count++;
}

@end

/* ── PropHolderNonatomic: @property(nonatomic, strong) ─────────── */

@interface PropHolderNonatomic : Object {
	id _thing;
}
@property (nonatomic, strong) id thing;
@end

@implementation PropHolderNonatomic

@synthesize thing = _thing;

- (void)dealloc
{
	g_dealloc_count++;
}

@end

/* ══════════════════════════════════════════════════════════════════
 *  Suite 1: arc_scope — Scope lifecycle tests
 * ══════════════════════════════════════════════════════════════════ */

void arc_test_scope_single(void)
{
	TrackedObj *obj = [[TrackedObj alloc] initWithTag:1];
	(void)obj;
	/* ARC releases obj at scope exit */
}

void arc_test_scope_multi_reverse(void)
{
	TrackedObj *a = [[TrackedObj alloc] initWithTag:1];
	TrackedObj *b = [[TrackedObj alloc] initWithTag:2];
	TrackedObj *c = [[TrackedObj alloc] initWithTag:3];
	(void)a;
	(void)b;
	(void)c;
	/* ARC releases c, b, a in reverse order */
}

void arc_test_scope_nested(void)
{
	TrackedObj *outer = [[TrackedObj alloc] initWithTag:100];
	{
		TrackedObj *inner = [[TrackedObj alloc] initWithTag:200];
		(void)inner;
		/* inner released here */
	}
	(void)outer;
	/* outer released here */
}

static void arc_test_early_return_inner(int should_return)
{
	TrackedObj *obj = [[TrackedObj alloc] initWithTag:42];
	(void)obj;
	if (should_return) {
		return;
		/* ARC releases obj on early return */
	}
	/* ARC releases obj at function end */
}

void arc_test_scope_early_return(void)
{
	arc_test_early_return_inner(1);
}

void arc_test_scope_loop(void)
{
	for (int i = 0; i < 5; i++) {
		TrackedObj *obj = [[TrackedObj alloc] initWithTag:i];
		(void)obj;
		/* ARC releases obj at end of each iteration */
	}
}

void arc_test_scope_cond_true(void)
{
	if (1) {
		TrackedObj *obj = [[TrackedObj alloc] initWithTag:10];
		(void)obj;
	}
}

void arc_test_scope_cond_false(void)
{
	if (0) {
		TrackedObj *obj = [[TrackedObj alloc] initWithTag:99];
		(void)obj;
	} else {
		TrackedObj *obj = [[TrackedObj alloc] initWithTag:20];
		(void)obj;
	}
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 2: arc_cxx_destruct — Strong ivar cleanup
 * ══════════════════════════════════════════════════════════════════ */

void arc_test_cxx_single_ivar(void)
{
	IvarOwner *owner = [[IvarOwner alloc] init];
	owner.child = [[TrackedObj alloc] initWithTag:10];
	(void)owner;
	/* ARC releases owner → .cxx_destruct releases child → both dealloc */
}

void arc_test_cxx_multi_ivar(void)
{
	MultiIvarOwner *owner = [[MultiIvarOwner alloc] init];
	owner.childA = [[TrackedObj alloc] initWithTag:10];
	owner.childB = [[TrackedObj alloc] initWithTag:20];
	(void)owner;
	/* ARC releases owner → .cxx_destruct releases childA + childB */
}

void arc_test_cxx_hierarchy(void)
{
	SubIvarOwner *owner = [[SubIvarOwner alloc] init];
	owner.child = [[TrackedObj alloc] initWithTag:10];
	owner.extra = [[TrackedObj alloc] initWithTag:20];
	(void)owner;
	/* ARC releases owner → .cxx_destruct walks hierarchy:
	 * SubIvarOwner releases extra, IvarOwner releases child */
}

void arc_test_cxx_chain(void)
{
	IvarOwner *a = [[IvarOwner alloc] init];
	IvarOwner *b = [[IvarOwner alloc] init];
	TrackedObj *leaf = [[TrackedObj alloc] initWithTag:99];
	a.child = b;
	b.child = leaf;
	(void)a;
	/* Releasing a → .cxx_destruct releases b → .cxx_destruct releases leaf */
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 3: arc_autorelease — Pool tests (ARC-compiled)
 * ══════════════════════════════════════════════════════════════════ */

/*
 * Under ARC, [obj autorelease] is forbidden. Instead, we use
 * @autoreleasepool {} with a factory-style return that ARC
 * implicitly autoreleases.
 */
static TrackedObj *arc_create_autoreleased(int tag)
{
	return [[TrackedObj alloc] initWithTag:tag];
}

void arc_test_autoreleasepool_syntax(void)
{
	@autoreleasepool {
		/*
		 * ARC sees the return from arc_create_autoreleased as a +0
		 * (autoreleased) value. The pool captures it and drains it
		 * when the block exits.
		 */
		TrackedObj *obj = arc_create_autoreleased(77);
		(void)obj;
	}
	/* Pool drained → obj released → dealloc fires */
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 4: arc_rvo — Return value optimization
 * ══════════════════════════════════════════════════════════════════ */

static TrackedObj *arc_factory_create(int tag)
{
	/* Clang emits objc_autoreleaseReturnValue here */
	return [[TrackedObj alloc] initWithTag:tag];
}

static TrackedObj *arc_intermediate(int tag)
{
	/* Clang emits objc_retainAutoreleasedReturnValue + objc_autoreleaseReturnValue */
	return arc_factory_create(tag);
}

void arc_test_rvo_round_trip(void)
{
	/* Consumer: Clang emits objc_retainAutoreleasedReturnValue */
	TrackedObj *obj = arc_factory_create(50);
	(void)obj;
	/* ARC releases at scope exit → dealloc */
}

void arc_test_rvo_chain(void)
{
	TrackedObj *obj = arc_intermediate(60);
	(void)obj;
	/* Passes through intermediate, still 1 dealloc */
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 5: arc_graph — Object graph lifecycle
 * ══════════════════════════════════════════════════════════════════ */

void arc_test_graph_linear(void)
{
	ChainNode *a = [[ChainNode alloc] init];
	ChainNode *b = [[ChainNode alloc] init];
	a.tag = 1;
	b.tag = 2;
	a.next = b;
	(void)a;
	/* Releasing a → .cxx_destruct releases b → both dealloc */
}

void arc_test_graph_deep_chain(void)
{
	ChainNode *a = [[ChainNode alloc] init];
	ChainNode *b = [[ChainNode alloc] init];
	ChainNode *c = [[ChainNode alloc] init];
	ChainNode *d = [[ChainNode alloc] init];
	a.tag = 1;
	b.tag = 2;
	c.tag = 3;
	d.tag = 4;
	a.next = b;
	b.next = c;
	c.next = d;
	(void)a;
	/* Releasing a → cascading .cxx_destruct → all 4 dealloc */
}

void arc_test_graph_retain_cycle(void)
{
	ChainNode *a = [[ChainNode alloc] init];
	ChainNode *b = [[ChainNode alloc] init];
	a.tag = 1;
	b.tag = 2;
	a.next = b;
	b.next = a;
	/*
	 * Both a and b go out of scope. ARC releases local refs but the
	 * cycle keeps both alive. Neither deallocs. This is EXPECTED
	 * behavior — no weak refs in this runtime.
	 */
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 7: arc_pool — Static slab + ARC (ARC-compiled alloc)
 * ══════════════════════════════════════════════════════════════════ */

void arc_test_pool_scope(void)
{
	ArcPoolObj *obj = [[ArcPoolObj alloc] initWithTag:1];
	(void)obj;
	/* ARC releases at scope exit → dealloc → slab freed */
}

void arc_test_pool_cycle(int count)
{
	for (int i = 0; i < count; i++) {
		ArcPoolObj *obj = [[ArcPoolObj alloc] initWithTag:i];
		(void)obj;
	}
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 9: arc_property — ARC-compiled property tests
 * ══════════════════════════════════════════════════════════════════ */

void arc_test_prop_atomic(void)
{
	PropHolderAtomic *holder = [[PropHolderAtomic alloc] init];
	TrackedObj *val = [[TrackedObj alloc] initWithTag:1];
	holder.thing = val;
	(void)holder;
	/* ARC releases holder → .cxx_destruct releases thing → both dealloc */
}

void arc_test_prop_nonatomic(void)
{
	PropHolderNonatomic *holder = [[PropHolderNonatomic alloc] init];
	TrackedObj *val = [[TrackedObj alloc] initWithTag:1];
	holder.thing = val;
	(void)holder;
}

void arc_test_prop_overwrite(void)
{
	PropHolderNonatomic *holder = [[PropHolderNonatomic alloc] init];
	TrackedObj *a = [[TrackedObj alloc] initWithTag:10];
	TrackedObj *b = [[TrackedObj alloc] initWithTag:20];
	holder.thing = a;
	holder.thing = b; /* a released by property setter */
	(void)holder;
	/* holder dealloc → .cxx_destruct releases b */
}

void arc_test_prop_same_object(void)
{
	PropHolderNonatomic *holder = [[PropHolderNonatomic alloc] init];
	TrackedObj *val = [[TrackedObj alloc] initWithTag:3];
	holder.thing = val;
	holder.thing = val; /* same object — should be no-op or safe */
	(void)holder;
}

/* ══════════════════════════════════════════════════════════════════
 *  Suite 10: arc_stress — Stress tests (ARC-compiled)
 * ══════════════════════════════════════════════════════════════════ */

void arc_test_stress_alloc_loop(void)
{
	for (int i = 0; i < 100; i++) {
		TrackedObj *obj = [[TrackedObj alloc] initWithTag:i];
		(void)obj;
	}
}

void arc_test_stress_autorelease_capacity(void)
{
	@autoreleasepool {
		for (int i = 0; i < 60; i++) {
			TrackedObj *obj = arc_create_autoreleased(i);
			(void)obj;
		}
	}
}
