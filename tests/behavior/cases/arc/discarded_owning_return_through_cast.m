/* A +1 result discarded *through a cast* is released too (#327).
 *
 * #322 made a discarded `+1` release itself, but only where the send was
 * the whole of the statement: `arc::is_owning_expr` reads a cast as
 * borrowed -- deliberately, and still does -- so `(void)[t copy];` never
 * reached the new path and leaked while `[t copy];` did not. The two
 * spellings mean the same thing, and the cast is the one people write on
 * purpose: `(void)expr` is the idiom for "I am throwing this away", so it
 * is the spelling least likely to be a mistake. `arc::discarded_value`
 * looks through it at the discarded statement and nowhere else.
 *
 * A leak passes any assertion about return values -- the blindness #283
 * recorded -- so each entry point instead reports whether a *later*
 * allocation still finds a slab slot. With the abandoned reference
 * released the two-slot slab always has one free; with it leaked the slab
 * is full and `[Thing alloc]` answers nil. That makes the leak a Unity
 * failure on any host, which matters because macOS arm64 has no
 * `-fsanitize=leak` and cannot run the corpus's `--check-leaks` gate at
 * all. `Thing=2` is exact: peak live is `t` plus one more, and one slot
 * short of the leak. Each entry point gives its slots back, so each one
 * starts from an empty slab.
 *
 * Every cast here is written under a `(void)`, including the one to a real
 * type. That is not decoration: `(Thing *)[t copy];` on its own is
 * `-Wunused-value`, which this corpus compiles as an error, so a plain
 * pointer cast would fail the case before the leak could be measured. The
 * bare spelling is covered in
 * `tools/oz_static/tests/arc_leak_regressions.rs` instead, where the
 * generated C is compiled without `-Werror`.
 *
 * `run_init_through_cast_on_an_owned_local` is the opposite direction, and
 * the one that matters more: `-init...` consumes its receiver's +1 and
 * hands it back, so the reference a discarded `(void)[t init];` throws
 * away is `t`'s -- and `t` is released at the end of its scope. Releasing
 * it here as well is a double free. That entry point reads the slab the
 * other way round -- one slot free and no second -- so an over-release
 * shows up as a *spare* slot rather than as a missing one, and under
 * `--sanitize=address` as the double free it is.
 *
 * The `-retain` half of the same guard is not here: `-retain` and
 * `-release` are declared on no SDK interface, so the corpus's Clang AST
 * pass rejects a send of either. It lives in `arc_leak_regressions.rs`
 * instead (`discarded_retain_and_init_through_a_cast_are_left_alone`),
 * which is where #322 put its own retain guard for the same reason.
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

/* The deliberate-discard idiom: an owning send behind a `(void)`. */
int run_discarded_through_void_cast(void)
{
	Thing *t = [Thing alloc];

	(void)[t copy];

	Thing *slot = [Thing alloc];
	int slot_free = (slot != nil);

	[t poke];
	return slot_free;
}

/* A cast to a real type, under the `(void)` that keeps the statement
 * warning-free. Two casts deep, so the peel has to recurse rather than
 * strip one layer. */
int run_discarded_through_pointer_cast(void)
{
	Thing *t = [Thing alloc];

	(void)(Thing *)[t copy];

	Thing *slot = [Thing alloc];
	int slot_free = (slot != nil);

	[t poke];
	return slot_free;
}

/* A cast on the *receiver* of a discarded `-init`, whose +1 belongs to a
 * temporary nothing tracks. The cast used to stop the receiver from being
 * resolved back to an `+alloc`. */
int run_discarded_init_behind_cast_receiver(void)
{
	(void)[(Thing *)[Thing alloc] init];

	Thing *first = [Thing alloc];
	Thing *second = [Thing alloc];
	int both_free = (first != nil) && (second != nil);

	return both_free;
}

/* The double-free direction. `-init...` consumes its receiver's +1 and
 * hands it back, so the reference `(void)[t init];` throws away is `t`'s
 * -- and `t` is released at the end of this scope. A cast must not hide
 * that: looking through it has to reach the receiver, not stop at the
 * selector's name.
 *
 * `t` therefore still holds its slot: one more allocation succeeds and the
 * next must not. Release the discarded result as well and `t` is freed
 * early, which shows up here as a second free slot and, under
 * `--sanitize=address`, as the double free it is.
 */
int run_init_through_cast_on_an_owned_local(void)
{
	Thing *t = [Thing alloc];

	(void)[t init];

	Thing *only = [Thing alloc];
	Thing *spare = [Thing alloc];
	int exactly_one_free = (only != nil) && (spare == nil);

	[t poke];
	return exactly_one_free;
}
