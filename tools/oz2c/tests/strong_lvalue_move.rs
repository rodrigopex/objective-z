// SPDX-License-Identifier: Apache-2.0
//
// strong_lvalue_move.rs -- moving a `__strong` lvalue out of a slot (#527).
//
// `Reading *taken = _slots[i]; _slots[i] = nil; return taken;` is the
// idiomatic queue pop -- a cache eviction, an ownership handoff out of an
// array ivar -- and it handed the caller a **freed block**. The load
// retained nothing, so the nil store's release was the last one on the
// reference. With `CONFIG_OBJZ_DEBUG_REFCOUNT=y` the caller read
// `0xA5A5A5A5`; with the instruments off it read a plausible `11`, which is
// why this survived a passing on-target run.
//
// `docs/ARC.md` 2.5.5 recorded the construct as `UNEXAMINED` -- "no
// construct in the accepted subset moves a slot". The record was accurate
// about the analysis and silent about the third outcome, which was the one
// that happened: accepted, undiagnosed, and miscompiled.
//
// **Asserted on lifetimes, with a dealloc counter, not only on emitted
// text** -- the same reason `strong_slots.rs` does it for #359: the
// complaint is that the object died at the wrong time, and a retain in the
// right place is only evidence that it might not.
//
// The fix follows Clang: the load retains. Clang's AST marks such a local
// `cinit destroyed` and the return `ARCProduceObject`, and `arc.rs`
// normally elides that retain -- sound exactly while the source slot cannot
// be invalidated during the local's lifetime, which is the premise this
// shape breaks. Nothing downstream would remove the pair if it were emitted
// everywhere instead: there is no `ObjCARCOpt` here, and GCC elided zero
// refcount traffic even under `-flto` (docs/STATUS.md, "Why this is not
// Clang's ARC"). So it is paid at the move and nowhere else.
//
// Two halves that must agree, and do because they read one predicate:
// `moved_slot_locals` makes the declaration retain, and
// `managed_object_locals` puts the same local in the managed set so the
// scope exit releases it -- or a `return` hands the `+1` on instead. The
// retain alone leaks; the release alone is a double free.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

const RING: &str = "\
static int g_deallocs = 0;

@interface Reading : OZObject {
	int _v;
}
- (id)initWithV:(int)v;
- (int)v;
@end
@implementation Reading
- (id)initWithV:(int)v
{
	self = [super init];
	if (self != nil) {
		_v = v;
	}
	return self;
}
- (int)v
{
	return _v;
}
- (void)dealloc
{
	g_deallocs++;
}
@end

@interface Ring : OZObject {
	Reading *_slots[4];
}
- (void)putAt:(int)i value:(int)v;
- (Reading *)takeIndex:(int)i;
- (int)dropIndex:(int)i;
- (Reading *)peekIndex:(int)i;
- (int)liveSlots;
@end
@implementation Ring
- (void)putAt:(int)i value:(int)v
{
	_slots[i] = [[Reading alloc] initWithV:v];
}
/* The move, returned: the caller is handed the reference. */
- (Reading *)takeIndex:(int)i
{
	Reading *taken = _slots[i];

	_slots[i] = nil;
	return taken;
}
/* The move, *not* returned: the local is the last owner, so the scope
   exit has to free it -- exactly once. */
- (int)dropIndex:(int)i
{
	Reading *taken = _slots[i];

	_slots[i] = nil;
	return [taken v];
}
/* A plain borrow, for contrast: the slot is never written, so nothing
   here may retain or release. */
- (Reading *)peekIndex:(int)i
{
	Reading *borrowed = _slots[i];

	return borrowed;
}
- (int)liveSlots
{
	int n = 0;

	for (int i = 0; i < 4; i++) {
		if (_slots[i] != nil) {
			n++;
		}
	}
	return n;
}
@end
";

fn program(body: &str) -> String {
    format!("/* oz-pool: Reading=8,Ring=2 */\n{}{}\n{}", PREAMBLE(), RING, body)
}

