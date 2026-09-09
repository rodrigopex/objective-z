/* `self->_ivar = value` lowers exactly like `_ivar = value` (#352).
 *
 * The strong-store lowering matched only the bare-identifier spelling of
 * the ivar, because it opened with
 *
 *     if left.kind() != "identifier" { return None; }
 *
 * and `self->_x` is a `field_expression`. So the explicit spelling became a
 * plain C store: no retain of the new value, no release of the old. Two
 * spellings of one operation with opposite ownership behaviour, over a
 * choice that says nothing about ownership.
 *
 * One missing retain, three defects in sequence: the storing method's own
 * scope-exit release destroyed the object immediately, the ivar was left
 * dangling, and the synthesized dealloc released that freed block a second
 * time when the owner died.
 *
 * Two observables, because the two failures show up differently -- the same
 * split `return_alias_escape.m` documents for #351:
 *
 *   - **Destroyed too early** cannot be seen by counting slots, since a
 *     premature free leaves *more* of them. `run_stored_ivar_survives`
 *     catches it by reading the ivar back after the storing method has
 *     returned, having allocated another instance into what would be the
 *     freed slot: with the bug both names are one object and the tag read
 *     back is the other one's.
 *   - **Released twice** is what `run_repeated_store_balances` is for.
 *     Overwriting a one-slot-per-generation ivar three times can only
 *     succeed if each store released exactly what the ivar held -- no more,
 *     since an over-release would clamp the slab's used count and let a
 *     later allocation hand out a live block.
 *
 * `Thing=2` is exact: the survival test holds the stored instance plus the
 * decoy at once, and two is one slot short of tolerating a leak.
 */
/* oz-pool: Thing=2,Holder=1 */
#import "OZTestBase.h"

@interface Thing : OZObject
{
	int _tag;
}
- (void)setTag:(int)tag;
- (int)tag;
@end

@implementation Thing
- (void)setTag:(int)tag
{
	_tag = tag;
}
- (int)tag
{
	return _tag;
}
@end

@interface Holder : OZObject
{
	Thing *_held;
}
- (void)storeThroughSelf:(int)tag;
- (int)heldTag;
- (int)heldIsSet;
@end

@implementation Holder
/* The spelling under test. Ownership must not depend on it. */
- (void)storeThroughSelf:(int)tag
{
	Thing *t = [Thing alloc];

	[t setTag:tag];
	self->_held = t;
}
- (int)heldTag
{
	return [_held tag];
}
- (int)heldIsSet
{
	return _held != nil;
}
@end

/* The premature-destruction direction. `decoy` is allocated after the
 * store has returned: with the reference unaccounted for, `_held`'s block
 * is already back in the slab, `decoy` is handed the same memory, and the
 * tag read back through `_held` is the decoy's. */
int run_stored_ivar_survives(void)
{
	Holder *h = [Holder alloc];
	Thing *decoy = nil;
	int tag = 0;

	[h storeThroughSelf:7];
	decoy = [Thing alloc];
	[decoy setTag:9];

	tag = [h heldTag];
	return ([h heldIsSet] == 1) && (tag == 7);
}

/* The over-release direction, and the leak direction at once. Three stores
 * through one ivar: each must give back exactly the block it replaced, so
 * the third store still finds a free slot, and the decoy afterwards proves
 * the slab is not confused about how many are live. */
int run_repeated_store_balances(void)
{
	Holder *h = [Holder alloc];
	Thing *decoy = nil;
	int all_set = 0;

	[h storeThroughSelf:1];
	all_set = all_set + [h heldIsSet];
	[h storeThroughSelf:2];
	all_set = all_set + [h heldIsSet];
	[h storeThroughSelf:3];
	all_set = all_set + [h heldIsSet];

	decoy = [Thing alloc];
	[decoy setTag:4];

	return (all_set == 3) && ([h heldTag] == 3) && ([decoy tag] == 4);
}
