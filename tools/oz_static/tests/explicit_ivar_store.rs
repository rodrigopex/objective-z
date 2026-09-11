// SPDX-License-Identifier: Apache-2.0
//
// explicit_ivar_store.rs -- `self->_ivar = value` lowers exactly like
// `_ivar = value` (#352).
//
// `render_strong_ivar_assign` is what makes a store into an owned object
// ivar balance: assign, retain the new value unless the right-hand side is
// already +1, release whatever the ivar held before. It opened with
//
//     if left.kind() != "identifier" { return None; }
//
// and `self->_x` is a `field_expression`, so the explicit spelling fell
// through to a plain C store with no retain and no release of the previous
// value. Two spellings of one operation, opposite ownership behaviour, and
// nothing about the author's choice between them says anything about
// ownership.
//
// One missing retain produced three defects in sequence: the local's own
// scope-exit release destroyed the object immediately, so the ivar
// dangled; and the synthesized dealloc later released that freed block a
// second time. `-fsanitize=address` reports `heap-use-after-free` inside
// `oz_static_release`.
//
// Same root cause as #351 -- an ownership decision keyed on a *syntactic
// form* rather than on the reference -- which is why the fix is the same
// shape: one extractor (`assigned_ivar_name`) that both spellings go
// through, rather than a second lowering for the second spelling.
//
// The array-element store had the identical gate and was fixed the same
// way in #360: `render_strong_array_element_assign` required the
// subscript's receiver to be an `identifier`, so `self->_arr[i] = v` fell
// through to a plain C store while `_arr[i] = v` retained. Three sites of
// one cause now (#351, #352, #360), each fixed by routing every spelling
// through one function -- which is the only thing that has stopped a
// fourth appearing.
//
// The cases that must *not* change are half this file. Dot syntax
// (`self.x = value`) is a property store and must keep going through the
// setter, which retains on its own; a scalar ivar has no ownership to
// manage; an `__unsafe_unretained` ivar must not be retained at all, since
// releasing what it points at would be the double-free direction; and
// `other->_x` is deliberately still left alone.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

const THING: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag
{
	return 7;
}
- (void)dealloc
{
	g_deallocs = g_deallocs + 1;
}
@end
";

fn program(body: &str) -> String {
    format!("/* oz-pool: Thing=4,Holder=2 */\n{}{}\n{}", PREAMBLE(), THING, body)
}

/// One generated C function's *definition*, by name.
///
/// Skips lines that end in `;`, because the companion interface block
/// declares a prototype for every method and `find` reaches that first --
/// which returns a body consisting of the prototype plus whatever follows
/// it, and makes an assertion about the function's contents pass or fail
/// for reasons that have nothing to do with the function.
fn function_body(source_c: &str, name: &str) -> String {
    let needle = format!("{}(", name);
    let mut from = 0;
    while let Some(rel) = source_c[from..].find(&needle) {
        let at = from + rel;
        let line_end = source_c[at..].find('\n').map(|e| at + e).unwrap_or(source_c.len());
        if !source_c[at..line_end].trim_end().ends_with(';') {
            let tail = &source_c[at..];
            let end = tail.find("\n}").map(|e| e + 2).unwrap_or(tail.len());
            return tail[..end].to_string();
        }
        from = at + needle.len();
    }
    panic!("no definition of `{}` in:\n{}", name, source_c);
}

