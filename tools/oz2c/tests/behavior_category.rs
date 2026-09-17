// SPDX-License-Identifier: Apache-2.0
//
// behavior_category.rs - categories (`@interface Foo (Cat)` /
// `@implementation Foo (Cat)`).
//
// New coverage: the oracle's tests/behavior/cases/ has no category case,
// so there was nothing to port and nothing pinning the behavior on either
// side.
//
// Method *bodies* in a category already worked before this file existed:
// `collect`'s pass 1 skips a category interface (it declares no new
// class), but its pass 2 `class_implementation` arm never looked at the
// category name, so `@implementation Foo (Cat)`'s methods were always
// collected onto `Foo`. What did not work was a category-declared
// `@property` -- pass 2 skipped category *interfaces* wholesale, so the
// property was never collected and any use of its accessors failed with
// "class 'Foo' has no method matching 'setSlot2:'".

mod common;
use common::{compile_and_run, ozobject_src};

#[test]
fn category_method_bodies_reach_ivars_and_siblings() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Greeter : OZObject {
	int _n;
}
- (int)base;
@end
@implementation Greeter
- (int)base {
	return 1;
}
@end

@interface Greeter (Extra)
- (int)extra;
- (int)extraPlus:(int)k;
@end

@implementation Greeter (Extra)
- (int)extra {
	return 40 + [self base];
}
- (int)extraPlus:(int)k {
	_n = k;
	return _n + [self extra];
}
@end

