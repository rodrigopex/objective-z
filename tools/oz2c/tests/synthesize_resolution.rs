// SPDX-License-Identifier: Apache-2.0
//
// synthesize_resolution.rs -- where `@synthesize` looks for the `@property`
// it names, and what it says when it cannot find one (#498).
//
// Reported as "`@synthesize` seems unstable, sometimes it works and
// sometimes it doesn't". It is deterministic; two separate causes were
// behind it, and neither diagnostic mentioned `@property` or
// `@synthesize`, which is what made it read as flakiness.
//
// **A protocol-declared property was refused outright.** `collect.rs`
// looked the property up in `info.properties` for the class alone, so
// adopting a protocol property -- the idiomatic spelling, which Clang
// accepts with zero diagnostics -- was rejected with a message saying no
// such property was declared. That was wrong about the cause: it *was*
// declared, in a protocol nobody looked in. The gap went deeper than the
// lookup: `collect_protocol_methods` matched only `method_declaration`, so
// a protocol `@property` was collected **nowhere at all**.
//
// **A name shared with the SDK was refused as a dispatch error.** A
// property called `count` or `length` collides with `OZArray`'s and
// `OZString`'s selectors, which return `size_t`, and dispatch is keyed on
// the selector name. The rule is sound; the message never said a
// `@property` introduced the selector.
//
// One measurement the report did not have, and it matters to reproducing
// this: the collision needs the SDK to be *in the program*. With no
// `#import <Foundation/Foundation.h>`, a property called `count` is
// accepted. So the failure is conditional on the name **and** on whether
// the class owning that selector is reachable -- which makes it worse from
// an author's seat, since adding an unrelated import can start it.
//
// What is deliberately **not** changed: protocol properties are collected
// but not wired into conformance checking. A class that adopts a protocol
// property and provides neither accessor nor `@synthesize` is still
// accepted, where an unmet protocol *method* is refused -- measured.
// Making that an error would newly reject programs that build today, so it
// is a separate decision rather than a consequence of collecting the data.

mod common;
use common::{compile_and_run_strict, expect_reject, ozarray_src, ozobject_src};

/// A protocol-declared property, synthesized -- accepted, and the
/// accessors work.
///
/// The runtime oracle matters more than usual here: the fix turns a
/// rejection into an acceptance, so a test that only asserts "no longer
/// refused" would pass on a tree that accepts the source and generates
/// accessors reading the wrong storage. Setting through the setter and
/// reading back through the getter is what proves the synthesized ivar is
/// the one both accessors use.
#[test]
fn a_protocol_declared_property_can_be_synthesized() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>

@protocol Counted
@property (nonatomic) int tally;
@end

@interface Box : OZObject <Counted>
@end
@implementation Box
@synthesize tally = _tally;
@end

int main(void)
{
	Box *b = [[Box alloc] init];

	printf(\"initial=%d\\n\", [b tally]);
	[b setTally:7];
	printf(\"after=%d\\n\", [b tally]);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run_strict(&src, "synthres_protocol_property"),
        "initial=0\nafter=7\n",
        "the getter and setter must agree on one ivar -- Clang accepts this source with zero \
         diagnostics, and oz2c refused it saying no such property was declared"
    );
}

/// Found through protocol **inheritance**, not just direct adoption.
///
/// `@protocol Counted <Tallied>` and the property on `Tallied`. The lookup
/// mirrors `Program::protocol_methods`' transitive walk, and this is what
/// says so -- a lookup over `conforms` alone would pass the case above and
/// fail this one.
#[test]
fn a_property_inherited_through_a_protocol_is_found() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>

@protocol Tallied
@property (nonatomic) int tally;
@end
@protocol Counted <Tallied>
@end

@interface Box : OZObject <Counted>
@end
@implementation Box
@synthesize tally = _tally;
@end

int main(void)
{
	Box *b = [[Box alloc] init];

	[b setTally:3];
	printf(\"tally=%d\\n\", [b tally]);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run_strict(&src, "synthres_protocol_inherited"),
        "tally=3\n",
        "the property is on the protocol that `Counted` extends, so finding it needs the \
         transitive walk rather than a scan of the adopted list"
    );
}

/// An **object** property from a protocol, so the type is rendered.
///
/// Its own case because the collection is deferred for exactly this
/// reason: `extract_property` needs the known-class set to turn `Box *` into
/// `struct Box *`, and protocols are parsed before any `@interface` has
/// been seen. Extracting at parse time with an empty class set types the
/// property as a bare `Box *` and the generated C does not compile, which a
/// scalar `int` property would never have caught.
#[test]
fn an_object_property_from_a_protocol_is_typed() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#include <stdio.h>

@interface Leaf : OZObject
@end
@implementation Leaf
@end

@protocol Holder
@property (nonatomic) Leaf *leaf;
@end

@interface Box : OZObject <Holder>
@end
@implementation Box
@synthesize leaf = _leaf;
@end

int main(void)
{
	Box *b = [[Box alloc] init];
	Leaf *l = [[Leaf alloc] init];

	[b setLeaf:l];
	printf(\"same=%d\\n\", [b leaf] == l);
	return 0;
}
"
    );
    assert_eq!(
        compile_and_run_strict(&src, "synthres_protocol_object_property"),
        "same=1\n",
        "a class-typed protocol property has to render as `struct Leaf *`, which needs the \
         known-class set that only exists after every @interface is collected"
    );
}

