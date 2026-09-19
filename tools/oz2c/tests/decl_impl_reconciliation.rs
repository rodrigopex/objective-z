// SPDX-License-Identifier: Apache-2.0
//
// decl_impl_reconciliation.rs - the `@interface` and the `@implementation`
// have to agree (#566, #567, #568).
//
// Three defects with one shape: oz2c collected declarations and definitions
// into the same table and never asked whether the two halves described the
// same class. Each surfaced at a different, wrong end of the pipeline.
//
//   #567  an `@implementation` with no `@interface` fabricated the class,
//         so the shared dispatch called a `Foo_oz_free` only an
//         `@interface` makes `companion.rs` define -- a link error from
//         `oz_release`, which nothing can dead-strip.
//   #566  a declared selector with no body, or a body whose selector
//         differs from the declaration's, emitted a call to a mangled
//         symbol that appears nowhere in the source. Whether the program
//         built depended on the *caller*: `-Wl,--gc-sections` drops an
//         unreferenced function, so one typo linked in one program and
//         failed in another.
//   #568  a declaration and a body disagreeing about the return type let
//         the declaration's spelling win, so `arc` claimed a `+1`
//         reference on an `int` and reported it as an internal
//         ownership-analysis *bug* -- asking the author to file a
//         transpiler issue about their own typo.
//
// The accepting halves matter at least as much here, because each check
// borders a legal shape that is easy to catch by accident, and one of them
// is the SDK itself. They are asserted below rather than left to the
// corpus: `method_family_ownership.rs` and `type_constraints.rs` both held
// one of these shapes *incidentally*, which is not the same as a test of
// it.

mod common;
use common::{
    compile_and_run, expect_reject, iterator_protocol_src, ozarray_src,
    ozobject_src as PREAMBLE,
};

/* ------------------------------------------------------------------ #566 */

/// A declared selector with no body anywhere, and a send to it (M76).
#[test]
fn declared_and_never_defined_send_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)missing;
- (int)run;
@end

@implementation Probe
- (int)run
{
	return [self missing];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'-missing' is declared on 'Probe' and defined nowhere"), "{}", diags);
    /* The mangled name is in the message on purpose: it is what the linker
     * would have said, and it appears nowhere in the source. */
    assert!(diags.contains("Probe_missing()"), "{}", diags);
}

/// Declaration and body name different selectors -- `second:` against
/// `third:` (M79). The body is a legal private method; what is missing is
/// the *declared* one, and the send resolves to that.
#[test]
fn selector_piece_mismatch_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)first:(int)a second:(int)b;
- (int)run;
@end

@implementation Probe
- (int)first:(int)a third:(int)b
{
	return a + b;
}
- (int)run
{
	return [self first:1 second:2];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("'-first:second:' is declared on 'Probe' and defined nowhere"),
        "{}",
        diags
    );
    /* The near-miss help is the whole value of the diagnostic for this
     * shape: the two selectors agree on the first piece and differ later,
     * which reads as a typo and is in fact two different methods. */
    assert!(diags.contains("'-first:third:' is the body meant here"), "{}", diags);
}

/// A class method, so the `_cls` suffix and the `+` spelling are both
/// exercised -- the instance path is the one the two cases above take.
#[test]
fn declared_and_never_defined_class_method_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
+ (int)tally;
- (int)run;
@end

@implementation Probe
- (int)run
{
	return [Probe tally];
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(diags.contains("'+tally' is declared on 'Probe' and defined nowhere"), "{}", diags);
    assert!(diags.contains("Probe_tally_cls()"), "{}", diags);
}

/// **Accepting.** A body with no declaration is an ordinary private method
/// (#566's M75 says so explicitly, and it builds correctly today). This
/// check asks the question the other way round, so it must not fire -- and
/// the method is called, so a wrong answer here is a rejection rather than
/// a link error.
#[test]
fn defined_and_never_declared_private_method_accepted() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)run;
@end

@implementation Probe
- (int)hidden
{
	return 41;
}
- (int)run
{
	return [self hidden] + 1;
}
@end

#include <stdio.h>
int main(void) {
	Probe *p = [Probe alloc];
	printf(\"%d\\n\", [p run]);
	return 0;
}
"
    );
    assert_eq!(compile_and_run(&src, "private_method_accepted"), "42\n");
}

/// **Accepting.** A class declared here and implemented somewhere else --
/// another translation unit, or hand-written C providing `Remote_ping()`.
/// The whole-program model cannot see either, so a send to a declared
/// selector stands when no primary `@implementation` is present to have
/// omitted it (`ClassInfo::has_primary_implementation`).
///
/// This is not a hypothetical: `method_family_ownership.rs` declares
/// `@interface Remote` with no implementation at all and reads the emitted
/// release text without ever linking, which is how the create-rule
/// families are tested. The first cut of #566's check refused all six of
/// its rows. Asserted here so the boundary has a test of its own rather
/// than a distant suite that happens to depend on it.
#[test]
fn declared_class_with_no_implementation_here_accepted() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Remote : OZObject
- (int)ping;
@end