/// The two spellings produce the same three operations. Asserted as a
/// comparison between them rather than against a fixed string, so the test
/// says "these agree" -- which is the actual requirement -- and does not
/// have to be rewritten if the lowering's shape ever changes.
///
/// The `assert_eq!` held across #405 exactly as intended. The two
/// `contains` checks above it did not, because one of them named the
/// generated temporary instead of the release it stood for -- a reminder
/// that a proxy for a property is not the property.
#[test]
fn both_spellings_of_an_owned_ivar_store_lower_identically() {
    let src = program(
        "\
@interface Holder : OZObject {
	Thing *_implicit;
	Thing *_explicit;
}
- (void)storeImplicit;
- (void)storeExplicit;
@end
@implementation Holder
- (void)storeImplicit
{
	Thing *t = [[Thing alloc] init];
	_implicit = t;
}
- (void)storeExplicit
{
	Thing *t = [[Thing alloc] init];
	self->_explicit = t;
}
@end
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let implicit = function_body(&out.source_c, "Holder_storeImplicit");
    let explicit = function_body(&out.source_c, "Holder_storeExplicit");

    for (label, body, ivar) in [
        ("implicit", &implicit, "_implicit"),
        ("explicit", &explicit, "_explicit"),
    ] {
        assert!(
            body.contains("oz_static_retain"),
            "the {} store takes no retain, so the ivar holds a reference nothing accounts for:\n{}",
            label,
            body
        );
        /* That the previous value is released, asked of the operation
         * rather than of the temporary's name. This used to look for
         * `_oz_prev_`, which was a proxy for the release and stopped being
         * one when #405 removed the temporary: a store whose right-hand
         * side is a plain identifier now names `self->_x` directly, which
         * is what keeps it correct inside a loop. The property is
         * unchanged; only its spelling was. */
        assert!(
            body.contains(&format!(
                "oz_static_release((struct OZObject *)(self->{}))",
                ivar
            )),
            "the {} store does not release what the ivar held before:\n{}",
            label,
            body
        );
    }

    /* The same text, once three things that are *meant* to differ are
     * normalised away: the ivar's name, the generated temporary's
     * position-derived suffix, and the provenance comment above each
     * statement -- which quotes the author's own line and so says
     * `self->_x` in one and `_x` in the other. Dropping comment-only
     * lines is what makes this a comparison of the emitted operations
     * rather than of the source that produced them. */
    let normalise = |body: &str, ivar: &str| {
        let code: Vec<&str> =
            body.lines().filter(|l| !l.trim_start().starts_with("/*")).collect();
        let mut out = code
            .join("\n")
            .replace(ivar, "_IVAR")
            .replace("storeImplicit", "STORE")
            .replace("storeExplicit", "STORE");
        while let Some(at) = out.find("_oz_prev_L") {
            let rest = &out[at + "_oz_prev_L".len()..];
            let end = rest
                .find(|c: char| !c.is_ascii_digit() && c != '_' && c != 'C')
                .unwrap_or(rest.len());
            out = format!("{}_oz_prev_N{}", &out[..at], &rest[end..]);
        }
        out
    };
    assert_eq!(
        normalise(&implicit, "_implicit"),
        normalise(&explicit, "_explicit"),
        "the two spellings still lower differently"
    );
}

/// And the object survives the method that stored it, then is destroyed
/// exactly once. Before the fix the counter read 1 immediately after
/// `storeExplicit` returned -- the ivar was already dangling -- and the
/// owner's dealloc released the freed block again.
#[test]
fn an_ivar_stored_through_self_outlives_the_method_and_dies_once() {
    let src = program(
        "\
@interface Holder : OZObject {
	Thing *_explicit;
}
- (void)store;
- (int)read;
@end
@implementation Holder
- (void)store
{
	Thing *t = [[Thing alloc] init];
	self->_explicit = t;
}
- (int)read
{
	return [_explicit tag];
}
@end

#include <stdio.h>
static void useHolder(void)
{
	Holder *h = [[Holder alloc] init];

	[h store];
	/* Still alive after the storing method returned: with the bug the
	 * object was destroyed at its exit and this reads freed memory. */
	printf(\"after store deallocs=%d tag=%d\\n\", g_deallocs, [h read]);
}

int main(void)
{
	useHolder();
	/* The holder is gone, so its dealloc released the ivar -- exactly
	 * once, which a double release could not report. */
	printf(\"after holder deallocs=%d\\n\", g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "explicit_ivar_store_lifetime");
    assert_eq!(stdout, "after store deallocs=0 tag=7\nafter holder deallocs=1\n");
}

/* ---- what must not change ------------------------------------------- */

/// Dot syntax is a property store and keeps going through the setter,
/// which does its own retaining. Hijacking it here would retain twice.
#[test]
fn dot_syntax_on_self_still_goes_through_the_setter() {
    let src = program(
        "\
@interface Holder : OZObject {
	Thing *_held;
}
@property (nonatomic, retain) Thing *held;
- (void)store;
@end
@implementation Holder
@synthesize held = _held;
- (void)store
{
	Thing *t = [[Thing alloc] init];
	self.held = t;
}
@end
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "Holder_store");
    assert!(
        body.contains("Holder_setHeld_"),
        "a property store must still be sent to the setter:\n{}",
        body
    );
    assert!(
        !body.contains("_oz_prev_"),
        "the setter releases the old value itself; doing it here as well is a double release:\n{}",
        body
    );
}

/// A scalar ivar has no ownership to manage, whichever way it is spelled.
#[test]
fn a_scalar_ivar_stored_through_self_is_untouched() {
    let src = program(
        "\
@interface Counter : OZObject {
	int _n;
}
- (void)bump;
@end
@implementation Counter
- (void)bump
{
	self->_n = 5;
}
@end
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "Counter_bump");
    assert!(
        !body.contains("oz_static_retain") && !body.contains("_oz_prev_"),
        "a scalar store grew refcount traffic:\n{}",
        body
    );
    assert!(body.contains("self->_n = 5;"), "expected the verbatim store:\n{}", body);
}

/// An `__unsafe_unretained` ivar is an unowned reference. Retaining it
/// here -- or releasing what it held -- is the double-free direction, so
/// the explicit spelling must be left alone exactly as the bare one is.
#[test]
fn an_unretained_ivar_stored_through_self_is_untouched() {
    let src = program(
        "\
@interface Backref : OZObject {
	__unsafe_unretained Thing *_owner;
}
- (void)pointAt:(Thing *)t;
@end
@implementation Backref
- (void)pointAt:(Thing *)t
{
	self->_owner = t;
}
@end
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "Backref_pointAt_");
    assert!(
        !body.contains("oz_static_retain") && !body.contains("_oz_prev_"),
        "an unretained ivar must not be retained or release what it held:\n{}",
        body
    );
}

