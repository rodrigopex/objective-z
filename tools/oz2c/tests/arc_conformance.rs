// SPDX-License-Identifier: Apache-2.0
//
// arc_conformance.rs -- one row per ARC specification rule this project does
// not implement itself, asserting that something else still refuses it.
//
// `docs/ARC.md` gives every normative rule of Clang's ARC specification a
// verdict. Two of those verdicts are claims about behaviour rather than
// descriptions of code, and so can rot without anything noticing:
//
//   * `DELEGATED` -- `clang -fobjc-arc` refuses the construct before `oz2c`
//     sees it, so oz2c needs no rule of its own. This is the whole
//     reason the transpiler does not reimplement ARC's front end, and it
//     rests on a compiler this repo does not own. A Clang upgrade, a changed
//     target triple or a dropped flag can retire any of these silently.
//   * `REFUSED` -- a located oz2c error.
//
// #443 is the precedent and the reason this file exists in this shape. The
// eleven rejections in `static_bar_rejects.rs` enforce a rule `-fobjc-arc`
// makes true, and every one of them kept passing with the flag removed --
// the property was gone and the tests could not see it.
// `arc_flag_is_universal.rs` closed that by asserting the flag is *present*
// on all six paths. This file asserts the other half: that the flag, on the
// clang this project actually uses, still produces the refusals the matrix
// credits it with.
//
// **What a failure here means.** Not "the code is broken" -- these tests
// exercise almost no oz2c code. It means a verdict in `docs/ARC.md` is
// now wrong, and the rule it covers may have become reachable. Fix the
// matrix and decide whether oz2c now needs its own rule, rather than
// relaxing the assertion.
//
// Deliberately *not* asserted here: whether a release is correctly placed.
// That is `ownership_matrix.rs` (every sink, by refcount count) and
// `selector_ownership_matrix.rs` (every selector, by observed output). This
// file is about which rules apply; those are about whether the emitter
// honours them.

mod common;

use std::fs;
use std::process::Command;

/// Every declaration a probe might need, so a probe body stays one idea.
///
/// The four memory-management selectors are **declared** on purpose. Clang
/// answers `no visible @interface ... declares the selector 'retain'` for an
/// undeclared one, which is an ordinary unknown-selector error and says
/// nothing about ARC -- a probe written without these declarations passes
/// while testing the wrong thing. Declared, the answer is
/// `ARC forbids explicit message send of 'retain'`, which is the rule § 4.1
/// actually states. Measured both ways before writing it down.
const PREAMBLE: &str = "\
@interface OZObject\n\
- (instancetype)init;\n\
+ (instancetype)alloc;\n\
- (id)retain;\n\
- (void)release;\n\
- (id)autorelease;\n\
- (int)retainCount;\n\
- (void)dealloc;\n\
@end\n\
@interface Thing : OZObject\n\
- (int)tag;\n\
@end\n";

/// Run `clang -fobjc-arc` over `source` and return its diagnostics.
///
/// The flags are `_objz_build_ast_flags()`'s (`cmake/ObjcClang.cmake:354`)
/// plus the target triple every dump uses, and **minus `-w`**. The AST-dump
/// path silences warnings because the dump is transpiler input and the noise
/// is not actionable there; here the diagnostics are the entire subject.
/// `objz_clang.py` picks the compiler, so this asks the same clang the
/// Zephyr build does (#269).
fn clang_diagnostics(name: &str, source: &str) -> String {
    let dir = common::test_scratch_dir("arc_conformance");
    fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("cannot create {}: {}", dir.display(), e));
    let probe = dir.join(format!("{}.m", name));
    fs::write(&probe, source)
        .unwrap_or_else(|e| panic!("cannot write {}: {}", probe.display(), e));

    let out = Command::new(common::ast_clang())
        .arg("-x")
        .arg("objective-c")
        .arg("-fsyntax-only")
        .args(["-fobjc-runtime=macosx", "-fobjc-arc", "-fblocks"])
        .arg("--target=x86_64-unknown-linux-gnu")
        .arg("-ferror-limit=0")
        .arg(&probe)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {}", common::ast_clang(), e));

    let _ = fs::remove_file(&probe);
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// One `DELEGATED` row: the rule, the smallest source that trips it, and the
/// substring of Clang's answer that proves it did.
struct Delegated {
    /// The specification section, so a failure can be read against the spec.
    section: &'static str,
    /// The rule, in the words `docs/ARC.md` uses.
    rule: &'static str,
    /// Appended to `PREAMBLE`.
    body: &'static str,
    /// A fragment of the expected `error:` line. Matched as a substring so a
    /// future Clang may reword the rest.
    diagnostic: &'static str,
}

