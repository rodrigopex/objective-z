// SPDX-License-Identifier: Apache-2.0
//
// class_extension.rs - `@interface Foo ()`, the unnamed category.
//
// The construct had **no coverage at all** before #529. The only class
// extension anywhere in the tree was in `diagnostic_locations.rs`
// (`a_rejection_after_a_repaired_bare_macro_names_the_source_line`, where
// `@interface Pad ()` carries a `weak` property), and that case stops at a
// diagnostic and never reaches `emit` -- so nothing had ever looked at what
// an extension emits. `samples/` has none either.
//
// What it emitted was a second, complete `@interface`: a second
// `struct Foo` whose fields were read off the *extension's* node text and so
// differed from the primary's in both membership and order, a second copy of
// every method prototype, and a second `Foo_oz_alloc`/`_oz_free` definition.
// GCC caught the px-app case only because both structs landed in one header;
// the issue's own note is the important one -- split across two translation
// units it compiles clean and the two disagree about where each ivar lives.
//
// The two halves of the defect are independent, and the second is the worse
// one, so both are pinned here:
//
//  1. `emit::walk_top_level` rendered a full interface per non-category
//     `class_interface` node with no guard against having already rendered
//     this class (`extension_*_one_struct*`).
//  2. `collect`'s pass 2 *assigned* `info.own_ivars` where the
//     `@implementation` arm fifty lines below appends, so the extension
//     **replaced** the primary's ivar list
//     (`extension_declaring_no_ivars_*`).

mod common;
use common::{compile_and_run, ozobject_src};

/// Definitions of `struct {name}` across everything the transpile emits.
///
/// Both halves, because where the struct lands depends on whether the class
/// is the root: `render_interface` hoists a root class's struct into the
/// companion header and leaves every other class's in its own output. A
/// forward declaration (`struct Foo;`) does not match, and must not -- the
/// emitter relies on those.
fn struct_definition_count(out: &oz2c::TranspileOutput, name: &str) -> usize {
    let needle = format!("struct {} {{", name);
    out.source_c.matches(&needle).count() + out.companion_h.matches(&needle).count()
}

/// The px-app shape (`src/challenges/PXStrainGauge.m`, WA-013) reduced to
/// its skeleton: three ivars in the primary `@interface`, a fourth plus a
/// property and a private method in the extension.
///
/// This is the layout disagreement #529 calls "the most dangerous shape
/// found", and the assertion is field order and membership rather than a
/// bare count, because the count was never the defect. Observed before the
/// fix, exactly as the issue reports it:
///
///     the @interface  base, _sensorId, _calibrationOffset, _strain, _faultCount
///     the extension   base, _faultCount, _calibrationOffset
#[test]
fn extension_merges_into_one_struct_carrying_the_primary_layout() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Gauge : OZObject {
	int _sensorId;
	int _calibrationOffset;
	int _strain;
}
- (int)reportedFaults;
@end

@interface Gauge () {
	int _faultCount;
}
- (void)noteFault;
@end

@implementation Gauge
- (void)noteFault {
	_faultCount = _faultCount + 1;
}
- (int)reportedFaults {
	return _faultCount;
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("a class extension must transpile");

    assert_eq!(
        struct_definition_count(&out, "Gauge"),
        1,
        "a class extension must not emit a second struct.\nsource_c:\n{}",
        out.source_c
    );

    /* The whole struct in one assertion, so a field that moves or goes
     * missing fails here rather than in four independent `contains` checks
     * that each stay true while the order rots. */
    let expected = "struct Gauge {\n\
                    \tstruct OZObject base; /* synthesized: inherited from OZObject */\n\
                    \tint _sensorId;\n\
                    \tint _calibrationOffset;\n\
                    \tint _strain;\n\
                    \tint _faultCount; /* declared outside the primary @interface */\n\
                    };\n";
    assert!(
        out.source_c.contains(expected),
        "the merged struct is not the primary layout plus the extension's ivar.\n\
         expected:\n{}\nsource_c:\n{}",
        expected,
        out.source_c
    );
}

