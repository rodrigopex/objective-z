// SPDX-License-Identifier: Apache-2.0
//
// return_alias_escape.rs -- a `return` releases the reference it hands
// back, whichever name that reference reached the `return` under (#351).
//
// The release decision used to ask one question: was this *name*
// initialised by something recognisable as `+1`? That is provenance, and
// it is only half of what a release needs. The other half is **escape**:
// is the reference reachable after the scope through some other path? A
// `+1` reference that acquires a second name was invisible, so
//
//     Thing *a = [[Thing alloc] init];
//     Thing *b = a;
//     return b;
//
// released `a` -- the only reference there was -- and handed the caller a
// freed object. `arc.rs` documents the invariant that breaks, on
// `return_hands_back_ownership`: whichever local one side keeps is the one
// whose ownership the other says passes to the caller. For an alias they
// disagreed, and in the corrupting direction: emit released the owner while
// the analysis reported the function as `+0`, so nothing owned an object
// that was already gone.
//
// Two mechanisms, because the two shapes are knowable to different degrees,
// and the tests here are grouped that way:
//
//   1. A **syntactic alias** is resolvable and costs nothing.
//      `arc::alias_chain` follows plain-identifier initialisers to the
//      local that owns the reference, and that local is kept instead of the
//      alias. No retain, no release, byte-identical output.
//   2. An **opaque call** is not resolvable at all -- nothing can know
//      whether `passthrough(a)` hands back `a` -- so the returned value is
//      retained and the caller owns it. This is what ARC does; the Clang
//      AST marks the same call `ARCReclaimReturnedObject`.
//
// The cases that must *not* change are as important as the ones that must,
// because a retain here is not free: measured on ARM at -O2, wrapping this
// shape in a retain/release pair is 8 instructions to 12 with the ops out
// of line and 62 with them inlined, and GCC elides none of it even under
// whole-program LTO. So `a_returned_parameter_is_not_retained`,
// `a_returned_ivar_is_not_retained` and
// `a_return_with_nothing_owned_stays_byte_identical` are the guards on
// mechanism 2's blast radius -- which was measured at zero across all 118
// corpus and adapted cases.
//
// Balance is asserted on a **dealloc counter** rather than on slab
// exhaustion, following `arc_leak_regressions.rs`: `-fsanitize=leak` is
// unsupported on arm64-apple-darwin, and a counter distinguishes the two
// failure directions that matter here -- 0 deallocs is a leak, 2 is a
// double free, and an early 1 is the use-after-free this fixes.
//
// The caller is always a *nested function* that lets the reference go by
// falling off its own end, never an explicit `[t release]` -- which #428
// made a located error outright. It was never style even before that:
// these functions hand back `+1`, so the caller's scope exit already
// releases, and an explicit release on top is a second one. Written
// that way the tests still passed -- the `deallocating` flag in
// `oz_static_release` absorbs the extra -- which would have made this file
// blind to exactly the double free it is meant to guard against.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE};

/// A `Thing` that counts its own destruction, plus a plain C function that
/// hands back exactly what it was given -- the shape no analysis can see
/// through.
const THING: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject {
	int _n;
}
- (id)initWithN:(int)n;
- (int)n;
@end
@implementation Thing
- (id)initWithN:(int)n
{
	self = [super init];
	if (self != nil) {
		_n = n;
	}
	return self;
}
- (int)n
{
	return _n;
}
- (void)dealloc
{
	g_deallocs = g_deallocs + 1;
}
@end

Thing *passthrough(Thing *t)
{
	return t;
}
";

fn program(body: &str) -> String {
    format!("/* oz-pool: Thing=4 */\n{}{}\n{}", PREAMBLE(), THING, body)
}

/// One generated C function's *definition*, by name.
///
/// Skips lines that end in `;`, because the companion interface block
/// declares a prototype for every method and `find` reaches that first --
/// which returns a body consisting of the prototype plus whatever follows
/// it, and makes an assertion about the function's contents pass or fail
/// for reasons that have nothing to do with the function. This is the same
/// correction `explicit_ivar_store.rs` and `ownership_matrix.rs` already
/// carry; this copy was left behind.
///
/// It was not academic here. `a_returned_ivar_is_not_retained` asserts the
/// *absence* of `oz_static_retain` in `Holder_held`, and the span this
/// used to return started at `Holder_held`'s prototype and ran on through
/// the spliced `OZObject.h`. #418 put `int oz_static_retain_count(id obj);`
/// in that header, so the test began failing on a substring of a
/// declaration in a file it was never meant to read -- and had been
/// passing only because nothing in that span happened to match.
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