/// The shape #527 filed. The caller reads a live object, and the ring no
/// longer holds it.
///
/// Before the fix `[got v]` read freed memory: `value=11` with the
/// instruments off, `0xA5A5A5A5` with the freed-slot poison on.
#[test]
fn a_moved_slot_hands_the_caller_a_live_object() {
    let src = program(
        "\
#include <stdio.h>

int main(void)
{
	Ring *r = [[Ring alloc] init];

	[r putAt:0 value:11];
	[r putAt:1 value:22];
	printf(\"before live=%d deallocs=%d\\n\", [r liveSlots], g_deallocs);

	Reading *got = [r takeIndex:0];

	printf(\"taken v=%d live=%d deallocs=%d\\n\", [got v], [r liveSlots], g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "move_hands_live_object");
    assert_eq!(
        stdout,
        "before live=2 deallocs=0\ntaken v=11 live=1 deallocs=0\n",
        "the moved object must still be alive in the caller, and the slot must have \
         given it up"
    );
}

/// The caller's `+1` is released when its own scope ends -- once, not
/// twice and not never.
///
/// This is the half that says the method really is `+1`-returning:
/// ownership here is computed from the body, not from the selector's
/// family, so `-takeIndex:` needs no create-rule name for the caller to
/// take on the release.
#[test]
fn the_callers_reference_is_released_exactly_once() {
    let src = program(
        "\
#include <stdio.h>

static void take_and_drop(Ring *r)
{
	Reading *got = [r takeIndex:0];

	printf(\"inside v=%d deallocs=%d\\n\", [got v], g_deallocs);
}

int main(void)
{
	Ring *r = [[Ring alloc] init];

	[r putAt:0 value:11];
	take_and_drop(r);
	printf(\"after deallocs=%d live=%d\\n\", g_deallocs, [r liveSlots]);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "move_caller_releases_once");
    assert_eq!(
        stdout,
        "inside v=11 deallocs=0\nafter deallocs=1 live=0\n",
        "the caller owns the moved reference and frees it at scope exit -- exactly one \
         dealloc, and a second would mean the slot released it too"
    );
}

/// The move whose local never escapes. The local is the last owner, so
/// the scope exit frees it -- and the nil store must not have freed it
/// already.
///
/// This is the shape that a retain-only fix turns into a double free, and
/// a release-only fix into the original use-after-free.
#[test]
fn a_moved_slot_that_never_escapes_is_freed_once() {
    let src = program(
        "\
#include <stdio.h>

int main(void)
{
	Ring *r = [[Ring alloc] init];

	[r putAt:2 value:33];
	printf(\"v=%d\\n\", [r dropIndex:2]);
	printf(\"after deallocs=%d live=%d\\n\", g_deallocs, [r liveSlots]);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "move_no_escape_freed_once");
    assert_eq!(
        stdout,
        "v=33\nafter deallocs=1 live=0\n",
        "the local is the last owner: one dealloc, and reading `v` before it must not \
         have been a use-after-free"
    );
}

/// The negative control, and the one that keeps this fix narrow: a plain
/// borrow from a slot the body never writes must emit no refcount traffic
/// at all.
///
/// Six live sites in `src/` have exactly this shape -- `OZArray`'s element
/// access and enumeration, `OZDictionary`'s key and value access -- and
/// they are the SDK's hottest paths. Retaining every bind from a strong
/// lvalue is Clang's model verbatim and would charge all six; keyed on the
/// store, none of them pays.
#[test]
fn a_borrow_from_an_unwritten_slot_emits_no_traffic() {
    let src = program(
        "\
#include <stdio.h>

int main(void)
{
	Ring *r = [[Ring alloc] init];

	[r putAt:3 value:44];

	Reading *seen = [r peekIndex:3];

	printf(\"v=%d live=%d deallocs=%d\\n\", [seen v], [r liveSlots], g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "borrow_no_traffic");
    assert_eq!(
        stdout,
        "v=44 live=1 deallocs=0\n",
        "a borrow leaves the slot owning the object: still live, never freed"
    );

    /* The *definition*, not a prototype and not a comment that happens to
     * name the symbol -- the first attempt at this used `split` on the
     * function name and caught a comment about `retainCount`, which is the
     * "a test helper can manufacture a finding" trap. Anchored on the
     * opening brace of the definition, and brace-matched to its end. */
    let out = oz2c::transpile(&src).expect("should transpile");
    let needle = "struct Reading * Ring_peekIndex_(struct Ring *self, int i)\n{";
    let start = out
        .source_c
        .find(needle)
        .unwrap_or_else(|| panic!("the borrowing method's definition is emitted:\n{}", out.source_c));
    let rest = &out.source_c[start + needle.len()..];
    let mut depth = 1usize;
    let mut end = rest.len();
    for (i, ch) in rest.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &rest[..end];
    assert!(
        !body.contains("oz_retain") && !body.contains("oz_release"),
        "a borrow from a slot this body never writes must cost nothing:\n{}",
        body
    );
}