/// The eleven `conflicting types for ...` errors #529 reports alongside the
/// redefinition: the second `render_interface` re-emitted every prototype,
/// and `companion::render_alloc_free`'s function *bodies* with them.
#[test]
fn extension_does_not_duplicate_prototypes_or_the_allocator() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Gauge : OZObject
- (int)reportedFaults;
@end

@interface Gauge ()
- (void)noteFault;
@end

@implementation Gauge
- (void)noteFault {
}
- (int)reportedFaults {
	return 7;
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("a class extension must transpile");
    let all = format!("{}{}{}", out.source_c, out.companion_h, out.companion_c);

    /* A prototype declared twice is legal C; a *definition* twice is not,
     * which is why the allocator is the row that mattered. Both are pinned:
     * the prototype count is what told the author something was emitted
     * twice at all. */
    assert_eq!(
        all.matches("void Gauge_oz_free(struct Gauge *obj)\n{").count(),
        1,
        "the allocator must be defined once.\n{}",
        all
    );
    assert_eq!(
        all.matches("struct Gauge *Gauge_oz_alloc(void)\n{").count(),
        1,
        "the allocator must be defined once.\n{}",
        all
    );
    assert_eq!(
        out.source_c.matches("void Gauge_noteFault(struct Gauge *self);").count(),
        1,
        "the extension's method must get exactly one prototype.\nsource_c:\n{}",
        out.source_c
    );
}

/// **The half no one reported, and the one that would have been silent.**
///
/// `collect`'s pass 2 assigned rather than appended, so an extension
/// declaring *no* ivars -- the common shape, adding only private methods --
/// left the class owning none. Nothing in the emitted struct gave it away,
/// because `render_interface` reads the primary node's own text for the
/// fields; what went missing was everything downstream that reads
/// `ClassInfo::own_ivars`. Measured against the fixed and unfixed binaries
/// on identical input, an ivar-less extension cost:
///
///   - the `oz_release` of every owned object ivar in `_oz_free` (a leak),
///   - the release-old half of every ivar store (a second leak),
///   - and ivar *scope* in every method body, so `_held = ...` came out as a
///     bare `_held` rather than `self->_held` -- "use of undeclared
///     identifier", i.e. C that does not compile.
///
/// The release is asserted rather than the struct, because the struct was
/// right the whole time. That is the point.
#[test]
fn extension_declaring_no_ivars_leaves_the_primary_ivars_intact() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Thing : OZObject
@end
@implementation Thing
@end

@interface Owner : OZObject {
	Thing *_held;
}
- (void)setup;
@end

@interface Owner ()
- (void)privateHelper;
@end

@implementation Owner
- (void)setup {
	_held = [Thing alloc];
}
- (void)privateHelper {
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("a class extension must transpile");

    assert!(
        out.source_c.contains("struct Thing *_held;"),
        "the primary's ivar must survive the extension.\nsource_c:\n{}",
        out.source_c
    );
    /* Ownership, not just the field: this is what the clobber actually
     * destroyed, and it is invisible in the struct. */
    assert!(
        out.source_c.contains("oz_release((struct OZObject *)self->_held)"),
        "an owned ivar must still be released on dealloc.\nsource_c:\n{}",
        out.source_c
    );
    /* And ivar scope, which is the difference between C that compiles and C
     * that does not. */
    assert!(
        out.source_c.contains("self->_held = Thing_oz_alloc()"),
        "an ivar store must still resolve through 'self->'.\nsource_c:\n{}",
        out.source_c
    );
}

/// **A class extension may add storage; a category may not.** The one
/// semantic line separating #529's construct from #530's, and the reason
/// `InterfaceKind::may_declare_ivars` exists as a named predicate rather
/// than an inline test for parentheses.
///
/// Keying #530's suppression on "the header had parens" -- the obvious
/// reading of the `saw_paren` bit #529 stopped discarding -- would strip the
/// backing ivar from *this* property, which really does own one. This test
/// is the guard against that, and its counterpart is
/// `behavior_category.rs::category_property_gets_no_backing_ivar`.
#[test]
fn extension_property_gets_real_backing_storage() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Holder : OZObject
@end

