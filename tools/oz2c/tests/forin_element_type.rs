// SPDX-License-Identifier: Apache-2.0
//
// forin_element_type.rs -- a `for-in` header that names a class unrelated
// to what the collection holds is a hard, located error (#505).
//
// The defect was that `emit::render_forin_statement` seeds `ctx.scope`
// with the loop variable's **declared** class, validated against nothing
// but the set of class *names* -- so `for (Ghost *g in arrayOfOwner)`
// emitted `Ghost_ghostOnly((struct Ghost *)(g))`, a static call to a
// function belonging to a class the object is not, with no diagnostic.
// The program ran, and only because `-ghostOnly` never touched `self`.
//
// **This is one of the few places static dispatch was *less safe* than
// the runtime it replaces**, rather than merely narrower. Real
// Objective-C dispatches dynamically and raises an unrecognised selector
// at the send; oz2c called the wrong function silently. Clang cannot
// help -- `OZArray` holds `id`, so there is no element type in the source
// for it to check the header against, and it accepts the program too.
//
// **Why the check can be static at all, which is the whole reason this is
// a transpile-time error rather than a runtime one.** `OZArray` and
// `OZDictionary` are immutable: no `addObject:`, `insertObject:`,
// `removeObject:` or `setObject:` anywhere in `include/oz_sdk/`, and no
// mutable subclass of either. So the element type observed where a
// collection is *built* holds for its whole life, and reading it off an
// `@[...]` literal is sound rather than heuristic. If a mutable
// collection is ever added, the `Evidence::Literal` half of
// `generics::ElementClass` stops being sound and has to go.
//
// **What it deliberately does not refuse**, all three by decision:
//
//   - a header naming an **ancestor** of the element class -- widening,
//     and ordinary (`for (OZObject *o in arrayOfOwner)`);
//   - a header naming a **descendant** -- a downcast loop whose only
//     other spelling is `id` plus an explicit cast, so refusing it would
//     refuse something the language can express no other way. #505's
//     step 2 (the per-iteration runtime class check) is what catches a
//     *wrong* downcast, and that is the argument that made leaving this
//     accepted safe;
//   - anything it cannot resolve -- an `id` header, a collection that is
//     not a bare local, a heterogeneous literal, a collection that
//     arrived as a parameter or a method return. Silence on the
//     unresolvable is this module's standing rule, and it is why this
//     change refuses nothing the tree already had.
//
// So the refusal is narrow by construction: **two classes on different
// branches of the hierarchy**, which cannot be a cast of any kind.

mod common;
use common::{
    expect_reject, iterator_protocol_src, ozarray_src, ozdictionary_src, oznumber_src,
    ozobject_src, ozstring_src,
};

/// `Ghost` and `Owner` are siblings under `OZObject` -- unrelated, so
/// neither can stand in for the other under any cast. That is the shape
/// the check refuses, and the shape #505 reproduced.
const CLASSES: &str = "\
@interface Ghost : OZObject
- (int)ghostOnly;
@end
@implementation Ghost
- (int)ghostOnly
{
	return 3;
}
@end

@interface Owner : OZObject
- (int)ownerOnly;
@end
@implementation Owner
- (int)ownerOnly
{
	return 5;
}
@end

@interface Heir : Owner
- (int)heirOnly;
@end
@implementation Heir
- (int)heirOnly
{
	return 9;
}
@end
";

/// The body goes in a **plain C function**, not a method, and that is
/// deliberate: `generics::walk_for_method_bodies` reached only
/// `method_definition` before #505, so a check written only against a
/// method body would have passed while every sample -- which keeps its
/// code in `main()` -- went unchecked. `in_method` below is the same
/// program in a method, and both are asserted.
fn in_main(decl: &str, header: &str, body: &str) -> String {
    format!(
        "/* oz-pool: Ghost=2,Owner=2,Heir=2,OZArray=3,OZDictionary=2,OZString=4,OZNumber=4 */\n\
         {}{}{}{}{}{}{}\n\
         int main(void)\n{{\n\t{}\n\tint last = 0;\n\tfor ({}) {{\n\t\t{}\n\t}}\n\
         \t(void)last;\n\treturn 0;\n}}\n",
        ozobject_src(),
        iterator_protocol_src(),
        ozarray_src(),
        ozdictionary_src(),
        ozstring_src(),
        oznumber_src(),
        CLASSES,
        decl,
        header,
        body
    )
}

fn in_method(decl: &str, header: &str, body: &str) -> String {
    format!(
        "/* oz-pool: Ghost=2,Owner=2,Heir=2,OZArray=3,OZDictionary=2,OZString=4,OZNumber=4,Holder=1 */\n\
         {}{}{}{}{}{}{}\n\
         @interface Holder : OZObject\n- (int)go;\n@end\n\
         @implementation Holder\n- (int)go\n{{\n\t{}\n\tint last = 0;\n\tfor ({}) {{\n\t\t{}\n\t}}\n\
         \treturn last;\n}}\n@end\n\n\
         int main(void)\n{{\n\treturn [[Holder alloc] go];\n}}\n",
        ozobject_src(),
        iterator_protocol_src(),
        ozarray_src(),
        ozdictionary_src(),
        ozstring_src(),
        oznumber_src(),
        CLASSES,
        decl,
        header,
        body
    )
}

