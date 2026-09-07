/* A +1 result bound to nothing is still released (#322).
 *
 * `-copy` is +1 by convention (`arc::is_owning_selector`), so a send of it
 * whose result is discarded hands back a reference no variable holds and
 * no scope exit can release. Two shapes, because the defect is not
 * specific to reflection: a direct send, and the same selector reached
 * through a `SEL`.
 *
 * A leak passes any assertion about return values -- the blindness #283
 * recorded -- so each entry point instead reports whether a *later*
 * allocation still finds a slab slot. With the abandoned reference
 * released the two-slot slab always has one free; with it leaked the slab
 * is full and `[Thing alloc]` answers nil. That makes the leak a Unity
 * failure on any host, which matters because macOS arm64 has no
 * `-fsanitize=leak` and cannot run the corpus's `--check-leaks` gate at
 * all. `Thing=2` is exact: peak live is `t` plus the copy, and one slot
 * short of the leak.
 */
/* oz-pool: Thing=2 */
#import "OZTestBase.h"

@interface Thing : OZObject
- (instancetype)copy;
- (void)poke;
@end

@implementation Thing
- (instancetype)copy
{
	return [Thing alloc];
}
- (void)poke {}
@end

/* An owning method sent directly, its +1 result discarded. */
int run_discarded_direct(void)
{
	Thing *t = [Thing alloc];

	[t copy];

	Thing *slot = [Thing alloc];
	int slot_free = (slot != nil);

	[t poke];
	return slot_free;
}

/* The same owning method reached through a SEL held in a local. */
int run_discarded_via_selector(void)
{
	Thing *t = [Thing alloc];
	SEL c = @selector(copy);

	[t performSelector:c];

	Thing *slot = [Thing alloc];
	int slot_free = (slot != nil);

	[t poke];
	return slot_free;
}