@interface Holder ()
@property (nonatomic) int privateSlot;
@end

@implementation Holder
@end
"
    );
    let out = oz2c::transpile(&src).expect("a class extension must transpile");

    assert!(
        out.source_c.contains("int _privateSlot; /* synthesized: backs property 'privateSlot' */"),
        "an extension property must get its backing ivar.\nsource_c:\n{}",
        out.source_c
    );
    /* And a synthesized body, which a category property must not get: the
     * extension is part of the class, so there is a field to read. */
    assert!(
        out.source_c.contains("int Holder_privateSlot(struct Holder *self)\n{"),
        "an extension property must get a synthesized getter.\nsource_c:\n{}",
        out.source_c
    );
    assert_eq!(
        struct_definition_count(&out, "Holder"),
        1,
        "source_c:\n{}",
        out.source_c
    );
}

/// All three shapes on one class at once -- primary, extension, named
/// category -- because each is handled by a different arm of
/// `walk_top_level` and nothing had ever asserted they agree on the number
/// of structs. This is the guard the tree lacked: **exactly one
/// `struct <Class>` per class**, which is what #529 violated.
#[test]
fn all_three_interface_shapes_on_one_class_emit_one_struct() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Trio : OZObject {
	int _fromPrimary;
}
- (int)primaryOnly;
@end

@interface Trio () {
	int _fromExtension;
}
- (int)extensionOnly;
@end

@interface Trio (Extra)
- (int)categoryOnly;
@end

@implementation Trio
- (int)primaryOnly {
	return _fromPrimary + 1;
}
- (int)extensionOnly {
	return _fromExtension + 2;
}
@end

@implementation Trio (Extra)
- (int)categoryOnly {
	return 3;
}
@end
"
    );
    let out = oz2c::transpile(&src).expect("all three shapes must transpile together");

    assert_eq!(
        struct_definition_count(&out, "Trio"),
        1,
        "one class, one struct.\nsource_c:\n{}",
        out.source_c
    );
    /* Positive checks beside the count: a count of one is also what a class
     * whose members all went missing would report. */
    for field in ["int _fromPrimary;", "int _fromExtension;"] {
        assert!(out.source_c.contains(field), "missing {}\nsource_c:\n{}", field, out.source_c);
    }
    for func in [
        "int Trio_primaryOnly(struct Trio *self)\n{",
        "int Trio_extensionOnly(struct Trio *self)\n{",
        "int Trio_categoryOnly(struct Trio *self)\n{",
    ] {
        assert_eq!(
            out.source_c.matches(func).count(),
            1,
            "{} must be defined exactly once.\nsource_c:\n{}",
            func,
            out.source_c
        );
    }
}

/// The construct end to end on the host: an ivar and a method private to
/// the `.m`, which is the idiom `WA-013` had to give up.
#[test]
fn extension_private_ivar_and_method_run() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Gauge : OZObject {
	int _strain;
}
- (int)reportedFaults;
- (void)sample;
@end

@interface Gauge () {
	int _faultCount;
}
- (void)noteFault;
@end

@implementation Gauge
- (void)noteFault {
	_faultCount = _faultCount + 1;
}
- (void)sample {
	_strain = _strain + 10;
	[self noteFault];
}
- (int)reportedFaults {
	return _faultCount;
}
- (int)strain {
	return _strain;
}
@end

#include <stdio.h>
int main(void) {
	Gauge *g = [Gauge alloc];
	[g sample];
	[g sample];
	printf(\"faults=%d\\n\", [g reportedFaults]);
	printf(\"strain=%d\\n\", [g strain]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "extension_private_ivar_and_method_run");
    assert_eq!(stdout, "faults=2\nstrain=20\n");
}
