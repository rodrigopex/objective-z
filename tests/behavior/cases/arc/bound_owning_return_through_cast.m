/* A +1 result *bound* through a cast is released too (#332).
 *
 * The other half of #327. That one released a `+1` thrown away behind a
 * cast; this one is the reference a strong slot takes over.
 * `arc::is_owning_expr` reads a cast as borrowed -- deliberately, and
 * still does -- and it was what every binding site consulted, so
 *
 *     Thing *t = (Thing *)[Thing alloc];   -- leaked
 *     Thing *u = [Thing alloc];            -- released at scope end
 *
 * differed in whether they leaked, over a cast that changes the static
 * type and says nothing about who owns the reference. `binds_ownership`
 * is what those sites ask now: `is_owning_expr`, plus a non-bridging cast
 * over a reference `created_by` says is genuinely new.
 *
 * Four binding sites, one per entry point, because they fail differently:
 * a local's initialiser leaked, a store to a strong ivar earned an extra
 * *retain* (so the object outlived its owner), a reassignment took the
 * local out of ARC's hands entirely (which `staticbar` then rejected as a
 * loop that escapes its iteration), and a `return` behind a cast released
 * the local on the way out and handed the caller a freed pointer.
 *
 * A leak passes any assertion about return values -- the blindness #283
 * recorded -- so each entry point instead reports whether a *later*
 * allocation still finds a slab slot. That makes the leak a Unity failure
 * on any host, which matters because macOS arm64 has no
 * `-fsanitize=leak` and cannot run the corpus's `--check-leaks` gate at
 * all. `Thing=2` is exact: the widest peak is the ivar overwrite, which
 * is briefly two live instances because that path assigns the new value
 * before releasing the old one, and two is also one slot short of every
 * leak here. Each entry point gives its slots back, so each one starts
 * from an empty slab.
 *
 * `run_init_bound_through_cast` is the double-free direction, and it is
 * honest about what it can and cannot see. `-init...` consumes its
 * receiver's +1 and hands it back, so `Thing *t = (Thing *)[u init];` is
 * one object under two names and exactly one release is owed. Both
 * releases would land at the *same* scope exit, and a second
 * `oz_static_release` on a freed object reads a refcount that is already
 * zero, returns early without a second `-dealloc`, and leaves the host
 * slab's used count clamped at zero -- so no slot count and no dealloc
 * counter can tell the two apart. What it is is a read of freed memory
 * inside `oz_static_release`, which `just test-behavior
 * --sanitize=address` reports and this file's assertions cannot. The
 * host-independent gate for it is
 * `arc_leak_regressions.rs::init_bound_through_a_cast_is_released_exactly_once`,
 * which counts the releases in the emitted C instead of running them.
 */
/* oz-pool: Thing=2 */
#import "OZTestBase.h"

@interface Thing : OZObject
- (void)poke;
@end

@implementation Thing
- (void)poke {}
@end

@interface Holder : OZObject
{
	Thing *_kid;
}
- (int)refill;
@end

@implementation Holder
/* A store to a strong ivar. Read as borrowed, this earned a retain it had
 * no business having, leaving the allocation at +2 with one release ever
 * to come. */
- (int)refill
{
	_kid = (Thing *)[Thing alloc];
	return _kid != nil;
}
@end

/* The reported shape: a local's initialiser behind a cast. Its own
 * function, because the leak is only visible once the scope has ended. */
static void bind_through_cast(void)
{
	Thing *t = (Thing *)[Thing alloc];

	[t poke];
}

int run_bound_through_cast(void)
{
	bind_through_cast();

	/* The scope has ended, so the slab must be empty again. */
	Thing *first = [Thing alloc];
	Thing *second = [Thing alloc];
	int both_free = (first != nil) && (second != nil);

	[first poke];
	[second poke];
	return both_free;
}

/* The double-free direction. `-init...` hands back its receiver's own +1,
 * so this is one object under two names -- see the header for why the
 * assertion below cannot see an over-release and `--sanitize=address`
 * can. */
static void init_bound_through_cast(void)
{
	Thing *u = [Thing alloc];
	Thing *t = (Thing *)[u init];

	[t poke];
}

int run_init_bound_through_cast(void)
{
	init_bound_through_cast();

	/* One object, one release owed: both slots free either way. */
	Thing *first = [Thing alloc];
	Thing *second = [Thing alloc];
	int both_free = (first != nil) && (second != nil);

	[first poke];
	[second poke];
	return both_free;
}

/* A reassignment to a strong local. The cast was neither owning nor a
 * plain identifier, which took the local out of `managed_object_locals`
 * altogether -- so nothing released the overwritten object, and the
 * ordinary reassign-in-a-loop shape was rejected outright, because an
 * unmanaged local cannot bound how many instances are live. Three
 * iterations through one slot is the assertion, which can only hold if
 * each release comes before the next allocation. */
int run_reassign_through_cast(void)
{
	Thing *t = nil;
	int i = 0;
	int made = 0;

	while (i < 3) {
		t = (Thing *)[Thing alloc];
		made = made + (t != nil);
		i = i + 1;
	}
	[t poke];
	return made == 3;
}

/* The strong-ivar store. Two slots and three overwrites: the third can
 * only find a slot if each store released what the ivar held. */
int run_ivar_store_through_cast(void)
{
	Holder *h = [Holder alloc];
	int all = [h refill] + [h refill] + [h refill];

	return all == 3;
}

/* A `return` behind a cast, both halves of it. `makeSent`'s cast used to
 * leave the function classified +0, so the caller kept a reference
 * nothing released; `makeHeld`'s used to hide the returned local's name,
 * so the local was released on the way out and the caller was handed a
 * freed pointer. */
static Thing *makeSent(void)
{
	return (Thing *)[Thing alloc];
}

static Thing *makeHeld(void)
{
	Thing *s = [Thing alloc];

	return (Thing *)s;
}

static int takeBoth(void)
{
	Thing *sent = makeSent();
	Thing *held = makeHeld();

	[sent poke];
	[held poke];
	return (sent != nil) + (held != nil);
}

int run_return_through_cast(void)
{
	int first = takeBoth();
	/* Two slots, and the first pair must have given them back. */
	int second = takeBoth();

	return (first == 2) && (second == 2);
}
