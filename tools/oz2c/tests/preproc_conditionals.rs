// SPDX-License-Identifier: Apache-2.0
//
// preproc_conditionals.rs -- oz2c decides which arm of a preprocessor
// conditional is part of the program (#570, #573).
//
// oz2c runs before Clang and its own parser reads the raw file rather
// than the preprocessed translation unit, so both arms of every
// conditional reached every pass. That is one cause with two symptoms,
// and which one appeared depended only on the shape of the consumer:
//
//   * A **top-level** walk iterates `root.children()`, and tree-sitter
//     makes the whole conditional one `preproc_if`/`preproc_ifdef` node
//     with the arms as its *children*. So a nested `@interface` reached
//     no arm of `walk_top_level` that knew what to do with it, and the
//     catch-all copied the conditional's text through -- into the
//     generated *header*, because `kind().starts_with("preproc")` routes
//     it there. The output then held raw Objective-C and no `struct`, no
//     method and no dispatch row behind it, so GCC refused the generated
//     C (#573).
//
//   * A **whole-tree** walk descends into both arms, so
//     `check_malformed_sends` fired on text inside `#if 0` that Clang
//     never compiles -- a refusal of a program that builds (#570).
//
// The two halves are one oracle, `preproc::Liveness`, carried on
// `Program` so collect and emit cannot answer differently: a class
// collected from one arm and emitted from the other is not a shape worth
// letting exist.
//
// Three properties these tests exist to hold, each easy to lose:
//
//   * **A resolved conditional does not survive into the generated C.**
//     Not a choice -- a class becomes a struct, a row in the shared
//     dispatch table and a slab sized from its allocation sites, all
//     whole-program artifacts. There is no way to hand the C compiler
//     half a dispatch table, so the arm has to be settled before any C
//     is emitted.
//
//   * **A name absent from the buffer is undefined, except in two
//     namespaces.** That is C's rule and it is what makes
//     `#ifdef MT96_NEVER_DEFINED` decidable at all. It is *unsound* for
//     implementation-reserved names (`__GNUC__`, which the C compiler
//     defines) and for `CONFIG_` (Zephyr's Kconfig, which arrives via
//     `-include autoconf.h`). Guessing "undefined" there would silently
//     select the `#else` arm of every `#ifdef CONFIG_FOO` a real
//     application writes, so both are refusals instead.
//
//   * **A conditional carrying no Objective-C is untouched.** The SDK is
//     full of them -- `#ifndef OZ_Q31_HELPERS`, `#ifdef __OBJC__` -- and
//     they work today because their text is copied through for the C
//     compiler to decide. Measured before the change: 0 of 173
//     repo-owned `.m` files absorb Objective-C into a conditional, which
//     is why 145 of 145 transpilable sources emit byte-identical C
//     across this fix.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// #570: the M92 shape. A send with its colon dropped, parked inside
/// `#if 0`, where Clang never sees it.
///
/// The check that fired is `staticbar::check_malformed_sends`, reached
/// from `collect`, which is a *hard gate* -- it returns before any later
/// pass runs. So this is not merely a spurious message: the build stops.
#[test]
fn malformed_send_inside_if_zero_is_not_reported() {
    let src = format!(
        "{}
#if 0
@interface MT92Dead : OZObject
- (int)run;
@end

@implementation MT92Dead
- (int)run
{{
	return [self badSend missingColon];
}}
@end
#endif

@interface MT92Probe : OZObject
- (int)run;
@end

@implementation MT92Probe
- (int)run
{{
	return 92;
}}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("a malformed send inside `#if 0` is not in the program");
    /* The live class is still lowered -- the point is that the dead arm
     * is ignored, not that the file is skipped. */
    assert!(
        out.source_c.contains("int MT92Probe_run(struct MT92Probe *self)"),
        "the live class should still be emitted:\n{}",
        out.source_c
    );
}

/// The same, through `#ifdef` of a name the buffer never defines rather
/// than through `#if 0`. A different condition kind, the same verdict.
#[test]
fn malformed_send_inside_a_never_defined_ifdef_is_not_reported() {
    let src = format!(
        "{}
#ifdef PP_NEVER_DEFINED
@interface PPDead : OZObject
- (int)run;
@end
@implementation PPDead
- (int)run {{ return [self badSend missingColon]; }}
@end
#endif

@interface PPLive : OZObject
- (int)run;
@end
@implementation PPLive
- (int)run {{ return 7; }}
@end
",
        PREAMBLE()
    );
    oz2c::transpile(&src).expect("a dead `#ifdef` arm is not part of the program");
}

/// #573: the M96 shape. Two complete declarations of one class, the live
/// one behind `#else`.
///
/// Both halves of the defect are asserted, because passing only the first
/// is what the broken tree already did: the raw text *was* in the output.
#[test]
fn live_else_arm_is_lowered_and_the_conditional_does_not_survive() {
    let src = format!(
        "{}
#ifdef MT96_NEVER_DEFINED
@interface MT96Probe : OZObject
- (int)run;
@end
@implementation MT96Probe
- (int)run {{ return -96; }}
@end
#else
@interface MT96Probe : OZObject
- (int)run;
@end
@implementation MT96Probe
- (int)run {{ return 96; }}
@end
#endif
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("the `#else` arm is the program");

    /* Lowered, not copied: a struct, a prototype and a definition. */
    assert!(
        out.source_c.contains("struct MT96Probe {"),
        "the live arm's class should have a struct:\n{}",
        out.source_c
    );
    assert!(
        out.source_c.contains("int MT96Probe_run(struct MT96Probe *self)"),
        "the live arm's method should be lowered:\n{}",
        out.source_c
    );
    /* The *live* arm, not the other one. This is the assertion that would
     * catch an off-by-one in the arm chain -- both arms declare the same
     * class with the same selector, and only the return value tells them
     * apart. */
    assert!(
        out.source_c.contains("return 96;"),
        "the `#else` arm's body is the live one:\n{}",
        out.source_c
    );
    assert!(
        !out.source_c.contains("return -96;"),
        "the dead `#ifdef` arm's body must not be emitted:\n{}",
        out.source_c
    );
    /* And the conditional itself is gone -- resolved, not deferred. A
     * `#ifdef` left in the output would mean the C compiler still decides,
     * which it cannot do for a class. */
    assert!(
        !out.source_c.contains("MT96_NEVER_DEFINED"),
        "the resolved conditional should not reach the generated C:\n{}",
        out.source_c
    );
    /* The companion header is where the raw Objective-C landed before the
     * fix, so it gets its own assertion rather than being covered by
     * `source_c` above. */
    assert!(
        !out.companion_h.contains("@interface MT96Probe : OZObject\n- (int)run;"),
        "raw Objective-C must not reach the companion header:\n{}",
        out.companion_h
    );
}

/// The dispatch table is the artifact that made this a whole-program
/// decision, so it gets its own assertion: the live arm's class has a row.
#[test]
fn the_live_arms_class_reaches_the_dispatch_table() {
    let src = format!(
        "{}
#if 0
@interface PPRow : OZObject
- (int)run;
@end
@implementation PPRow
- (int)run {{ return 1; }}
@end
#else
@interface PPRow : OZObject
- (int)run;
@end
@implementation PPRow
- (int)run {{ return 2; }}
@end
#endif
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("the `#else` arm is the program");
    assert!(
        out.companion_h.contains("OZ_CLASS_PPRow"),
        "the live class needs a dispatch-table id:\n{}",
        out.companion_h
    );
}

/// `#if 1` selects the *then* arm, which is the mirror of the `#else`
/// cases above and the one an inverted condition would break.
#[test]
fn if_one_selects_the_then_arm() {
    let src = format!(
        "{}
#if 1
@interface PPOne : OZObject
- (int)run;
@end
@implementation PPOne
- (int)run {{ return 11; }}
@end
#else
@interface PPOne : OZObject
- (int)run;
@end
@implementation PPOne
- (int)run {{ return 22; }}
@end
#endif
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("`#if 1` takes the then arm");
    assert!(out.source_c.contains("return 11;"), "then arm is live:\n{}", out.source_c);
    assert!(!out.source_c.contains("return 22;"), "else arm is dead:\n{}", out.source_c);
}

/// `#ifndef X` where the buffer never defines `X` selects the then arm --
/// the negation actually applied, rather than dropped.
#[test]
fn ifndef_of_an_undefined_name_selects_the_then_arm() {
    let src = format!(
        "{}
#ifndef PP_ABSENT
@interface PPNdef : OZObject
- (int)run;
@end
@implementation PPNdef
- (int)run {{ return 33; }}
@end
#else
@interface PPNdef : OZObject
- (int)run;
@end
@implementation PPNdef
- (int)run {{ return 44; }}
@end
#endif
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("`#ifndef` of an absent name takes the then arm");
    assert!(out.source_c.contains("return 33;"), "then arm is live:\n{}", out.source_c);
    assert!(!out.source_c.contains("return 44;"), "else arm is dead:\n{}", out.source_c);
}

/// An `#elif` chain: the *first* true arm wins and the rest are dead,
/// including a later arm that would also have been true.
#[test]
fn elif_chain_takes_the_first_true_arm() {
    let src = format!(
        "{}
#ifdef PP_ABSENT_A
@interface PPElif : OZObject
- (int)run;
@end
@implementation PPElif
- (int)run {{ return 1; }}
@end
#elif !defined(PP_ABSENT_B)
@interface PPElif : OZObject
- (int)run;
@end
@implementation PPElif
- (int)run {{ return 2; }}
@end
#else
@interface PPElif : OZObject
- (int)run;
@end
@implementation PPElif
- (int)run {{ return 3; }}
@end
#endif
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("the `#elif` arm is the program");
    assert!(out.source_c.contains("return 2;"), "the `#elif` arm is live:\n{}", out.source_c);
    assert!(!out.source_c.contains("return 1;"), "the `#ifdef` arm is dead:\n{}", out.source_c);
    assert!(!out.source_c.contains("return 3;"), "the `#else` arm is dead:\n{}", out.source_c);
}

/// A conditional nested inside a live arm is resolved too -- the scan
/// descends into the arm it selected, and `effective_top_level` flattens
/// recursively. Without the recursion the inner `#if` would reach the
/// catch-all and its class would be copied through as raw text.
#[test]
fn a_conditional_nested_in_a_live_arm_is_resolved() {
    let src = format!(
        "{}
#if 1
#if 0
@interface PPNest : OZObject
- (int)run;
@end
@implementation PPNest
- (int)run {{ return 55; }}
@end
#else
@interface PPNest : OZObject
- (int)run;
@end
@implementation PPNest
- (int)run {{ return 66; }}
@end
#endif
#endif
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("the inner `#else` arm is the program");
    assert!(
        out.source_c.contains("int PPNest_run(struct PPNest *self)"),
        "the nested live arm should be lowered:\n{}",
        out.source_c
    );
    assert!(out.source_c.contains("return 66;"), "inner else arm is live:\n{}", out.source_c);
    assert!(!out.source_c.contains("return 55;"), "inner then arm is dead:\n{}", out.source_c);
}

/// A `CONFIG_` name is refused rather than assumed undefined: Kconfig
/// reaches the C compiler through `-include autoconf.h`, which oz2c never
/// reads, so "absent from the buffer" says nothing about it.
///
/// This is the case that would otherwise have been *silently* wrong --
/// oz2c would have taken the `#else` arm of every `#ifdef CONFIG_FOO` in
/// a real Zephyr application and emitted a program the author did not
/// write.
#[test]
fn a_kconfig_guarded_class_is_refused_not_guessed() {
    let src = format!(
        "{}
#ifdef CONFIG_PP_SENSOR
@interface PPKconfig : OZObject
- (int)run;
@end
@implementation PPKconfig
- (int)run {{ return 1; }}
@end
#endif
",
        PREAMBLE()
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("cannot evaluate '#ifdef CONFIG_PP_SENSOR'"),
        "the refusal should quote the whole directive, naming the macro:\n{}",
        err
    );
}

/// An implementation-reserved name is refused for the neighbouring
/// reason: the *compiler* defines `__GNUC__`, so no `#define` in the
/// buffer will, and absence proves nothing.
///
/// A shape, not a list -- `is_reserved_name` implements C17 7.1.3, so this
/// holds for a name nobody thought to enumerate.
#[test]
fn a_reserved_name_is_undecidable_rather_than_undefined() {
    let src = format!(
        "{}
#ifdef __GNUC__
@interface PPReserved : OZObject
- (int)run;
@end
@implementation PPReserved
- (int)run {{ return 1; }}
@end
#endif
",
        PREAMBLE()
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("cannot evaluate '#ifdef __GNUC__'"),
        "a compiler-defined name is unknowable, not absent:\n{}",
        err
    );
}

/// The refusal is *located* and carries a remedy, which is the standing
/// rule for anything outside the subset -- there is no soft-diagnostic
/// mode to demote it to.
#[test]
fn the_refusal_is_located_and_actionable() {
    let src = format!(
        "{}
#ifdef CONFIG_PP_THING
@interface PPLocated : OZObject
- (int)run;
@end
@implementation PPLocated
- (int)run {{ return 1; }}
@end
#endif
",
        PREAMBLE()
    );
    match oz2c::transpile(&src) {
        Ok(_) => panic!("an unevaluatable condition around a class must be refused"),
        Err(diags) => {
            let d = diags
                .iter()
                .find(|d| d.message.contains("cannot evaluate"))
                .expect("the conditional refusal should be among the diagnostics");
            assert!(d.span.is_some(), "the refusal needs a span to underline");
            assert!(d.line > 1, "the refusal should name a real line, not the (1,1) fallback");
            assert!(
                d.note.as_deref().unwrap_or("").contains("autoconf.h"),
                "the note should say why oz2c cannot see the macro: {:?}",
                d.note
            );
            assert!(
                !d.help.is_empty(),
                "a refusal of source Clang accepts has to offer a way forward"
            );
        }
    }
}

/// **Behaviour preserved.** A conditional carrying no Objective-C is not
/// oz2c's decision to make: its text passes through and the C compiler
/// decides, which is how every `#ifndef OZ_Q31_HELPERS` in the SDK works.
///
/// The `#ifdef` here names a `CONFIG_` macro deliberately -- the very
/// condition the test above refuses. It is accepted here, and that
/// contrast *is* the property: the refusal is scoped to conditionals
/// whose arm oz2c must lower, not to conditionals in general.
#[test]
fn a_c_only_conditional_is_passed_through_untouched() {
    let src = format!(
        "{}
#ifdef CONFIG_PP_FEATURE
static int pp_feature_level = 1;
#else
static int pp_feature_level = 0;
#endif

@interface PPPlainC : OZObject
- (int)run;
@end
@implementation PPPlainC
- (int)run {{ return pp_feature_level; }}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("a conditional around plain C is the C compiler's");
    let text = format!("{}{}", out.companion_h, out.source_c);
    assert!(
        text.contains("#ifdef CONFIG_PP_FEATURE"),
        "the conditional must survive for the C compiler to decide:\n{}",
        text
    );
    assert!(
        text.contains("pp_feature_level = 1") && text.contains("pp_feature_level = 0"),
        "both arms of a C-only conditional must pass through:\n{}",
        text
    );
}

/// A `#if 0` around plain C keeps passing through as well, even though
/// oz2c now *knows* the arm is dead. Knowing an arm is dead suppresses
/// diagnostics about it; it does not license deleting the author's text.
#[test]
fn a_dead_c_only_arm_still_passes_through() {
    let src = format!(
        "{}
#if 0
static int pp_disabled = 1;
#endif

@interface PPDeadC : OZObject
- (int)run;
@end
@implementation PPDeadC
- (int)run {{ return 1; }}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("plain C in `#if 0` is fine");
    let text = format!("{}{}", out.companion_h, out.source_c);
    assert!(
        text.contains("pp_disabled"),
        "a dead C arm is still the author's text and is copied through:\n{}",
        text
    );
}

/// #573, run rather than read. The generated C compiles and the live
/// arm's body is what executes -- which four defects of 2026-09 showed is
/// a different claim from "the emitted C looks right".
#[test]
fn the_live_arm_runs() {
    let src = format!(
        "{}
#ifdef MT96_NEVER_DEFINED
@interface PPRun : OZObject
- (int)value;
@end
@implementation PPRun
- (int)value {{ return -96; }}
@end
#else
@interface PPRun : OZObject
- (int)value;
@end
@implementation PPRun
- (int)value {{ return 96; }}
@end
#endif

#include <stdio.h>

int main(void) {{
	PPRun *p = [PPRun alloc];
	printf(\"value=%d\\n\", [p value]);
	return 0;
}}
",
        PREAMBLE()
    );
    let out = compile_and_run(&src, "preproc_live_arm_runs");
    assert_eq!(out.trim(), "value=96", "the `#else` arm's body is what runs");
}

/// The nested case, also run: `#if 1` outside, `#if 0`/`#else` inside.
#[test]
fn a_nested_live_arm_runs() {
    let src = format!(
        "{}
#if 1
#if 0
@interface PPNestRun : OZObject
- (int)value;
@end
@implementation PPNestRun
- (int)value {{ return 1; }}
@end
#else
@interface PPNestRun : OZObject
- (int)value;
@end
@implementation PPNestRun
- (int)value {{ return 77; }}
@end
#endif
#endif

#include <stdio.h>

int main(void) {{
	PPNestRun *p = [PPNestRun alloc];
	printf(\"value=%d\\n\", [p value]);
	return 0;
}}
",
        PREAMBLE()
    );
    let out = compile_and_run(&src, "preproc_nested_live_arm_runs");
    assert_eq!(out.trim(), "value=77", "the inner `#else` arm's body is what runs");
}

/// A **footprint** consequence of the same mechanism, found while fixing
/// it rather than reported: `pools.rs` walks the whole tree, so an
/// `[Foo alloc]` inside a dead arm was counted as an allocation site and
/// reserved a slab slot -- real static storage, on a target with no heap.
///
/// Measured on the unfixed tree: three `alloc`s inside `#if 0` took the
/// class's slab from 1 slot to 4. Nothing would have caught it, because
/// the generated C compiles and runs correctly either way; it is simply
/// three objects' worth of RAM the program can never use.
#[test]
fn an_alloc_in_a_dead_arm_does_not_reserve_a_slab_slot() {
    let src = format!(
        "{}
@interface PDThing : OZObject
- (int)v;
@end
@implementation PDThing
- (int)v {{ return 1; }}
@end

@interface PDUser : OZObject
- (int)run;
@end
@implementation PDUser
- (int)run
{{
	PDThing *a = [PDThing alloc];
#if 0
	PDThing *b = [PDThing alloc];
	PDThing *c = [PDThing alloc];
	PDThing *d = [PDThing alloc];
#endif
	return [a v];
}}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("dead allocs are not allocs");
    let slab = out
        .source_c
        .lines()
        .find(|l| l.contains("OZ_SLAB_DEFINE(oz_slab_PDThing"))
        .unwrap_or_else(|| panic!("no slab for PDThing:\n{}", out.source_c));
    /* One live site, so one slot. The unfixed tree emitted `, 4,` here. */
    assert!(
        slab.contains("sizeof(struct PDThing), 1,"),
        "only the live allocation site should reserve storage, got:\n  {}",
        slab
    );
}
