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

    for (label, body) in [("implicit", &implicit), ("explicit", &explicit)] {
        assert!(
            body.contains("oz_static_retain"),
            "the {} store takes no retain, so the ivar holds a reference nothing accounts for:\n{}",
            label,
            body
        );
        assert!(
            body.contains("_oz_prev_"),
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

/// `other->_x` is direct ivar access on another object, which needs that
/// object's class to resolve the ivar and its access path. Nothing in the
/// tree writes it and it still falls through -- recorded as a test so the
/// limitation is deliberate rather than assumed.
#[test]
fn another_objects_ivar_stored_directly_is_left_alone() {
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

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "Pair_fill_");
    assert!(
        !body.contains("_oz_prev_"),
        "an ivar store through another object was lowered; that needs its class resolved first:\n{}",
        body
    );
}