/* ---- mechanism 1: a syntactic alias, resolved for free ---------------- */

/// `Thing *b = a; return b;` -- the owner is kept, not the alias, and
/// nothing is retained. Without the fix `a` is released here and the
/// caller reads a freed object.
#[test]
fn an_alias_returned_keeps_the_owner_and_retains_nothing() {
    let src = program(
        "\
#include <stdio.h>

Thing *makeAliased(void)
{
	Thing *a = [[Thing alloc] initWithN:7];
	Thing *b = a;
	return b;
}

void useAliased(void)
{
	Thing *t = makeAliased();
	printf(\"n=%d deallocs=%d\\n\", [t n], g_deallocs);
}

int main(void)
{
	useAliased();
	printf(\"after=%d\\n\", g_deallocs);
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "makeAliased");
    assert!(
        !body.contains("oz_static_release"),
        "the owner is still released while its reference is handed back:\n{}",
        body
    );
    assert!(
        !body.contains("oz_static_retain"),
        "an alias needs no retain -- the owner is simply kept:\n{}",
        body
    );

    /* `n=7` proves the object is alive at the use, `deallocs=0` that it was
     * not destroyed on the way out, and `after=1` that it is destroyed
     * exactly once when the caller lets go. */
    let stdout = compile_and_run(&src, "return_alias_owner_kept");
    assert_eq!(stdout, "n=7 deallocs=0\nafter=1\n");
}

/// The same alias behind a cast. #332 routes a cast return through the same
/// peel, so this must resolve identically -- and it is the shape most
/// likely to regress, since the cast is what the peel was written for.
#[test]
fn an_alias_returned_behind_a_cast_keeps_the_owner() {
    let src = program(
        "\
#include <stdio.h>

Thing *makeCastAliased(void)
{
	Thing *a = [[Thing alloc] initWithN:5];
	Thing *b = a;
	return (Thing *)b;
}

void useCastAliased(void)
{
	Thing *t = makeCastAliased();
	printf(\"n=%d deallocs=%d\\n\", [t n], g_deallocs);
}

int main(void)
{
	useCastAliased();
	printf(\"after=%d\\n\", g_deallocs);
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "makeCastAliased");
    assert!(
        !body.contains("oz_static_release"),
        "the cast hid the alias again:\n{}",
        body
    );

    let stdout = compile_and_run(&src, "return_alias_cast");
    assert_eq!(stdout, "n=5 deallocs=0\nafter=1\n");
}

/// Both arms of a conditional must hand back the same contract. Before the
/// fix one path released the owner and returned a corpse while the other
/// returned a live `+1`, so no caller could be correct.
#[test]
fn both_paths_of_a_conditional_hand_back_the_same_contract() {
    let src = program(
        "\
#include <stdio.h>

Thing *pick(int c)
{
	Thing *a = [[Thing alloc] initWithN:3];
	Thing *b = a;
	if (c) {
		return b;
	}
	return a;
}

void usePick(int c)
{
	Thing *t = pick(c);
	printf(\"c=%d n=%d deallocs=%d\\n\", c, [t n], g_deallocs);
}

int main(void)
{
	usePick(1);
	usePick(0);
	printf(\"after=%d\\n\", g_deallocs);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "return_alias_conditional");
    /* Each call is balanced on its own path: the alias path and the
      * owner path both hand back exactly one reference, and each is
      * destroyed when its caller's scope ends. */
    assert_eq!(stdout, "c=1 n=3 deallocs=0\nc=0 n=3 deallocs=1\nafter=2\n");
}

/* ---- mechanism 2: an opaque call, retained ---------------------------- */

/// The shape #351 was filed on: the returned local came from a plain C
/// call, so nothing can prove whether it aliases the owned local. The
/// returned value is retained and the caller owns it.
#[test]
fn an_opaque_call_result_is_retained_at_the_return() {
    let src = program(
        "\
#include <stdio.h>

Thing *makeViaCall(void)
{
	Thing *a = [[Thing alloc] initWithN:9];
	Thing *b = passthrough(a);
	return b;
}

void useViaCall(void)
{
	Thing *t = makeViaCall();
	printf(\"n=%d deallocs=%d\\n\", [t n], g_deallocs);
}

int main(void)
{
	useViaCall();
	printf(\"after=%d\\n\", g_deallocs);
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "makeViaCall");
    assert!(
        body.contains("oz_static_retain"),
        "an unprovable return must be retained, not handed back at +0:\n{}",
        body
    );
    assert!(
        body.contains("oz_static_release"),
        "the owned local is still this function's to release:\n{}",
        body
    );

    /* `deallocs=0` at the use is the use-after-free this fixes -- before,
     * the object was destroyed inside `makeViaCall`. `after=1` is the
     * caller releasing the `+1` it was handed: `arc.rs` must report the
     * function as owning from the *same* predicate that retained, or this
     * reads 0 (a leak) instead. */
    let stdout = compile_and_run(&src, "return_alias_opaque_call");
    assert_eq!(stdout, "n=9 deallocs=0\nafter=1\n");
}

/// The invariant, stated as a test: when the return retains, the caller
/// releases. Two implementations of that rule would be one drift away from
/// a double free, so both sides call `arc::return_needs_retain`.
#[test]
fn the_caller_releases_exactly_what_the_return_retained() {
    let src = program(
        "\
#include <stdio.h>

Thing *makeViaCall(void)
{
	Thing *a = [[Thing alloc] initWithN:1];
	Thing *b = passthrough(a);
	return b;
}

int main(void)
{
	Thing *t = makeViaCall();
	printf(\"n=%d\\n\", [t n]);
	return 0;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let main_body = function_body(&out.source_c, "int main");
    assert!(
        main_body.contains("oz_static_release"),
        "the caller was not told to release the reference it was handed:\n{}",
        main_body
    );
}

/* ---- the blast radius: what must NOT change -------------------------- */

/// A returned *parameter* carries the caller's own reference, so releasing
/// our local cannot strand it and no retain is warranted. This is a cost
/// guard: retaining here would add a pair to every pass-through accessor.
#[test]
fn a_returned_parameter_is_not_retained() {
    let src = program(
        "\
Thing *passBack(Thing *p)
{
	Thing *scratch = [[Thing alloc] initWithN:2];
	[scratch n];
	return p;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "passBack");
    assert!(
        !body.contains("oz_static_retain"),
        "a parameter is the caller's reference and needs no retain:\n{}",
        body
    );
    assert!(
        body.contains("oz_static_release"),
        "the local is still owned and must be released:\n{}",
        body
    );
}

/// A returned ivar was already retained by the strong store that put it
/// there, so its reference outlives the scope on its own.
#[test]
fn a_returned_ivar_is_not_retained() {
    let src = program(
        "\
@interface Holder : OZObject {
	Thing *_held;
}
- (Thing *)held;
@end
@implementation Holder
- (Thing *)held
{
	Thing *scratch = [[Thing alloc] initWithN:4];
	[scratch n];
	return _held;
}
@end
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "Holder_held");
    assert!(
        !body.contains("oz_static_retain"),
        "a strong ivar's reference is already accounted for:\n{}",
        body
    );
}

/// With no owned local live, there is nothing for the return to release and
/// so nothing to protect the returned value from. The output stays exactly
/// as it was -- no retain, and no synthesized temporary either.
#[test]
fn a_return_with_nothing_owned_stays_byte_identical() {
    let src = program(
        "\
Thing *echo(Thing *p)
{
	Thing *b = passthrough(p);
	return b;
}
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let body = function_body(&out.source_c, "echo");
    assert!(
        !body.contains("oz_static_retain") && !body.contains("_oz_sync_ret_"),
        "a return owing nothing must not grow a retain or a temporary:\n{}",
        body
    );
    assert!(body.contains("return b;"), "expected the verbatim return:\n{}", body);
}
