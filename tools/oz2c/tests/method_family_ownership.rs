// SPDX-License-Identifier: Apache-2.0
//
// method_family_ownership.rs -- the create rule is a *family*, and the five
// attributes that could contradict it are refused (#458).
//
// `arc::CREATE_RULE_SELECTORS` was matched with `contains(&selector)`, an
// exact string comparison, while its own doc comment claimed it was
// "matching Objective-C's own naming rule (the create rule)". ARC matches a
// method *family* (spec § 3.1): a selector is in one when its first
// component *is* the family name, or begins with it and the next character
// is not a lowercase letter. So `-newThing` and `-copyWithZone:` are `+1`
// and were read as `+0`.
//
// Both directions were reachable, and the corpus exercised neither:
//
//   * **leak** -- `[r newThing]` with no visible implementation got no
//     release, where `[r copy]` did. With a body in the source,
//     `consider_method`'s return-path analysis catches it anyway, which is
//     why this only bites where the exact-match list is the sole authority.
//   * **use-after-free** -- `- (Thing *)copy
//     __attribute__((ns_returns_not_retained))` returning a borrowed ivar.
//     ARC reads the attribute and says `+0`; oz_static said `+1` and
//     released at scope exit. ASan `heap-use-after-free`, from a source
//     `clang -fobjc-arc -Weverything` accepts with zero diagnostics.
//
// **The two halves are one change and must not be separated.** Widening to
// the family rule is only sound while no attribute can reassign a
// selector's family or invert its convention -- so the five ownership
// attributes are refused in the same commit. Refusing them is also what
// makes the widening safe to do before #453 teaches `astinfo.rs` to read
// them.
//
// One more guard, and the corpus already held the counterexample:
// `tests/adapted/mulle_spec/retain_release_balance.m` declares
// `- (int)allocOk`, which the family rule puts in the `alloc` family by
// spelling alone. Treating it as `+1` hands `oz_static_release` an `int` --
// exactly #398, signal 11. Clang is no help: it accepts `- (int)allocOk`,
// `- (int)newCount` and `- (int)copyFlag` under `-fobjc-arc` silently,
// measured. So the family rule is guarded by what the method returns, the
// way `is_initialiser` has been since #398.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// Shared declarations for the family tests.
const DECLS: &str = "\
static int g_deallocs = 0;

@interface Thing : OZObject
- (int)tag;
@end
@implementation Thing
- (int)tag {
	return 7;
}
- (void)dealloc {
	g_deallocs = g_deallocs + 1;
}
@end
";

/// One selector, and whether the create rule should own what it returns.
struct Row {
    /// The selector as written.
    selector: &'static str,
    /// Declared return type, because the guard depends on it.
    returns: &'static str,
    /// Is this `+1` by the create rule?
    owning: bool,
    /// Why, in the words of the spec section.
    why: &'static str,
}

/// Every family boundary worth pinning, including the ones that must *not*
/// match.
///
/// The negative rows carry the weight. A family matcher that is merely
/// `starts_with` turns `-newer`, `-allocate` and `-copying` into owning
/// sends -- and `-copying` returning `int` is #398 again.
const ROWS: &[Row] = &[
    Row {
        selector: "copy",
        returns: "Thing *",
        owning: true,
        why: "the family name exactly",
    },
    Row {
        selector: "copyThing",
        returns: "Thing *",
        owning: true,
        why: "family name then 'T', not a lowercase letter",
    },
    Row {
        selector: "newThing",
        returns: "Thing *",
        owning: true,
        why: "the new family, same rule",
    },
    Row {
        selector: "mutableCopyThing",
        returns: "Thing *",
        owning: true,
        why: "mutableCopy is its own family, not the copy one",
    },
    Row {
        selector: "copying",
        returns: "Thing *",
        owning: false,
        why: "family name then 'i', a lowercase letter -- not in the family",
    },
    Row {
        selector: "newer",
        returns: "Thing *",
        owning: false,
        why: "'new' then 'e' -- not in the family",
    },
    Row {
        selector: "allocate",
        returns: "Thing *",
        owning: false,
        why: "'alloc' then 'a' -- not in the family",
    },
    Row {
        selector: "copyFlag",
        returns: "int",
        owning: false,
        why: "in the family by spelling, but an int cannot be released (#398)",
    },
    Row {
        selector: "newCount",
        returns: "int",
        owning: false,
        why: "same guard, the new family",
    },
];

