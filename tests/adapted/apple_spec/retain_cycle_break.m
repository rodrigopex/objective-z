/*
 * Behavioral spec derived from: Apple ARC documentation
 * This test is ORIGINAL CODE — no Apple code was copied.
 * Pattern: a parent/child pair that would cycle, broken the way this
 * backend documents — an `__unsafe_unretained` back-reference.
 *
 * This file used to contain no reference between its two nodes at all: it
 * allocated two independent objects, set an integer tag on each, read them
 * back, and asserted the tags. Nothing in it could fail if a cycle leaked
 * forever, because nothing in it made a cycle (#455). It now builds the
 * shape it is named for.
 *
 * Why the remedy is `__unsafe_unretained` and not `__weak`: there is no
 * zeroing `__weak` here and a `weak` property is a hard error, because
 * nothing would zero the reference without a runtime side table. So a
 * back-reference that must not own is spelled `__unsafe_unretained`, and
 * the owner has to outlive the child — which is what this fixture
 * demonstrates and then proves by re-allocating from exhausted pools.
 */
/* oz-pool: NodeA=1,NodeB=1,CycleTest=1 */
#import "OZTestBase.h"

/* Declared first so `NodeA` can hold one by type. */
@interface NodeB : OZObject {
	/* The back-reference. Owning this would close the cycle. */
	__unsafe_unretained id _owner;
	int _tag;
}
- (void)setOwner:(id)o;
- (void)setTag:(int)t;
- (int)tag;
- (int)hasOwner;
@end

@implementation NodeB
- (void)setOwner:(id)o { _owner = o; }
- (void)setTag:(int)t { _tag = t; }
- (int)tag { return _tag; }
- (int)hasOwner { return (_owner != nil) ? 1 : 0; }
@end

@interface NodeA : OZObject {
	/* Strong: A owns B, so releasing A must release B. */
	NodeB *_next;
	int _tag;
}
- (void)setNext:(NodeB *)n;
- (void)setTag:(int)t;
- (int)tag;
- (int)nextTag;
@end

@implementation NodeA
- (void)setNext:(NodeB *)n { _next = n; }
- (void)setTag:(int)t { _tag = t; }
- (int)tag { return _tag; }
- (int)nextTag { return [_next tag]; }
@end

@interface CycleTest : OZObject {
	int _aTag;
	int _bTag;
	int _linked;
	int _poolIsOne;
	int _brokeOk;
}
- (void)run;
- (int)aTag;
- (int)bTag;
- (int)linked;
- (int)poolIsOne;
- (int)brokeOk;
@end

@implementation CycleTest
- (void)run {
	{
		NodeA *a = [NodeA alloc];
		NodeB *b = [NodeB alloc];
		[a setTag:1];
		[b setTag:2];
		[a setNext:b];   /* strong, downward */
		[b setOwner:a];  /* unretained, upward — this is the break */
		_aTag = [a tag];
		/* Reached through A's strong ivar, so the link is real rather
		 * than two unrelated objects sitting in the same scope. */
		_bTag = [a nextTag];
		_linked = [b hasOwner];

		/* Control: both pools hold one slot, so while `a` is alive a
		 * second NodeA must be refused. Without this, `_brokeOk` below
		 * would pass on a wider pool with nothing ever released. */
		NodeA *denied = [NodeA alloc];
		_poolIsOne = (denied == nil) ? 1 : 0;
	}
	/*
	 * Both slots are free again, which is the whole claim: A released B
	 * on the way out, and B's back-reference did not keep A alive. Had
	 * `_owner` been strong, each would hold the other, neither pool would
	 * recycle, and both of these would come back nil.
	 */
	NodeA *a2 = [NodeA alloc];
	NodeB *b2 = [NodeB alloc];
	_brokeOk = (a2 != nil && b2 != nil) ? 1 : 0;
}
- (int)aTag { return _aTag; }
- (int)bTag { return _bTag; }
- (int)linked { return _linked; }
- (int)poolIsOne { return _poolIsOne; }
- (int)brokeOk { return _brokeOk; }
@end
