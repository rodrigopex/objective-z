// SPDX-License-Identifier: Apache-2.0
//
// leak_census.rs - `oz_check_all_slabs()`, the exit-time live-object
// census (#451, part of #445).
//
// Nothing in the tree used to answer "was every object freed?".
// `oz_slab_check_leaks` had sat in `include/platform/oz_platform_host.h`
// since the beginning with **zero** call sites in tests, src, samples,
// tools, cmake or the workflows, and the Zephyr backend had no such
// function at all. What stood in for it was one LSan job (gcc, -O0,
// `tests/behavior/` only) plus a one-block-slab exhaustion proxy in the
// corpus -- alloc, release, alloc again, assert non-NULL.
//
// Both instruments are blind in ways this one is not, and the blindnesses
// are structural rather than matters of coverage:
//
//   * LSan reports *unreachable* blocks. An object still held by a
//     file-scope `static Thing *g` is reachable from a root and LSan says
//     nothing. `an_object_parked_in_a_file_scope_static_is_reported` is
//     exactly that program, and it is the reason the census counts
//     allocations against frees instead of tracing reachability.
//   * `-fsanitize=leak` does not exist on arm64 macOS (see
//     `arc_leak_regressions.rs`), so the LSan gate is unreachable on a
//     maintainer's machine. Everything in this file runs there.
//   * the exhaustion proxy needs the pool sized to exactly 1 -- the
//     corpus harness defaults every class to 4, so three leaks fit in the
//     spare slots unnoticed -- it can see only the one class it
//     re-allocates, and it is blind to over-release by construction,
//     because freeing a block twice makes the slab *more* available and
//     the assertion still passes.
//
// Every claim below is made from both sides. A census is the kind of check
// that reads green when it is not running at all, so "reports zero" is
// only worth having next to "reports one, and names the class".
//
// And it is not a hypothetical net: #453's ARC audit found four `+1`s
// that are never released -- in value position, stored into an
// ObjC-pointer parameter, into a non-ivar array element outside a loop,
// and in an aggregate initializer. All four leak, **none corrupts**, so no
// sanitizer observable catches them, and every one stays reachable from a
// root so LSan is silent too.
// `the_census_catches_a_real_arc_leak_no_sanitizer_can_see` is the first
// of those four, measured.

mod common;
use common::{compile_and_run, ozobject_src as PREAMBLE, singleton_protocol_src};

/// The generated C, primary output and companion concatenated -- the slab
/// definitions live in the origin file and the census in the companion, so
/// a test about the relationship between them needs both.
fn generated(src: &str) -> String {
    let out = oz2c::transpile(src).unwrap_or_else(|d| panic!("should transpile: {:?}", d));
    format!("{}\n{}", out.source_c, out.companion_c)
}

fn companion_h(src: &str) -> String {
    oz2c::transpile(src).unwrap_or_else(|d| panic!("should transpile: {:?}", d)).companion_h
}

/// Two classes, both slab-allocated, and one plain C `main`.
const TWO_CLASSES: &str = "\
@interface Widget : OZObject
@end
@implementation Widget
@end

@interface Gadget : OZObject
@end
@implementation Gadget
@end

int main(void)
{
\t{
\t\tWidget *w = [Widget alloc];
\t\t(void)w;
\t}
\t{
\t\tGadget *g = [Gadget alloc];
\t\t(void)g;
\t}
\treturn 0;
}
";

#[test]
fn the_census_names_every_class_that_has_a_slab() {
    let src = format!("{}\n{}", PREAMBLE(), TWO_CLASSES);
    let out = generated(&src);

    // Presence, paired: the slab each call names must actually be defined,
    // or the call is a link error waiting to happen and the assertion
    // below would still pass.
    for class in ["Widget", "Gadget"] {
        assert!(
            out.contains(&format!("OZ_SLAB_DEFINE(oz_slab_{},", class)),
            "expected a slab for {} in:\n{}",
            class,
            out
        );
        assert!(
            out.contains(&format!("extern oz_slab_t oz_slab_{};", class)),
            "the companion must declare {}'s slab to reach it across the \
             translation-unit boundary:\n{}",
            class,
            out
        );
        assert!(
            out.contains(&format!(
                "leaked += oz_slab_check_leaks(&oz_slab_{class}, \"{class}\");",
                class = class
            )),
            "expected {} to be counted, and named by a string literal so the \
             report says which class:\n{}",
            class,
            out
        );
    }

    // Flushed before returning, and only when there is a report to lose.
    // #452 measured stdio discarding a diagnostic printed just before an
    // abort, and added `oz_platform_flush` for it; a census that runs at
    // exit is in the same position.
    assert!(
        out.contains("oz_platform_flush();"),
        "the census must flush before returning a non-zero count:\n{}",
        out
    );

    // The prototype is in the header, because `gen_test_main.py` includes
    // that header rather than writing an `extern` of its own -- which is
    // what makes a signature change a compile error instead of a silent
    // cross-TU mismatch.
    assert!(
        companion_h(&src).contains("int oz_check_all_slabs(void);"),
        "the companion header must declare the census"
    );
}

