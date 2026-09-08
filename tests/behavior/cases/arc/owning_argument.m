/* A +1 result passed straight as an *argument* is released (#328).
 *
 * The shape #322 recorded as unhandled and this issue tracked:
 *
 *     [self setFoo:[Foo new]];      -- the +1 from +new was never released
 *
 * It is neither *bound* nor *discarded*, so no existing path reached it.
 * Scope-based release needs a local to hang the release on,
 * `render_strong_local_assign` handles a store, `arc::discards_ownership`
 * (#322/#327) needs the value to be the whole of an `expression_statement`,
 * and `arc::binds_ownership` (#332) needs a binding site -- here it is
 * nested inside a message send. A synthesized strong setter *retains* its
 * argument on top of the leak, so the object ended at +2 with one release
 * ever owed.
 *
 * The fix holds the argument's reference in a temporary, sends the
 * message, then releases -- the caller's own reference given up once the
 * callee has had its chance to keep it. So a strong setter's retain still
 * stands and the object ends at +1, held by the ivar, and a callee that
 * only *borrows* the argument (`-take:` below) sees the object torn down
 * right after the send.
 *
 * A leak passes any assertion about return values -- the blindness #283
 * recorded -- so each entry point instead reports whether a *later*
 * allocation still finds a slab slot. That makes the leak a Unity failure
 * on any host, which matters because macOS arm64 has no
 * `-fsanitize=leak` and cannot run the corpus's `--check-leaks` gate at
 * all. `Thing=2` is exact: the widest peak is a store over a full ivar,
 * briefly two live instances because the setter assigns the new value
 * before releasing the old, and two is also one slot short of every leak
 * here. Every entry point gives its slots back before returning.
 *
 * `run_init_argument_left_alone` is the double-free direction, and it is
 * honest about what it can and cannot see. `-init...` consumes its
 * receiver's +1 and hands it back, so `[h setThing:[u init]];` passes a
 * reference `u` already accounts for and no call-site release is owed.
 * Taking one anyway leaves the object freed while the holder's ivar still
 * points at it, and the second `oz_static_release` -- the one
 * `-dealloc` issues over the ivars -- then reads freed memory. That read
 * is what `just test-behavior --sanitize=address` reports and what this
 * file's assertions cannot see: a refcount already at zero returns early
 * without a second `-dealloc`, and the host slab's used count is clamped
 * at zero, so neither a dealloc counter nor a slot count can tell the two
 * apart. The host-independent gate is
 * `arc_leak_regressions.rs::retained_and_init_arguments_are_left_alone`,
 * which counts the temporaries and releases in the emitted C instead of
 * running them.
 *
 * `-retain` is the other half of that guard and is deliberately *not*
 * here: `-retain`/`-release` are declared on no SDK interface, so the
 * corpus's Clang AST pass rejects a send of either. It lives in the same
 * Rust test, which is the split #322 and #327 both made.
 */
/* oz-pool: Thing=2 */
#import "OZTestBase.h"

@interface Thing : OZObject
+ (instancetype)new;
- (void)poke;
@end

@implementation Thing
+ (instancetype)new
{
	return [Thing alloc];
}
- (void)poke {}
@end

@interface Holder : OZObject
@property(strong) Thing *thing;
@end

@implementation Holder
@end

@interface Runner : OZObject
- (int)take:(Thing *)t;
- (int)countOf:(Thing *)t;
@end

@implementation Runner
/* Borrows the argument and keeps nothing, so the caller's +1 is the only
 * reference there ever was. */
- (int)take:(Thing *)t
{
	[t poke];
	return t != nil;
}
- (int)countOf:(Thing *)t
{
	return t != nil;
}
@end

/* The reported shape. Its own function, because the leak is only visible
 * once the holder's scope has ended and its ivar has been released. Three
 * stores through two slots: the third can only find one if the first
 * object was genuinely freed when the setter let it go. */
static int fill_holder(void)
{
	Holder *h = [Holder alloc];

	[h setThing:[Thing new]];
	[h setThing:[Thing new]];
	/* Only reachable if the first object's slot came back. */
	[h setThing:[Thing new]];

	return [h thing] != nil;
}

int run_owning_argument_to_setter(void)
{
	int held = fill_holder();

	/* The holder went with its scope, so the slab must be empty again. */
	Thing *first = [Thing alloc];
	Thing *second = [Thing alloc];
	int both_free = (first != nil) && (second != nil);

	[first poke];
	[second poke];
	return held && both_free;
}

/* The nested spelling, which is one reference under two sends:
 * `created_by` follows the `-init...` back to the `+alloc` that created
 * it, so exactly one temporary and one release are taken. */
static int nest_holder(void)
{
	Holder *h = [Holder alloc];

	[h setThing:[[Thing alloc] init]];
	[h setThing:[[Thing alloc] init]];
	[h setThing:[[Thing alloc] init]];

	return [h thing] != nil;
}

int run_nested_alloc_init_argument(void)
{
	int held = nest_holder();

	Thing *first = [Thing alloc];
	Thing *second = [Thing alloc];
	int both_free = (first != nil) && (second != nil);

	[first poke];
	[second poke];
	return held && both_free;
}

/* A callee that only borrows, which is the half a *consuming* setter could
 * never have covered -- `-take:` stores nothing and retains nothing. One
 * slot serves all three sends only if each object is torn down right
 * after its own. */
int run_owning_argument_borrowed_by_callee(void)
{
	Runner *r = [Runner alloc];
	int seen = 0;

	seen = seen + [r take:[Thing new]];
	seen = seen + [r take:[Thing new]];
	seen = seen + [r take:[Thing new]];

	return seen == 3;
}

/* The same argument in a *declaration's initialiser*, where the send's own
 * value is used. Emitted as a bare group of statements rather than a
 * braced one, because bracing would scope `a`, `b` and `c` out of the rest
 * of this function -- which is exactly what this entry point would catch. */
int run_owning_argument_in_declaration(void)
{
	Runner *r = [Runner alloc];
	int a = [r countOf:[Thing new]];
	int b = [r countOf:[Thing new]];
	int c = [r countOf:[Thing new]];

	return (a + b + c) == 3;
}

/* The guard that fails closed: a *borrowed* argument must not be released
 * by the call site. `t` still holds the reference and the setter retains
 * it, so a call-site release would take the object away from `t` before
 * its own scope exit does. Asserted where it is visible -- `t` is used
 * after the send, and the holder still has it. */
static int lend_to_holder(void)
{
	Holder *h = [Holder alloc];
	Thing *t = [Thing alloc];

	[h setThing:t];
	/* Still ours to use: the send borrowed it, it did not consume it. */
	[t poke];

	return ([h thing] == t) && (t != nil);
}

int run_borrowed_argument_stays(void)
{
	int lent = lend_to_holder();

	Thing *first = [Thing alloc];
	Thing *second = [Thing alloc];
	int both_free = (first != nil) && (second != nil);

	[first poke];
	[second poke];
	return lent && both_free;
}

/* The double-free direction -- see the header for why the assertion below
 * cannot see an over-release and `--sanitize=address` can. `-init...`
 * hands back its receiver's own +1, so this argument creates nothing and
 * no call-site release is owed. */
static void init_argument(void)
{
	Holder *h = [Holder alloc];
	Thing *u = [Thing alloc];

	[h setThing:[u init]];
}

int run_init_argument_left_alone(void)
{
	init_argument();

	/* One object, one release owed: both slots free either way. */
	Thing *first = [Thing alloc];
	Thing *second = [Thing alloc];
	int both_free = (first != nil) && (second != nil);

	[first poke];
	[second poke];
	return both_free;
}