const DELEGATED: &[Delegated] = &[
    Delegated {
        section: "4.1",
        rule: "a send of 'retain' is ill-formed",
        body: "void f(Thing *t) { [t retain]; }\n",
        diagnostic: "ARC forbids explicit message send of 'retain'",
    },
    Delegated {
        section: "4.1",
        rule: "a send of 'release' is ill-formed",
        body: "void f(Thing *t) { [t release]; }\n",
        diagnostic: "ARC forbids explicit message send of 'release'",
    },
    Delegated {
        section: "4.1",
        rule: "a send of 'autorelease' is ill-formed",
        body: "void f(Thing *t) { [t autorelease]; }\n",
        diagnostic: "ARC forbids explicit message send of 'autorelease'",
    },
    Delegated {
        section: "4.1",
        rule: "a send of 'retainCount' is ill-formed",
        body: "void f(Thing *t) { (void)[t retainCount]; }\n",
        diagnostic: "ARC forbids explicit message send of 'retainCount'",
    },
    Delegated {
        section: "4.1",
        rule: "'retain' in a @selector is ill-formed",
        body: "void f(void) { SEL s = @selector(retain); (void)s; }\n",
        diagnostic: "ARC forbids use of 'retain' in a @selector",
    },
    Delegated {
        section: "4.1",
        rule: "an implementation of 'retain' is ill-formed",
        body: "@implementation Thing\n- (id)retain { return self; }\n- (int)tag { return 1; }\n@end\n",
        diagnostic: "ARC forbids implementation of 'retain'",
    },
    Delegated {
        section: "4.2",
        rule: "a send of 'dealloc' is ill-formed",
        body: "void f(Thing *t) { [t dealloc]; }\n",
        diagnostic: "ARC forbids explicit message send of 'dealloc'",
    },
    Delegated {
        section: "4.2",
        rule: "[super dealloc] is a send of 'dealloc' like any other",
        body: "@implementation Thing\n- (void)dealloc { [super dealloc]; }\n- (int)tag { return 1; }\n@end\n",
        diagnostic: "ARC forbids explicit message send of 'dealloc'",
    },
    Delegated {
        section: "4.4",
        rule: "'self' is externally retained outside the init family",
        body: "@implementation Thing\n- (int)tag { self = 0; return 1; }\n@end\n",
        diagnostic: "cannot assign to 'self' outside of a method in the init family",
    },
    Delegated {
        section: "1.4",
        rule: "an object-to-C-pointer conversion needs a bridged cast",
        body: "void f(Thing *t) { void *p = (void *)t; (void)p; }\n",
        diagnostic: "requires a bridged cast",
    },
    Delegated {
        section: "2.6.3",
        rule: "converting between differently-qualified pointers is ill-formed",
        body: "void f(__strong Thing **p) { __weak Thing **q = (__weak Thing **)p; (void)q; }\n",
        diagnostic: "changes retain/release properties of pointer",
    },
    Delegated {
        section: "2.2",
        rule: "__weak is unavailable on this deployment target",
        body: "void f(Thing *t) { __weak Thing *w = t; (void)w; }\n",
        diagnostic: "cannot create __weak reference",
    },
];