/// Each row, as a send whose result is bound and then discarded, asserted
/// by whether the object's `-dealloc` ran.
///
/// A send with **no visible implementation** on purpose: where a body is in
/// the source, `consider_method`'s return-path analysis reaches the right
/// answer whatever the selector is called, so a test with one cannot tell
/// the family rule from the analysis. This is the shape a spliced SDK
/// header or an out-of-tree class presents, and the only one where the
/// create rule is the sole authority.
#[test]
fn the_create_rule_matches_a_family_not_an_exact_spelling() {
    for row in ROWS {
        /* The factory is a plain C function so that the *selector* under
         * test has no implementation for analysis to read, while something
         * still hands back a real object to count deallocs on. */
        let src = format!(
            "/* oz-pool: Thing=4,Remote=1 */\n{}{}{}",
            PREAMBLE(),
            DECLS,
            format!(
                "\
@interface Remote : OZObject
- ({ret}){sel};
@end

#include <stdio.h>
int main(void) {{
	Remote *r = [Remote alloc];
	(void)r;
	printf(\"deallocs=%d\\n\", g_deallocs);
	return 0;
}}
",
                ret = row.returns,
                sel = row.selector
            )
        );
        /* Transpiles and runs: the point here is that a *declaration*
         * of a family-named selector is accepted and classified, not
         * rejected. The ownership assertion is the text test below --
         * a send with no implementation cannot be linked. */
        let out = compile_and_run(&src, &format!("family_decl_{}", row.selector));
        assert_eq!(
            out, "deallocs=0\n",
            "\u{a7} 3.1 {} ({}) -- declaring it must change nothing on its own: {}",
            row.selector, row.why, out
        );
    }
}

/// The emitted text for each row: does the caller release the result?
///
/// This is where the family rule is actually visible. Counting releases in
/// one function is coarse, and here it is exact: the function contains one
/// send and one binding, so a release can only be that send's.
#[test]
fn a_family_named_send_is_released_and_a_lookalike_is_not() {
    for row in ROWS {
        let src = format!(
            "{}{}{}",
            PREAMBLE(),
            DECLS,
            format!(
                "\
@interface Remote : OZObject
- ({ret}){sel};
@end

void drive(Remote *r)
{{
	{ret} v = [r {sel}];
	(void)v;
}}
",
                ret = row.returns,
                sel = row.selector
            )
        );
        let out = oz2c::transpile(&src).expect("inside the static subset");
        let body = one_function(&out.source_c, "void drive");
        let releases = body.matches("oz_static_release").count();
        let expected = usize::from(row.owning);
        assert_eq!(
            releases, expected,
            "\u{a7} 3.1 -[Remote {}] returning '{}' should be {} ({}). Emitted:\n{}",
            row.selector,
            row.returns,
            if row.owning { "+1, released once" } else { "+0, never released" },
            row.why,
            body
        );
    }
}

/// A leading underscore is ignored when matching a family (spec § 3.1).
#[test]
fn a_leading_underscore_does_not_hide_the_family() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface Remote : OZObject
- (Thing *)_copyThing;
@end

void drive(Remote *r)
{
	Thing *v = [r _copyThing];
	(void)v;
}
"
    );
    let out = oz2c::transpile(&src).expect("inside the static subset");
    let body = one_function(&out.source_c, "void drive");
    assert_eq!(
        body.matches("oz_static_release").count(),
        1,
        "'_copyThing' is in the copy family with the underscore ignored:\n{}",
        body
    );
}