/// A **superclass** property still cannot be synthesized, and the message
/// now says why.
///
/// Clang rejects this too -- `property 'tally' attempting to use instance
/// variable '_tally' declared in super class 'Base'` -- so the refusal is
/// right and only the wording was vague. Kept as a case so the protocol fix
/// cannot be widened into accepting it: the subclass would be claiming the
/// superclass's backing ivar.
#[test]
fn a_superclass_property_is_still_refused_and_says_why() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Base : OZObject
@property (nonatomic) int tally;
@end
@implementation Base
@end
@interface Box : Base
@end
@implementation Box
@synthesize tally = _tally;
@end

int main(void) { return 0; }
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("or on any protocol it adopts"),
        "the message must say both places that were searched, since 'not declared on Box' was \
         read as a bug when the property was declared on a protocol; got:\n{}",
        diags
    );
    assert!(
        diags.contains("superclass"),
        "and must name the superclass case, which is the one shape here that is genuinely \
         the author's mistake; got:\n{}",
        diags
    );
}

/// The name collision, and what makes it reproducible.
///
/// A property called `count` collides with `OZArray`'s `-count`, which
/// returns `size_t`. The refusal is correct -- one shared
/// `OZ_PROTOCOL_SEND_count` cannot route two return types -- and the point
/// of this case is the **condition**: the same property called `tally` is
/// accepted, and `count` is accepted too when `OZArray` is not in the
/// program at all.
///
/// That second half is what the report was missing, and it is why this read
/// as instability rather than as a rule: the failure depends on a class the
/// author may not have mentioned.
#[test]
fn the_collision_is_conditional_on_the_sdk_being_present() {
    /* With `OZArray` present, `count` collides. */
    let with_sdk = format!(
        "{}{}{}",
        ozobject_src(),
        ozarray_src(),
        "\
@interface Box : OZObject
@property (nonatomic) int count;
@end
@implementation Box
@synthesize count = _count;
@end

int main(void) { return 0; }
"
    );
    let diags = expect_reject(&with_sdk);
    assert!(
        diags.contains("count"),
        "the collision must be reported against the colliding selector; got:\n{}",
        diags
    );

    /* Without it, the identical property is fine -- so the name alone is
     * not the rule, and a reader of the diagnostic needs to know that. */
    let without_sdk = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Box : OZObject
@property (nonatomic) int count;
@end
@implementation Box
@synthesize count = _count;
@end

int main(void) { return 0; }
"
    );
    assert!(
        oz2c::transpile(&without_sdk).is_ok(),
        "with no OZArray in the program there is nothing to collide with, so the same \
         property is accepted -- the failure is conditional on reachability, not on the name"
    );
}
/// The collision diagnostic names the `@property` that introduced the
/// selector.
///
/// This is the whole of finding 1. The rule was already correct and the
/// message already accurate about the collision -- what it never said was
/// that a `@property` is where the selector came from, so an author who
/// wrote one line of property syntax read a paragraph about
/// `OZ_PROTOCOL_SEND_count` and dynamic dispatch with no thread back to it.
///
/// The code already knew: `locate_property_accessor` asked exactly this
/// question to place the caret. The answer simply never reached the text.
#[test]
fn the_collision_diagnostic_names_the_property() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        ozarray_src(),
        "\
@interface Box : OZObject
@property (nonatomic) int count;
@end
@implementation Box
@synthesize count = _count;
@end

int main(void) { return 0; }
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("is the accessor of '@property count'"),
        "the message must say a @property introduced the selector; got:\n{}",
        diags
    );
    assert!(
        diags.contains("renaming the property is what renames the selector"),
        "and must point the remedy at the property rather than at a method the author never \
         wrote; got:\n{}",
        diags
    );
    /* The original wording stays: it is accurate about the collision, and
     * the note is an addition rather than a replacement. */
    assert!(
        diags.contains("Dispatch is keyed on the selector name alone"),
        "the collision explanation must survive; got:\n{}",
        diags
    );
}

/// A hand-written method that collides gets **no** property note.
///
/// The control for the case above: the note has to be absent when no
/// property is involved, or it is decoration that fires regardless -- the
/// shape of a guard whose message is unconditional.
#[test]
fn a_hand_written_collision_gets_no_property_note() {
    let src = format!(
        "{}{}{}",
        ozobject_src(),
        ozarray_src(),
        "\
@interface Box : OZObject
- (int)count;
@end
@implementation Box
- (int)count
{
	return 0;
}
@end

int main(void) { return 0; }
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("Dispatch is keyed on the selector name alone"),
        "still the collision diagnostic; got:\n{}",
        diags
    );
    assert!(
        !diags.contains("is the accessor of '@property"),
        "but no property note, since the author wrote the method by hand; got:\n{}",
        diags
    );
}