/// `other->_x = ownedLocal` is now a **located error**, not a
/// pass-through.
///
/// This test asserted the opposite when #352 landed: that the store fell
/// through unlowered, "recorded as a test so the limitation is deliberate
/// rather than assumed". The ownership audit behind #359 showed what that
/// limitation actually produced -- the local is released when its scope
/// ends, so the ivar is left pointing at freed memory. A silent wrong free
/// is not a limitation worth pinning, and refusing it is what this
/// backend's own rule requires: never degrade silently.
///
/// Supporting it would mean `other`'s class taking ownership, which only a
/// store through `self` does today. The message says so, and names the
/// class, because the fix is to go through a method or setter on it.
#[test]
fn another_objects_ivar_stored_directly_is_refused() {
    let src = program(
        "\
@interface Pair : OZObject {
	Thing *_held;
}
- (void)fill:(Pair *)other;
@end
@implementation Pair
- (void)fill:(Pair *)other
{
	Thing *t = [[Thing alloc] init];
	other->_held = t;
}
@end
",
    );

    let diags = common::expect_reject(&src);
    assert!(
        diags.contains("another object's ivar"),
        "expected a located refusal naming the shape, got:\n{}",
        diags
    );
    assert!(
        diags.contains("Pair"),
        "the message must name the class that would have to own it:\n{}",
        diags
    );
}

/* ---- the array-element store, same gate, same fix (#360) ------------ */

/// `self->_arr[i] = value` lowers exactly like `_arr[i] = value`.
///
/// The emitted target is rebuilt from `ivar_access_path` either way, so
/// the two are byte-identical apart from the provenance comment quoting
/// the author's own line -- which is exactly what this compares.
#[test]
fn both_spellings_of_an_owned_array_element_store_lower_identically() {
    let src = program(
        "\
@interface Slots : OZObject {
	Thing *_arr[2];
}
- (void)bare;
- (void)viaSelf;
@end
@implementation Slots
- (void)bare
{
	Thing *t = [[Thing alloc] init];

	_arr[0] = t;
}
- (void)viaSelf
{
	Thing *t = [[Thing alloc] init];

	self->_arr[0] = t;
}
@end
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let bare = function_body(&out.source_c, "Slots_bare");
    let via_self = function_body(&out.source_c, "Slots_viaSelf");

    for (label, body) in [("bare", &bare), ("self->", &via_self)] {
        assert!(
            body.contains("oz_static_retain"),
            "the {} array store takes no retain, so the element dangles:\n{}",
            label,
            body
        );
    }

    let code = |body: &str, name: &str| {
        body.lines()
            .filter(|l| !l.trim_start().starts_with("/*"))
            .collect::<Vec<_>>()
            .join("\n")
            .replace(name, "STORE")
    };
    assert_eq!(
        code(&bare, "bare"),
        code(&via_self, "viaSelf"),
        "the two spellings still lower differently"
    );
}

/// And the element outlives the storing method, then dies exactly once
/// when the slot is overwritten -- the lifetime the text cannot prove.
#[test]
fn an_array_element_stored_through_self_outlives_the_method() {
    let src = program(
        "\
@interface Slots : OZObject {
	Thing *_arr[2];
}
- (void)store:(int)which;
- (int)tagAt:(int)which;
@end
@implementation Slots
- (void)store:(int)which
{
	Thing *t = [[Thing alloc] init];

	self->_arr[which] = t;
}
- (int)tagAt:(int)which
{
	return [_arr[which] tag];
}
@end

#include <stdio.h>
static void useSlots(void)
{
	Slots *s = [[Slots alloc] init];
	int i = 0;

	[s store:i];
	printf(\"after store deallocs=%d tag=%d\\n\", g_deallocs, [s tagAt:i]);
	/* Overwriting the slot must release exactly what it held. */
	[s store:i];
	printf(\"after overwrite deallocs=%d\\n\", g_deallocs);
}

int main(void)
{
	useSlots();
	printf(\"after owner died deallocs=%d\\n\", g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "array_element_through_self_lifetime");
    assert_eq!(
        stdout,
        "after store deallocs=0 tag=7\nafter overwrite deallocs=1\nafter owner died deallocs=2\n",
        "the element must survive the storing method, be released when overwritten, \
         and be released again with its owner"
    );
}
