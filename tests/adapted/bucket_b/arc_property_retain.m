/*
 * Adapted from: tests/objc-reference/runtime/arc/src/main.c
 * Pattern: a strong object property owns its target.
 *
 * Proves: the synthesized strong setter takes a reference of its own. The
 * target is allocated in an inner scope and the local's reference dies at the
 * closing brace, so a slot still outstanding afterwards can only be the
 * property's. The pool is one slot and the test asserts that first, because a
 * reuse claim on a wider pool passes with nothing ever retained (#455).
 *
 * Does not prove: which refcount value the setter arrived at. Counting is
 * `tests/behavior/cases/properties/strong_vs_assign.m`, which reads
 * `oz_retain_count` directly and also covers replace-old-on-overwrite. This
 * case is its **complement, not a duplicate**: an exhausted slab observes that
 * the allocator really still holds the block, which a counter cannot -- eager
 * allocation balances, so only a side effect catches it (`docs/ARC.md`, on
 * ownership_matrix's blind spots, #376).
 *
 * Until #596 this file asserted only `itemTag == 77` while holding its own
 * reference to the target for the whole test, so every assertion passed
 * against a setter that took no reference at all. Its header nevertheless
 * said "verifies property setter retains new value correctly". The cause was
 * the adaptation note above it: "removed objc_getProperty/objc_setProperty
 * introspection, replaced with direct property access and slab reuse" --
 * except the oracle it dropped was `oz_retain_count`, which is a plain C call
 * and not introspection. See tests/adapted/README.md.
 *
 * The owner's own dealloc releasing the ivar is covered too, but by the
 * exit-time census rather than an assertion here: the driver releases the
 * holder, and a retained-and-never-released target fails the run (#451).
 */
/* oz-pool: Held=1,PropHolder=1 */
#import "OZTestBase.h"

@interface Held : OZObject {
	int _tag;
}
- (void)setTag:(int)t;
- (int)tag;
@end

@implementation Held
- (void)setTag:(int)t { _tag = t; }
- (int)tag { return _tag; }
@end

@interface PropHolder : OZObject {
	Held *_item;
	int _poolIsOne;
	int _tagThroughProperty;
	int _propertyKeepsItAlive;
}
@property (nonatomic, strong) Held *item;
- (void)run;
- (int)itemTag;
- (int)poolIsOne;
- (int)tagThroughProperty;
- (int)propertyKeepsItAlive;
@end

@implementation PropHolder
@synthesize item = _item;

- (int)itemTag {
	return [_item tag];
}

- (void)run {
	/*
	 * The control. One slot, so while `probe` is alive a second allocation
	 * must be refused. Without this the claim below would pass on a wider
	 * pool with nothing retained.
	 */
	{
		Held *probe = [Held alloc];
		Held *denied = [Held alloc];
		_poolIsOne = (denied == nil && probe != nil) ? 1 : 0;
	}

	/*
	 * The claim. `owned`'s reference is gone at the closing brace, so
	 * anything still holding the slot is the property.
	 */
	{
		Held *owned = [Held alloc];
		[owned setTag:77];
		[self setItem:owned];
		_tagThroughProperty = [self itemTag];
	}
	Held *contested = [Held alloc];
	_propertyKeepsItAlive = (contested == nil) ? 1 : 0;
}

- (int)poolIsOne { return _poolIsOne; }
- (int)tagThroughProperty { return _tagThroughProperty; }
- (int)propertyKeepsItAlive { return _propertyKeepsItAlive; }
@end