int drive(Remote *r)
{
	return [r ping];
}
"
    );
    let out = oz2c::transpile(&src).expect("a class implemented elsewhere is not this file's bug");
    assert!(
        out.source_c.contains("Remote_ping("),
        "the call still stands, for the other unit to satisfy:\n{}",
        out.source_c
    );
}

/// **Accepting, and the one that would have refused the SDK.**
///
/// `include/oz_sdk/Foundation/OZArray.h` and `OZDictionary.h` both declare
/// `countByEnumeratingWithState:objects:count:`, which no built `.m`
/// implements -- `src/runtime_legacy/` has a body and no build file
/// references it. #566 asks for "every selector declared in an
/// `@interface` needs a definition", and that rule stated at the
/// declaration refuses Foundation outright.
///
/// It is safe on the *call* because `for (x in a)` lowers to
/// `OZ_PROTOCOL_SEND_nextObject` and never to that selector, so no call is
/// emitted to be undefined. That is a claim about the lowering, so it is
/// checked here rather than reasoned about: the assertion below is what
/// tells a later change to for-in that it has walked into this.
#[test]
fn for_in_over_an_array_does_not_reach_the_undefined_sdk_selector() {
    let src = format!(
        "{}{}{}{}",
        PREAMBLE(),
        iterator_protocol_src(),
        ozarray_src(),
        "\
@interface Probe : OZObject
- (int)run;
@end

@implementation Probe
- (int)run
{
	OZArray *a = @[];
	int n = 0;

	for (OZObject *each in a) {
		n = n + (each != nil);
	}
	return n;
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("for-in over an OZArray must stay inside the bar");
    /* Paired with a presence check. The absence above also holds if for-in
     * stops being emitted at all, which is the shape #542's four green
     * guards had: a test that had quietly stopped testing anything. This
     * says the loop is still there and still routed the other way. */
    assert!(
        out.source_c.contains("OZ_PROTOCOL_SEND_nextObject"),
        "the for-in lowered to neither selector, so the absence above proves nothing:\n{}",
        out.source_c
    );
    assert!(
        !out.source_c.contains("countByEnumeratingWithState"),
        "for-in reached the selector the SDK declares and never defines:\n{}",
        out.source_c
    );
}

/* ------------------------------------------------------------------ #568 */

/// The declaration says `int`, the body returns an object.
#[test]
fn return_type_mismatch_rejected() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)value;
@end

@implementation Probe
- (Probe *)value
{
	return self;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("'-value' is declared on 'Probe' returning 'int' and defined returning"),
        "{}",
        diags
    );
    /* Both types, so the author can see which half to change without
     * re-reading the source. */
    assert!(diags.contains("struct Probe *"), "{}", diags);
    /* The point of #568: this used to be an internal-invariant message
     * asking for a bug report about ordinary malformed input. Asserted as
     * an absence *and* a presence, because an absence alone also passes
     * when the check stops running (see
     * `docs/STATUS.md` on #542's four green guards). */
    assert!(!diags.contains("please report it"), "still an internal error:\n{}", diags);
    assert!(!diags.contains("ownership-analysis bug"), "still an internal error:\n{}", diags);
}

/// **Accepting.** `instancetype` and the class's own pointer type are the
/// same return type, spelled two ways. `extract_method_sig` resolves the
/// former to `struct Probe *` before it reaches the table, so the
/// comparison is on the resolved C spelling and these agree -- if it were
/// on the source text this would be a false rejection of the commonest
/// `-init` idiom there is.
#[test]
fn instancetype_against_the_class_pointer_accepted() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (instancetype)configure;
- (int)run;
@end

@implementation Probe
- (Probe *)configure
{
	return self;
}
- (int)run
{
	return [self configure] == nil ? 0 : 42;
}
@end
"
    );
    /* `-configure` returns `+0` (it hands back `self`), so this is only
     * about the two spellings agreeing. */
    let out = oz2c::transpile(&src).expect("'instancetype' and 'Probe *' are the same type");
    assert!(out.source_c.contains("Probe_configure("), "source_c:\n{}", out.source_c);
}

/* ------------------------------------------------------------------ #567 */

/// **Accepting.** A category's `@implementation` carries no `@interface` of
/// its own and must not be read as a class missing one -- it extends a
/// class declared elsewhere in the file. #501's guard is the one that
/// covers the category itself; this asserts the two checks do not overlap.
#[test]
fn category_implementation_beside_a_primary_interface_accepted() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Probe : OZObject
- (int)run;
@end

@interface Probe (Extra)
- (int)extra;
@end

@implementation Probe
- (int)run
{
	return [self extra];
}
@end

@implementation Probe (Extra)
- (int)extra
{
	return 42;
}
@end

#include <stdio.h>
int main(void) {
	Probe *p = [Probe alloc];
	printf(\"%d\\n\", [p run]);
	return 0;
}
"
    );
    assert_eq!(compile_and_run(&src, "category_impl_accepted"), "42\n");
}