#include <stdio.h>
int main(void) {
	Greeter *g = [Greeter alloc];
	printf(\"base=%d\\n\", [g base]);
	printf(\"extra=%d\\n\", [g extra]);
	printf(\"extraPlus=%d\\n\", [g extraPlus:2]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "category_method_bodies_reach_ivars_and_siblings");
    assert_eq!(stdout, "base=1\nextra=41\nextraPlus=43\n");
}

// ---------------------------------------------------------------------------
// A `@property` declared in a category (#530).
//
// **This is a reversal.** `category_property_synthesizes_accessors_once`
// stood here and asserted the opposite of what the three tests below now
// assert: that a category `@property` with an *empty* category
// `@implementation` gets working accessors, read and written through a
// backing ivar oz2c had added to the extended class.
//
// It passed, and it was wrong on both counts. A category cannot add an
// instance variable to the class it extends -- not in Objective-C, and not
// in the generated C, where the extended class's struct is compiled into
// every translation unit that includes its header. What the old behaviour
// produced:
//
//   1. `int _diagnosticCode; /* synthesized: backs property ... */` in
//      `struct PXSensorBase`, storage the class never declared, changing its
//      layout from a header that was not its own; and
//   2. a synthesized getter in the *class's* TU returning that dead field,
//      alongside the real computed getter in the *category's* TU --
//      `ld: multiple definition of 'PXSensorBase_diagnosticCode'`.
//
// The linker error is what saved it. #530's own note is the one to keep in
// view: had the names not collided, reads would have returned the dead
// field. The old test never saw any of this because its category
// `@implementation` was empty, so there was no second definition to collide
// with -- the single shape in which the defect looks like a feature.
//
// Real Objective-C rejects the old test's source too: Clang warns "property
// 'slot2' requires method 'slot2' to be defined", and a send reaches an
// unrecognized selector at runtime. It is now a hard, located error here,
// for the reason in `collect::reject_undefined_category_accessors`.
// ---------------------------------------------------------------------------

/// A category property whose accessors the category really does define --
/// the supported shape, and what `PXSensorBase+Diagnostics` was written to
/// do before `WA-014` gave up the property and used a plain method.
///
/// The property exists so `.` syntax works; the definition is the
/// category's own. Exactly one `PXSensorBase_diagnosticCode` in the program.
#[test]
fn category_property_with_a_real_implementation_runs() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Sensor : OZObject {
	int _sampleCount;
}
- (void)setSampleCount:(int)n;
@end
@implementation Sensor
- (void)setSampleCount:(int)n {
	_sampleCount = n;
}
@end

@interface Sensor (Diagnostics)
@property (nonatomic, readonly) int diagnosticCode;
@end

@implementation Sensor (Diagnostics)
- (int)diagnosticCode {
	return _sampleCount * 100;
}
@end

#include <stdio.h>
int main(void) {
	Sensor *s = [Sensor alloc];
	[s setSampleCount:4];
	printf(\"code=%d\\n\", [s diagnosticCode]);
	printf(\"dot=%d\\n\", s.diagnosticCode);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "category_property_with_a_real_implementation_runs");
    assert_eq!(stdout, "code=400\ndot=400\n");
}

/// The structural half, asserted on the emitted text because the runtime
/// test above cannot see it: no backing ivar, and one definition rather
/// than two.
///
/// Its counterpart is `class_extension.rs::extension_property_gets_real_
/// backing_storage`. The pair is the point -- an extension may add storage
/// and a category may not, and both shapes reach this code as "an
/// `@interface` with parentheses".
#[test]
fn category_property_gets_no_backing_ivar_and_one_definition() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Sensor : OZObject {
	int _sampleCount;
}
@end
@implementation Sensor
@end

@interface Sensor (Diagnostics)
@property (nonatomic, readonly) int diagnosticCode;
@end

@implementation Sensor (Diagnostics)
- (int)diagnosticCode {
	return _sampleCount * 100;
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("a defined category property must transpile");

    /* The needle is the field *declaration*, not the bare ivar name: the
     * name is a substring of `Sensor_diagnosticCode`, the category's own
     * getter, so a broad `!contains("_diagnosticCode")` fails on correct
     * output and would have to be weakened to something that passes on
     * incorrect output too. */
    assert!(
        !out.source_c.contains("int _diagnosticCode;"),
        "a category property must add no field to the extended class.\nsource_c:\n{}",
        out.source_c
    );
    assert_eq!(
        out.source_c.matches("int Sensor_diagnosticCode(struct Sensor *self)\n{").count(),
        1,
        "the category's getter must be defined exactly once.\nsource_c:\n{}",
        out.source_c
    );
    /* And the class's own struct is untouched -- the layout disagreement is
     * the consequence that outlives the link error. */
    assert!(
        out.source_c.contains("struct Sensor {\n\
                               \tstruct OZObject base; /* synthesized: inherited from OZObject */\n\
                               \tint _sampleCount;\n\
                               };\n"),
        "source_c:\n{}",
        out.source_c
    );
}

/// The shape the reversed test asserted was correct: a category property
/// with nothing defining its accessors. It is now refused, located at the
/// `@property`.
#[test]
fn category_property_with_no_implementation_rejected() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Holder : OZObject {
	int _slot;
}
@end
@implementation Holder
@end

@interface Holder (Props)
@property (nonatomic) int slot2;
@end

@implementation Holder (Props)
@end
"
    );
    let diags = oz2c::transpile(&src)
        .err()
        .expect("a category property nothing defines must be refused");
    let text = diags.iter().map(|d| format!("{:?}", d)).collect::<Vec<_>>().join("\n");
    assert!(
        text.contains("category property 'slot2' on 'Holder'"),
        "diagnostics:\n{}",
        text
    );
    /* Both accessors, since the property is not readonly -- a message
     * naming only the getter would leave the author to rediscover the
     * setter after fixing the first. */
    assert!(text.contains("slot2") && text.contains("setSlot2:"), "diagnostics:\n{}", text);
    /* The remedy that is specific to this construct, and the one an author
     * coming from Clang will not guess: a class extension does get storage. */
    assert!(text.contains("class extension"), "diagnostics:\n{}", text);
}

/// A category may restate a selector the main @interface already
/// declared; the merge deduplicates rather than emitting two prototypes.
#[test]
fn category_restating_a_declared_selector_is_deduplicated() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Dup : OZObject
- (int)val;
@end
@implementation Dup
- (int)val {
	return 5;
}
@end

@interface Dup (Again)
- (int)val;
- (int)twice;
@end
@implementation Dup (Again)
- (int)twice {
	return [self val] * 2;
}
@end

#include <stdio.h>
int main(void) {
	Dup *d = [Dup alloc];
	printf(\"val=%d\\n\", [d val]);
	printf(\"twice=%d\\n\", [d twice]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "category_restating_a_declared_selector_is_deduplicated");
    assert_eq!(stdout, "val=5\ntwice=10\n");
}