#[test]
fn a_class_with_no_slab_is_not_counted() {
    // `Ghost` is declared and implemented but never allocated, so
    // `pools::for_class` is 0 and `render_slab_define` emits no slab
    // (#419). There is no counter to read and no symbol to name.
    let src = format!(
        "{}\n\
@interface Ghost : OZObject\n\
@end\n\
@implementation Ghost\n\
@end\n\
\n\
{}",
        PREAMBLE(),
        TWO_CLASSES
    );
    let out = generated(&src);

    // Presence half: the class really is in this program, and the two
    // that *do* have slabs really are counted -- so the absence below is
    // about Ghost and not about the census having been dropped entirely.
    assert!(out.contains("struct Ghost"), "Ghost must be in the program:\n{}", out);
    assert!(
        out.contains("leaked += oz_slab_check_leaks(&oz_slab_Widget, \"Widget\");"),
        "the census must still be counting the slab-allocated classes:\n{}",
        out
    );

    assert!(
        !out.contains("oz_slab_Ghost"),
        "a class with no slab must not be named by the census, or the \
         companion references a symbol nothing defines:\n{}",
        out
    );
}

#[test]
fn an_immortal_singletons_slot_is_excluded_and_the_output_says_why() {
    // The one honest complication #451 names. `render_immortal_marker`
    // marks *every* instance of an OZSingletonProtocol class immortal at
    // alloc and `oz_release` returns before the decrement, so the slot is
    // held for the life of the program by design. Immortality is a
    // per-class property, which is what makes a static exclusion exact.
    let src = format!(
        "{}{}\n\
@interface Config : OZObject <OZSingletonProtocol>\n\
@end\n\
@implementation Config\n\
+ (void)initialize\n\
{{\n\
}}\n\
+ (instancetype)sharedInstance\n\
{{\n\
\treturn [Config alloc];\n\
}}\n\
@end\n\
\n\
int main(void)\n\
{{\n\
\t{{\n\
\t\tWidget *w = [Widget alloc];\n\
\t\t(void)w;\n\
\t}}\n\
\treturn 0;\n\
}}\n",
        PREAMBLE(),
        format!(
            "{}\n@interface Widget : OZObject\n@end\n@implementation Widget\n@end\n",
            singleton_protocol_src()
        )
    );
    let out = generated(&src);

    // Presence: Config is slab-allocated, and marked immortal. Without
    // this the exclusion below would be indistinguishable from Config
    // having no slab at all.
    assert!(
        out.contains("OZ_SLAB_DEFINE(oz_slab_Config,"),
        "Config must have a slab for its exclusion to mean anything:\n{}",
        out
    );
    assert!(
        out.contains("->_meta.immortal = 1;"),
        "Config's instances must be marked immortal:\n{}",
        out
    );

    // Absence: not counted, and not externed either.
    assert!(
        !out.contains("oz_slab_check_leaks(&oz_slab_Config"),
        "an immortal class's held slot is by design, not a leak:\n{}",
        out
    );
    assert!(
        !out.contains("extern oz_slab_t oz_slab_Config;"),
        "nothing in the census reaches Config's slab, so it needs no extern:\n{}",
        out
    );

    // And the skip is *visible*, so a reader of the generated C can see
    // what was left out and why rather than wondering.
    assert!(
        out.contains("Config is excluded: it conforms to OZSingletonProtocol"),
        "the emitted census must name the class it skipped:\n{}",
        out
    );

    // Widget is still counted: the exclusion is per class, not a switch
    // that turns the whole census off once a singleton appears.
    assert!(
        out.contains("leaked += oz_slab_check_leaks(&oz_slab_Widget, \"Widget\");"),
        "a non-singleton in the same program must still be counted:\n{}",
        out
    );
}

#[test]
fn a_program_with_no_slab_at_all_still_gets_a_census() {
    // `gen_test_main.py` calls the census unconditionally, so it has to
    // exist even for a program that reserves nothing -- otherwise the
    // generated main() fails to link, and the failure names a missing
    // symbol rather than the reason.
    let src = format!(
        "{}\n\
@interface Ghost : OZObject\n\
@end\n\
@implementation Ghost\n\
@end\n\
\n\
int main(void)\n\
{{\n\
\treturn 0;\n\
}}\n",
        PREAMBLE()
    );
    let out = generated(&src);

    assert!(
        out.contains("int oz_check_all_slabs(void)\n{"),
        "the census must be defined even with nothing to count:\n{}",
        out
    );
    assert!(
        out.contains("no class in this program reserves a countable slab"),
        "and should say so rather than leaving an empty body:\n{}",
        out
    );
    assert!(
        !out.contains("oz_slab_check_leaks("),
        "with no slab there is nothing to check:\n{}",
        out
    );
}