/// The needle. Measured for cardinality before being relied on: no other
/// diagnostic in `tools/oz2c/src/` contains "is unrelated to", so a
/// failure here can only be blaming this check. `generics.rs` already
/// emits "generic type mismatch", which is why that phrase is *not* the
/// needle -- it would match the pre-existing literal/annotation check
/// too and a regression in either would read as a regression in the
/// other.
const NEEDLE: &str = "is unrelated to";

fn assert_rejected(label: &str, decl: &str, header: &str, body: &str) {
    for (where_, src) in
        [("main()", in_main(decl, header, body)), ("a method", in_method(decl, header, body))]
    {
        let msg = expect_reject(&src);
        assert!(
            msg.contains(NEEDLE),
            "{} in {}: expected the for-in element-type refusal, got:\n{}",
            label,
            where_,
            msg
        );
        /* Presence, not just the needle: the message has to name both
         * classes, or it cannot tell the author which half is wrong. */
        assert!(
            msg.contains("Ghost") && msg.contains("Owner"),
            "{} in {}: the refusal must name both the header's class and the element's, got:\n{}",
            label,
            where_,
            msg
        );
    }
}

fn assert_accepted(label: &str, decl: &str, header: &str, body: &str) {
    for (where_, src) in
        [("main()", in_main(decl, header, body)), ("a method", in_method(decl, header, body))]
    {
        match oz2c::transpile(&src) {
            Ok(_) => {}
            Err(diags) => {
                let msg = diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
                assert!(
                    !msg.contains(NEEDLE),
                    "{} in {}: must NOT be refused by the element-type check, got:\n{}",
                    label,
                    where_,
                    msg
                );
            }
        }
    }
}

// ---------------------------------------------------------------------
// The defect
// ---------------------------------------------------------------------

/// #505's own reproduction: a plain `OZArray *` whose literal says
/// `Owner`, iterated as `Ghost`.
#[test]
fn a_lying_header_over_an_inferred_element_type_is_refused() {
    assert_rejected(
        "plain OZArray built from [Owner alloc]",
        "OZArray *arr = @[[Owner alloc]];",
        "Ghost *g in arr",
        "last = [g ghostOnly];",
    );
}

/// The author wrote the element type and the header contradicts it.
/// Needs no inference at all, and was accepted before #505 just the same.
#[test]
fn a_lying_header_over_a_declared_element_type_is_refused() {
    assert_rejected(
        "OZArray<Owner *>",
        "OZArray<Owner *> *arr = @[[Owner alloc]];",
        "Ghost *g in arr",
        "last = [g ghostOnly];",
    );
}

/// A dictionary's for-in binds its **keys** -- `src/OZDictionary.m`'s
/// `-nextObject` returns `_keys[_enumerationIndex]` -- so the first
/// generic argument is the one a header is checked against. Read from
/// that source rather than assumed: checking against the *value* type
/// would refuse every correct key loop in the tree.
#[test]
fn a_dictionary_header_is_checked_against_the_key_type() {
    assert_rejected(
        "OZDictionary<Owner *, OZNumber *> iterated as Ghost",
        "OZDictionary<Owner *, OZNumber *> *d = @{};",
        "Ghost *g in d",
        "last = [g ghostOnly];",
    );
}

// ---------------------------------------------------------------------
// Controls -- every one of these must stay accepted
// ---------------------------------------------------------------------

/// The ordinary case, and the control that says the refusal is not
/// unconditional. A guard whose message always fires is decoration.
#[test]
fn an_honest_header_is_accepted() {
    assert_accepted(
        "honest",
        "OZArray *arr = @[[Owner alloc]];",
        "Owner *o in arr",
        "last = [o ownerOnly];",
    );
}

/// Widening: the header names an ancestor of the element class. Legal
/// Objective-C, present in spirit throughout the SDK, and the reason the
/// check is kind-of rather than class equality.
#[test]
fn a_widening_header_is_accepted() {
    assert_accepted(
        "OZObject over an array of Owner",
        "OZArray *arr = @[[Owner alloc]];",
        "OZObject *o in arr",
        "last = (int)[o isEqual:o];",
    );
}

/// Narrowing: the header names a descendant. A downcast loop, accepted
/// **by decision** -- its only other spelling is `id` plus an explicit
/// cast at every send, and #505's step 2 runtime check is what catches a
/// wrong one. If this ever starts failing, that is a policy change and
/// not a bug fix.
#[test]
fn a_narrowing_header_is_accepted_by_decision() {
    assert_accepted(
        "Heir over an array of Owner",
        "OZArray *arr = @[[Owner alloc]];",
        "Heir *h in arr",
        "last = [h heirOnly];",
    );
}

