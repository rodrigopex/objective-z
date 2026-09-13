// SPDX-License-Identifier: Apache-2.0
//
// id_typed_slots.rs -- a `static` or file-scope slot declared `id` is a
// strong slot, so a `+1` stored in one is released when it is replaced
// (#429).
//
// #400 established that `id` is the one object spelling carrying no `*` in
// source -- it is already a pointer -- so a `stars == 1` test cannot see it.
// `emit::managed_object_locals` was fixed to ask
// `(stars == 1 && is_class(bare)) || (stars == 0 && bare == "id")`, and
// carries a ten-line comment saying exactly that.
//
// The slot paths never got it. `static_object_locals` sits about forty lines
// above that comment in the same file, and `is_file_scope_object` asks
// `class_name_from_type(ty).is_some()`, which answers None for `id` -- the
// same blind spot in a second and third place. A `+1` stored in either slot
// was never managed and never released.
//
// That is the "when one site is found wrong, enumerate the siblings" rule
// with the siblings left un-enumerated: #400 fixed the local and stopped.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// The class every case below stores, counting its own deallocations.
fn thing() -> &'static str {
    "\
static int g_gone = 0;

@interface Thing : OZObject
@end
@implementation Thing
- (void)dealloc {
	g_gone = g_gone + 1;
}
@end
"
}

/// A `static id` slot releases what it replaces.
///
/// Two calls, two allocations, one replacement -- so exactly one dealloc
/// must have run by the end. Before this fix the store was not recognised
/// as a strong slot at all, so the first object was simply abandoned and
/// `gone=0`.
#[test]
fn a_static_id_slot_releases_what_it_replaces() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        thing(),
        "\
static void tick(void)
{
	static id cached;

	cached = [Thing alloc];
}

#include <stdio.h>
int main(void)
{
	tick();
	tick();
	printf(\"gone=%d\\n\", g_gone);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "static_id_slot_releases");
    assert_eq!(
        out, "gone=1\n",
        "a `static id` slot is a strong slot: the second store must release the first \
         object. got:\n{}",
        out
    );
}

/// The same, declared with an initialiser rather than bare.
///
/// `static id primed = nil;` and `static id cached;` mean the same thing,
/// and `managed_object_locals` decides them by one rule for that reason --
/// so the slot paths must not split them either.
#[test]
fn a_static_id_slot_initialised_to_nil_behaves_the_same() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        thing(),
        "\
static void tick(void)
{
	static id cached = nil;

	cached = [Thing alloc];
}

#include <stdio.h>
int main(void)
{
	tick();
	tick();
	printf(\"gone=%d\\n\", g_gone);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "static_id_slot_nil_init");
    assert_eq!(out, "gone=1\n", "unexpected:\n{}", out);
}

/// A **file-scope** `id` is a strong slot too -- the third path.
///
/// `is_file_scope_object` is a separate question from
/// `static_object_locals`, asked by `render_strong_local_assign` for a name
/// that is neither a local nor an ivar, and it had the same blind spot.
#[test]
fn a_file_scope_id_slot_releases_what_it_replaces() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        thing(),
        "\
static id g_cached;

static void tick(void)
{
	g_cached = [Thing alloc];
}

#include <stdio.h>
int main(void)
{
	tick();
	tick();
	printf(\"gone=%d\\n\", g_gone);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "file_scope_id_slot_releases");
    assert_eq!(out, "gone=1\n", "unexpected:\n{}", out);
}

/// `__unsafe_unretained` still opts a slot out, for `id` as for a class
/// spelling.
///
/// The qualifier is the one documented way to say "this slot does not
/// participate", and widening what counts as a strong slot must not take it
/// away. Nothing is released here, so the count stays at zero.
#[test]
fn an_unsafe_unretained_static_id_slot_is_left_alone() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        thing(),
        "\
static void tick(void)
{
	static __unsafe_unretained id cached;

	cached = [Thing alloc];
	(void)cached;
}

#include <stdio.h>
int main(void)
{
	tick();
	tick();
	printf(\"gone=%d\\n\", g_gone);
	return 0;
}
"
    );
    let out = compile_and_run(&src, "unsafe_unretained_static_id");
    assert_eq!(
        out, "gone=0\n",
        "__unsafe_unretained opts the slot out, so nothing is released. got:\n{}",
        out
    );
}