/// A program that parks its only object in a file-scope `static` --
/// reachable from a root for the whole run, and so invisible to a leak
/// sanitizer -- then asks the census.
///
/// This is the case that justifies the whole approach over "just run more
/// LSan", and it is why the fixture is written this way rather than as a
/// dropped pointer: a dropped pointer LSan would also catch.
const PARKED: &str = "\
@interface Widget : OZObject
@end
@implementation Widget
@end

#include <stdio.h>

static Widget *gParked;

int main(void)
{
\tgParked = [Widget alloc];
\tprintf(\"census=%d\\n\", oz_check_all_slabs());
\treturn 0;
}
";

/// The control: the identical program with the object confined to a scope,
/// so ARC releases it at the closing brace before the census runs.
const RELEASED: &str = "\
@interface Widget : OZObject
@end
@implementation Widget
@end

#include <stdio.h>

int main(void)
{
\t{
\t\tWidget *w = [Widget alloc];
\t\t(void)w;
\t}
\tprintf(\"census=%d\\n\", oz_check_all_slabs());
\treturn 0;
}
";

#[test]
fn an_object_parked_in_a_file_scope_static_is_reported() {
    // `compile_and_run` returns stdout then stderr concatenated (#452's
    // harness change), so the count and the report it printed are both
    // checked here -- the count alone would not say the class was named.
    let out = compile_and_run(&format!("{}\n{}", PREAMBLE(), PARKED), "census_parked");
    assert_eq!(
        out, "census=1\nLEAK: Widget has 1 outstanding allocation(s)\n",
        "one class must report as outstanding, by name"
    );
}

#[test]
fn a_released_object_leaves_the_census_clean() {
    let out = compile_and_run(&format!("{}\n{}", PREAMBLE(), RELEASED), "census_released");
    // Exact, and over both streams: no `LEAK:` line anywhere, which is the
    // half that stops the assertion above from being satisfiable by a
    // census that reports unconditionally.
    assert_eq!(out, "census=0\n", "a balanced program must report nothing");
}

/// A `+1` in **value position** -- `Thing *t = pick ? [[Thing alloc] init]
/// : b;` -- one of the four leaks #453's ARC audit found, and the reason
/// this census is worth more than the issue claims.
///
/// All four of #453's shapes leak and **none corrupts**, so no sanitizer
/// observable catches them; and LSan cannot either, because it reports
/// unreachable blocks while these stay reachable. Counting `num_used` at
/// exit sees them regardless. This is that claim measured rather than
/// asserted: the census reports 1 for a program whose only fault is the
/// missing scope-end release.
///
/// **#477's M1 has landed, so this is now a control rather than a pinned
/// defect.** It asserted `census=1` while the `+1` in value position was
/// never released; `arc::is_owning_expr` gained a `conditional_expression`
/// arm and `emit::render_expr` the matching arm-normalisation, and the
/// census reports 0. Converted rather than deleted, exactly as the row
/// asked to be: the census staying *silent* over a shape that used to leak
/// is worth pinning, and it is the half a census cannot self-check.
///
/// It keeps its value in both directions. `an_object_parked_in_a_file_scope_static_is_reported`
/// is the same instrument answering 1 where a leak is real, so this row
/// answering 0 is not the census having stopped running -- the pair is what
/// makes either number mean anything.
const VALUE_POSITION_LEAK: &str = "\
@interface Thing : OZObject
@end
@implementation Thing
@end

#include <stdio.h>

static int make(int pick, Thing *b)
{
\tThing *t = pick ? [[Thing alloc] init] : b;
\treturn t != 0;
}

int main(void)
{
\t{
\t\tThing *b = [Thing alloc];
\t\tprintf(\"used=%d\\n\", make(1, b));
\t}
\tprintf(\"census=%d\\n\", oz_check_all_slabs());
\treturn 0;
}
";

#[test]
fn the_census_catches_a_real_arc_leak_no_sanitizer_can_see() {
    let src = format!("{}\n{}", PREAMBLE(), VALUE_POSITION_LEAK);
    let out = compile_and_run(&src, "census_value_position");
    assert_eq!(
        out, "used=1\ncensus=0\n",
        "the +1 from the ternary's true arm must be released (#477 M1), and the \
         census must stay silent about it. This row asserted the leak until M1 \
         landed and is now the control for the other direction: a census that \
         reports nothing is indistinguishable from one that is not running, so \
         this number is only meaningful beside \
         `an_object_parked_in_a_file_scope_static_is_reported`, which is the \
         same instrument answering 1. If this says census=1 again, the \
         conditional arm in `arc::is_owning_expr` or the arm-normalisation in \
         `emit::render_expr` has regressed -- they are two halves of one fix."
    );
}
