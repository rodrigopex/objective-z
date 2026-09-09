/* A `return` releases the reference it hands back when the returned name
 * is an alias of the owned local (#351).
 *
 * The release decision asked only *provenance* -- was this name
 * initialised by something recognisable as `+1`? -- and never *escape*:
 * is the reference reachable after the scope under some other name. So
 *
 *     Thing *a = [Thing alloc];
 *     Thing *b = a;
 *     return b;
 *
 * released `a`, the only reference there was, and handed the caller a
 * freed object. Two shapes, fixed by two different mechanisms, and both
 * are here because only one of them is answerable by looking at names:
 *
 *   - `makeAliased` is a *syntactic* alias. `arc::alias_chain` follows the
 *     plain-identifier initialiser to the local that owns the reference,
 *     and that local is kept instead of the alias -- no retain, no
 *     release, output byte identical to the correct spelling.
 *   - `makeViaCall` goes through a plain C call. Nothing can know whether
 *     `passthrough` hands back `a`, a different object, or nothing, so the
 *     returned value is retained and the caller owns it. That is what ARC
 *     does; Clang marks the same call `ARCReclaimReturnedObject`.
 *
 * Why this case exists at all: the defect reached `main` because no case
 * in this corpus aliased an owned local, so `leak-check` and `sanitizers`
 * -- which do catch it, on the host PAL where `oz_slab_alloc` is real
 * `malloc`/`free` -- were never pointed at the shape. It was a coverage
 * gap, not an instrument gap.
 *
 * The two directions need different observables, and neither is a dealloc
 * counter:
 *
 *   - **Freed too early** cannot be seen by counting slots, because a
 *     premature free leaves *more* of them. It is caught by allocating
 *     again: with the bug the returned object's block is back in the slab,
 *     the next `[Thing alloc]` hands out the same memory, and the two
 *     names become one object -- so a tag written through one is read back
 *     through the other. `run_*_is_live` is that test.
 *   - **Leaked** is the slot count, once the caller's scope has ended.
 *     `run_*_is_not_leaked` is that test.
 *
 * `Thing=2` is exact for both: the live test holds two instances at once,
 * and two is also one slot short of tolerating either leak.
 */
/* oz-pool: Thing=2 */
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

/* Opaque to the transpiler by construction: a plain C function whose body
 * says it returns its argument, which nothing in the ownership analysis
 * reads. */
Thing *passthrough(Thing *t)
{
	return t;
}

/* Mechanism one: the returned name aliases the owned local. */
static Thing *makeAliased(void)
{
	Thing *a = [Thing alloc];
	Thing *b = a;

	return b;
}

/* Mechanism two: the returned name came from a call, so its provenance
 * cannot be established at all. */
static Thing *makeViaCall(void)
{
	Thing *a = [Thing alloc];
	Thing *b = passthrough(a);

	return b;
}

/* The premature-free direction. With the reference dropped on the way out,
 * `other` is handed the very block `t` points at, and the second
 * `setTag:` overwrites the first. */
int run_alias_return_is_live(void)
{
	Thing *t = makeAliased();
	Thing *other = [Thing alloc];

	[t setTag:7];
	[other setTag:9];
	return [t tag] == 7;
}

int run_opaque_call_return_is_live(void)
{
	Thing *t = makeViaCall();
	Thing *other = [Thing alloc];

	[t setTag:7];
	[other setTag:9];
	return [t tag] == 7;
}

/* The leak direction. Own function, because the reference is only given
 * back once the scope holding it has ended. */
static void useAliased(void)
{
	Thing *t = makeAliased();

	[t setTag:1];
}

static void useViaCall(void)
{
	Thing *t = makeViaCall();

	[t setTag:1];
}

int run_alias_return_is_not_leaked(void)
{
	Thing *first = nil;
	Thing *second = nil;
	int both_free = 0;

	useAliased();

	first = [Thing alloc];
	second = [Thing alloc];
	both_free = (first != nil) && (second != nil);
	[first setTag:0];
	[second setTag:0];
	return both_free;
}

int run_opaque_call_return_is_not_leaked(void)
{
	Thing *first = nil;
	Thing *second = nil;
	int both_free = 0;

	useViaCall();

	first = [Thing alloc];
	second = [Thing alloc];
	both_free = (first != nil) && (second != nil);
	[first setTag:0];
	[second setTag:0];
	return both_free;
}