/// An `id` header asks for no static class, so there is nothing to
/// contradict.
#[test]
fn an_id_header_is_accepted() {
    assert_accepted(
        "id plus an explicit cast",
        "OZArray *arr = @[[Owner alloc]];",
        "id o in arr",
        "last = [(Owner *)o ownerOnly];",
    );
}

/// A heterogeneous literal has no single element class, so the honest
/// answer is the common ancestor -- which this pass does not compute.
/// It records nothing rather than guessing, and the loop goes unchecked.
#[test]
fn a_heterogeneous_literal_establishes_nothing() {
    assert_accepted(
        "mixed Owner and Ghost",
        "OZArray *arr = @[[Owner alloc], [Ghost alloc]];",
        "Ghost *g in arr",
        "last = [g ghostOnly];",
    );
}

/// **The control that pins which generic argument is read.**
/// `OZDictionary<id, OZNumber *>` has an unknown key type, so a header
/// naming any class must be accepted.
///
/// This is the cell that fails if the element class is ever taken from
/// `Constrained::constraints[0]` instead of the first argument *node*:
/// that list has already dropped the `id`, so its element 0 is
/// `OZNumber` -- the **value** class -- and an `OZString` header would
/// be refused as unrelated. Correct code, refused, from an off-by-one
/// that only appears when an argument is `id`.
#[test]
fn an_id_keyed_dictionary_establishes_no_key_class() {
    assert_accepted(
        "OZDictionary<id, OZNumber *> iterated as OZString",
        "OZDictionary<id, OZNumber *> *d = @{};",
        "OZString *s in d",
        "last = (int)[s length];",
    );
}

/// Reassigning the name retires an element class that was read off a
/// literal: it described the object that literal built, not the name.
/// A stale entry here would refuse correct code, which is strictly worse
/// than missing a defect.
#[test]
fn reassignment_retires_an_inferred_element_type() {
    assert_accepted(
        "arr reassigned from an unresolvable expression",
        "OZArray *arr = @[[Owner alloc]];\n\tarr = [[Ghost alloc] unknownArray];",
        "Ghost *g in arr",
        "last = [g ghostOnly];",
    );
}

// ---------------------------------------------------------------------
// Shape coverage
// ---------------------------------------------------------------------

/// Both headers of a nested for-in are checked, and the inner one is
/// reached only because the `for_statement` arm falls through to the
/// recursion instead of returning. `nested_forin.m` is the corpus case
/// this protects.
#[test]
fn both_headers_of_a_nested_forin_are_checked() {
    let src = in_main(
        "OZArray *outer = @[[Owner alloc]];\n\tOZArray *inner = @[[Owner alloc]];",
        "Owner *o in outer",
        "for (Ghost *g in inner) {\n\t\t\tlast = [g ghostOnly];\n\t\t}",
    );
    let msg = expect_reject(&src);
    assert!(
        msg.contains(NEEDLE),
        "the inner header must be checked too -- if the for_statement arm returns instead of \
         recursing, a nested loop is never reached:\n{}",
        msg
    );
}

/// The honest nested shape, which is what the corpus actually has
/// (`tests/behavior/cases/forin/nested_forin.m`). Pairs with the cell
/// above so neither "always refuses" nor "never refuses" can pass both.
#[test]
fn an_honest_nested_forin_is_accepted() {
    let src = in_main(
        "OZArray *outer = @[[Owner alloc]];\n\tOZArray *inner = @[[Owner alloc]];",
        "Owner *o in outer",
        "for (Owner *i in inner) {\n\t\t\tlast = [i ownerOnly];\n\t\t}",
    );
    match oz2c::transpile(&src) {
        Ok(_) => {}
        Err(diags) => {
            let msg = diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
            assert!(!msg.contains(NEEDLE), "an honest nested for-in must be accepted:\n{}", msg);
        }
    }
}

/// The refusal carries a note and at least one remedy, per #457's
/// tiering. Asserted because a bare one-line message here would leave
/// the author knowing the header is wrong and not what to write instead.
#[test]
fn the_refusal_explains_itself_and_offers_a_remedy() {
    let src = in_main(
        "OZArray *arr = @[[Owner alloc]];",
        "Ghost *g in arr",
        "last = [g ghostOnly];",
    );
    let Err(diags) = oz2c::transpile(&src) else {
        panic!("expected the lying header to be refused");
    };
    let mine =
        diags.iter().find(|d| d.message.contains(NEEDLE)).expect("the element-type refusal");
    assert!(
        mine.note.as_ref().is_some_and(|n| n.contains("dispatched statically")),
        "the note must say what goes wrong, got: {:?}",
        mine.note
    );
    assert!(
        mine.help.iter().any(|h| h.contains("Owner")),
        "one remedy must name the class the collection actually holds, got: {:?}",
        mine.help
    );
    assert!(
        mine.help.iter().any(|h| h.contains("'id'")),
        "one remedy must offer the `id` escape, got: {:?}",
        mine.help
    );
    /* Located, not a bare (1,1) -- the whole point of `Diagnostic::at`. */
    assert!(mine.span.is_some(), "the refusal must be located");
}