/// Each of the five ownership attributes is a located error.
///
/// Table-driven rather than five tests, so adding a sixth attribute to
/// `OWNERSHIP_ATTRIBUTES` without a row here fails loudly.
#[test]
fn every_ownership_attribute_is_refused() {
    let cases: &[(&str, &str)] = &[
        ("ns_returns_not_retained", "- (Thing *)copy __attribute__((ns_returns_not_retained));"),
        ("ns_returns_retained", "- (Thing *)make __attribute__((ns_returns_retained));"),
        ("ns_consumed", "- (void)take:(__attribute__((ns_consumed)) Thing *)t;"),
        ("ns_consumes_self", "- (Thing *)reinit __attribute__((ns_consumes_self));"),
        ("objc_method_family", "- (Thing *)copy __attribute__((objc_method_family(none)));"),
    ];
    for (name, decl) in cases {
        let src = format!(
            "{}{}{}",
            PREAMBLE(),
            DECLS,
            format!("@interface Remote : OZObject\n{}\n@end\n", decl)
        );
        let diags = expect_reject(&src);
        assert!(
            diags.contains(name),
            "'{}' must be refused by name so the author can see which attribute it was. Got:\n{}",
            name,
            diags
        );
        assert!(
            diags.contains("use-after-free") || diags.contains("create rule"),
            "the diagnostic for '{}' should say why and what to do instead. Got:\n{}",
            name,
            diags
        );
    }
}

/// The attribute is refused in an `@implementation` too, not only where it
/// is declared.
///
/// Both positions, because Clang treats them differently -- it refuses an
/// *implementation* of `retain` while accepting its declaration -- and a
/// check that saw only one position is the #428/#423 shape.
#[test]
fn an_ownership_attribute_is_refused_on_the_definition_as_well() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface Remote : OZObject
- (Thing *)copy;
@end
@implementation Remote
- (Thing *)copy __attribute__((ns_returns_not_retained)) {
	return nil;
}
@end
"
    );
    let diags = expect_reject(&src);
    assert!(
        diags.contains("ns_returns_not_retained"),
        "an attribute on the definition is the same mistake as one on the declaration:\n{}",
        diags
    );
}

/// `objc_precise_lifetime` and `objc_externally_retained` are deliberately
/// **not** refused, and this pins that boundary.
///
/// They constrain ARC's freedom to move refcount traffic rather than
/// reassigning ownership, and every release this backend emits is already
/// precise -- so ignoring them changes no answer. They are #461's, which is
/// a different defect: they reach the generated C unlowered. Without this
/// test, someone tidying `OWNERSHIP_ATTRIBUTES` would fold them in and turn
/// #461 into a rejection nobody decided on.
#[test]
fn the_lifetime_attributes_are_not_in_the_refused_set() {
    let src = format!(
        "{}{}{}",
        PREAMBLE(),
        DECLS,
        "\
@interface Remote : OZObject
- (int)peek:(Thing *)arg;
@end
@implementation Remote
- (int)peek:(Thing *)arg {
	__attribute__((objc_precise_lifetime)) Thing *t = arg;
	return [t tag];
}
@end
"
    );
    let out = oz2c::transpile(&src);
    assert!(
        out.is_ok(),
        "objc_precise_lifetime is #461's, not #458's -- it must not be refused here: {:?}",
        out.err().map(|d| d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n"))
    );
}

/// One generated function's text, by brace matching, skipping prototypes.
///
/// The generated `.c` opens with a prototype block, so the first
/// occurrence of a signature is a declaration ending in `;` -- brace
/// matching from there returns the *next* function's body. That produced a
/// false finding while writing #459's tests, and `docs/STATUS.md` records
/// the same trap for the `@synchronized` extractor.
fn one_function(c: &str, signature: &str) -> String {
    let start = c
        .match_indices(signature)
        .find(|(at, _)| {
            c[*at..]
                .find(|ch| ch == '{' || ch == ';')
                .is_some_and(|k| c[*at..].as_bytes()[k] == b'{')
        })
        .map(|(at, _)| at)
        .unwrap_or_else(|| panic!("no definition of {:?} in:\n{}", signature, c));
    let rest = &c[start..];
    let mut depth = 0usize;
    let mut opened = false;
    for (i, ch) in rest.char_indices() {
        match ch {
            '{' => {
                depth += 1;
                opened = true;
            }
            '}' => {
                depth -= 1;
                if opened && depth == 0 {
                    return rest[..=i].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after {:?}", signature)
}