/// Every `DELEGATED` verdict in `docs/ARC.md`, asserted against the clang
/// this project uses.
#[test]
fn delegated_rules_are_still_refused_by_clang() {
    let mut broken = Vec::new();
    /* The probe filename carries the row *index*, not a digest of the rule.
     * Naming it `s{section}_{rule.len()}` collided on the first attempt --
     * "a send of 'autorelease' is ill-formed" and "a send of 'retainCount'
     * is ill-formed" are both 37 characters in § 4.1 -- so two rows wrote
     * the same file. Harmless while this loop is sequential, and a flake
     * the moment it is not. */
    for (index, row) in DELEGATED.iter().enumerate() {
        let diags = clang_diagnostics(
            &format!("row{:02}_s{}", index, row.section.replace('.', "_")),
            &format!("{}{}", PREAMBLE, row.body),
        );
        if !diags.contains(row.diagnostic) {
            broken.push(format!(
                "  \u{a7} {} -- {}\n    expected a diagnostic containing: {}\n    clang said:\n{}",
                row.section,
                row.rule,
                row.diagnostic,
                diags
                    .lines()
                    .filter(|l| l.contains("error:") || l.contains("warning:"))
                    .map(|l| format!("      {}", l))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    assert!(
        broken.is_empty(),
        "docs/ARC.md credits `clang -fobjc-arc` with refusing these, and it no \
         longer does. That does not mean a test is wrong -- it means the rule may \
         now be reachable, because tree-sitter is the primary frontend and is more \
         permissive than Clang. Update the verdict in docs/ARC.md and decide \
         whether oz2c needs a rule of its own.\n\n{}",
        broken.join("\n\n")
    );
}

/// Every `DELEGATED` row must actually be a *Clang* rule and not an accident
/// of the probe.
///
/// Written because the first draft of this file got exactly that wrong. Four
/// rows asserted `no visible @interface for 'Thing' declares the selector
/// 'retain'` -- an unknown-selector error, present for any misspelled
/// selector, that has nothing to do with ARC. The probes passed and tested
/// nothing. So: the same bodies must be *accepted* without `-fobjc-arc`,
/// which is what makes each refusal attributable to ARC.
#[test]
fn each_delegated_refusal_is_attributable_to_arc() {
    let dir = common::test_scratch_dir("arc_conformance_noarc");
    fs::create_dir_all(&dir).unwrap();
    let mut not_arc = Vec::new();
    for row in DELEGATED {
        /* `__weak` and the qualifier-conversion rule are ARC-only spellings:
         * without `-fobjc-arc` Clang rejects them for a different reason
         * (or accepts them as a no-op), so the comparison says nothing.
         * Named rather than filtered by pattern, so adding a row forces a
         * decision about it. */
        if row.section == "2.2" || row.section == "2.6.3" {
            continue;
        }
        let probe = dir.join("probe.m");
        fs::write(&probe, format!("{}{}", PREAMBLE, row.body)).unwrap();
        let out = Command::new(common::ast_clang())
            .arg("-x")
            .arg("objective-c")
            .arg("-fsyntax-only")
            .args(["-fobjc-runtime=macosx", "-fblocks"])
            .arg("--target=x86_64-unknown-linux-gnu")
            .arg("-ferror-limit=0")
            .arg(&probe)
            .output()
            .unwrap();
        let diags = String::from_utf8_lossy(&out.stderr);
        if diags.contains("error:") {
            not_arc.push(format!(
                "  \u{a7} {} -- {}\n    rejected even without -fobjc-arc, so the row proves \
                 nothing about ARC:\n{}",
                row.section,
                row.rule,
                diags
                    .lines()
                    .filter(|l| l.contains("error:"))
                    .map(|l| format!("      {}", l))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    let _ = fs::remove_dir_all(&dir);
    assert!(
        not_arc.is_empty(),
        "a DELEGATED row must be refused *because of* ARC. These are refused \
         anyway, so they cannot support the verdict they are cited for:\n\n{}",
        not_arc.join("\n\n")
    );
}

/// One `REFUSED` row: a rule oz2c enforces itself, with a located error.
struct Refused {
    section: &'static str,
    rule: &'static str,
    /// A whole source, since `staticbar` needs a class to scan.
    source: &'static str,
    /// A fragment of oz2c's own diagnostic.
    diagnostic: &'static str,
}

const REFUSED: &[Refused] = &[
    Refused {
        section: "2.2",
        rule: "a __weak ivar is refused rather than emitted",
        source: "\
@interface Thing : OZObject\n\
{\n\
\t__weak Thing *_other;\n\
}\n\
@end\n\
@implementation Thing\n\
@end\n",
        diagnostic: "__weak",
    },
    Refused {
        section: "2.4",
        rule: "a weak property is refused -- Clang accepts the declaration, so this is ours",
        source: "\
@interface Thing : OZObject\n\
@property (weak) Thing *other;\n\
@end\n\
@implementation Thing\n\
@end\n",
        diagnostic: "weak",
    },
];

/// Every `REFUSED` verdict that is oz2c's own rule *and has no other
/// home*.
///
/// Thin on purpose, and not exhaustive by design -- a `REFUSED` verdict is
/// pinned wherever its own reasoning lives, and duplicating it here is the
/// "same fix twice" shape `docs/STATUS.md` warns about. The other homes:
///
///   * `static_bar_rejects.rs` -- the memory-management sends (#428, #436).
///   * `method_family_ownership.rs` -- the five ARC ownership attributes
///     (#458), together with the family rule whose soundness depends on
///     them being refused.
///   * `emit`'s own tests -- `@autoreleasepool` (#430).
///   * `weak_every_position.rs` -- `__weak` in all ten positions a
///     `type_qualifier` reaches, and the matching proof that
///     `__unsafe_unretained` is accepted in each, since that is the remedy
///     the diagnostic names (#448). The two rows below stay: they are the
///     *ivar* and the *property attribute*, which were the only two
///     refusals before that walk existed, and they are the two cases where
///     `clang -fobjc-arc` accepts the source outright.
///
/// What is left for this file is the rejections a reader would otherwise
/// assume were Clang's. Both rows below are cases where
/// `clang -fobjc-arc` **accepts** the source and oz2c does not, so
/// nothing but this test says the refusal is ours to keep.
#[test]
fn refused_rules_are_located_oz2c_errors() {
    for row in REFUSED {
        let diags = common::expect_reject(&format!("{}{}", common::ozobject_src(), row.source));
        assert!(
            diags.contains(row.diagnostic),
            "\u{a7} {} -- {}\nexpected oz2c's diagnostic to mention {:?}, got:\n{}",
            row.section,
            row.rule,
            row.diagnostic,
            diags
        );
    }
}

/// The matrix is a document, and a document drifts. Hold its shape.
///
/// Not a spell-check: it asserts the two verdicts this file exists to pin are
/// still the ones the document uses, and that every `GAP` row cites an issue.
/// A `GAP` with no issue number is the state `docs/ARC.md` is meant to make
/// impossible -- a known hole nobody can find again.
#[test]
fn every_gap_in_the_matrix_cites_an_issue() {
    const MATRIX: &str = include_str!("../../../docs/ARC.md");

    for verdict in ["`IMPLEMENTED`", "`DELEGATED`", "`REFUSED`", "`N/A`", "`GAP`"] {
        assert!(
            MATRIX.contains(verdict),
            "docs/ARC.md no longer uses the verdict {}; this file and the document \
             have diverged",
            verdict
        );
    }

    /* A `GAP` row must name an issue. `#` followed by digits is the
     * reference form CLAUDE.md mandates, and the `OZ-NNN` scheme is
     * retired, so there is exactly one spelling to look for.
     *
     * Counted by *cells* rather than matched on the word, because the
     * document says `GAP` in three shapes and only one of them is a rule:
     * the four-column verdict tables (section, rule, verdict, evidence),
     * the two-column legend that defines the word, and the three-column
     * work-queue tables that cite the issue in their first cell. Matching
     * the word alone flagged the legend, which was the first thing this
     * test did -- correctly, since the legend's own claim about gaps was
     * wrong too. */
    let mut uncited = Vec::new();
    for line in MATRIX.lines() {
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
        if cells.len() != 4 || cells[2].trim() != "`GAP`" {
            continue;
        }
        let cites_issue = line
            .split('#')
            .skip(1)
            .any(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit()));
        if !cites_issue {
            uncited.push(line.trim().to_string());
        }
    }
    assert!(
        uncited.is_empty(),
        "every `GAP` row in docs/ARC.md must cite the issue tracking it, so a \
         known hole cannot become one nobody can find. These do not:\n{}",
        uncited.join("\n")
    );
}
