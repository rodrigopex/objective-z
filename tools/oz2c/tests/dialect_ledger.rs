// SPDX-License-Identifier: Apache-2.0
//
// dialect_ledger.rs -- `docs/OBJECTIVE_C_DIALECT.md` says what an author may
// write. This file asserts the document has not rotted (#583).
//
// `arc_conformance.rs` is the precedent and the shape: a hand-written matrix
// is only worth what a gate makes it worth. The ARC Guide in `README.md` is
// the counter-example -- it taught `__weak` "panics at runtime" where the
// code gives a located error, recommended against `[super dealloc]` where
// the send is refused outright, presented `__bridge_retained` as the working
// exception after #460 made it an error, and imported a header deleted in
// #193. Nothing failed, because nothing checked.
//
// **What a failure here means.** Not "the transpiler is broken". It means a
// row in the dialect document is now wrong, or a construct has appeared that
// nobody documented. Fix the document, rather than relaxing the assertion.
//
// Deliberately *not* asserted: that a verdict is *correct*. No test can read
// prose and decide whether `IMPLEMENTED` was the right call. What is
// mechanised here is the part that rots silently -- vocabulary, citations,
// paths, and completeness against the grammar.

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

const LEDGER: &str = include_str!("../../../docs/OBJECTIVE_C_DIALECT.md");
const DISPOSITION: &str = include_str!("objc_node_disposition.rs");

/// The verdict vocabulary, which is `docs/ARC.md`'s on purpose.
///
/// A seventh word here means the two documents have drifted into two
/// vocabularies, which is two things to keep in step instead of one.
const VERDICTS: &[&str] =
    &["IMPLEMENTED", "DELEGATED", "REFUSED", "N/A", "GAP", "UNEXAMINED"];

/// Row count, pinned. A new construct is a deliberate act; an accidental
/// one is what this catches.
const ROW_COUNT: usize = 99;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Every table row whose first cell is a backticked row id.
///
/// Keyed on the id rather than on the construct text, so rewording a row's
/// prose is free and renaming its id is not -- the ids are the stable
/// handle this document promises.
fn rows() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for line in LEDGER.lines() {
        let line = line.trim();
        if !line.starts_with("| `") {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 3 {
            continue;
        }
        let id = cells[0].trim_matches('`').to_string();
        /* Section 10's table is id | construct | verdict | evidence; the
         * others are id | construct | verdict | contract | evidence. The
         * verdict is cell 2 in both, and the evidence is the last cell. */
        let verdict = cells[2].trim_matches('`').to_string();
        let evidence = cells[cells.len() - 1].to_string();
        out.push((id, verdict, evidence));
    }
    out
}

#[test]
fn the_row_count_is_pinned() {
    let rows = rows();
    assert_eq!(
        rows.len(),
        ROW_COUNT,
        "docs/OBJECTIVE_C_DIALECT.md has {} rows, expected {} -- if a construct was \
         documented or removed on purpose, update ROW_COUNT in the same commit",
        rows.len(),
        ROW_COUNT
    );
}

#[test]
fn every_row_id_is_unique() {
    let rows = rows();
    let mut seen = BTreeSet::new();
    let mut dupes = Vec::new();
    for (id, _, _) in &rows {
        if !seen.insert(id.clone()) {
            dupes.push(id.clone());
        }
    }
    assert!(
        dupes.is_empty(),
        "duplicate row ids in docs/OBJECTIVE_C_DIALECT.md: {:?} -- an id is the stable \
         handle for a construct, so two rows sharing one makes both unaddressable",
        dupes
    );
}

#[test]
fn every_verdict_is_one_of_the_six() {
    let rows = rows();
    let mut bad = Vec::new();
    for (id, verdict, _) in &rows {
        if !VERDICTS.contains(&verdict.as_str()) {
            bad.push(format!("{} => {:?}", id, verdict));
        }
    }
    assert!(
        bad.is_empty(),
        "docs/OBJECTIVE_C_DIALECT.md uses a verdict outside docs/ARC.md's vocabulary \
         {:?}: {:?}\nA seventh word means the two documents have drifted into two \
         vocabularies. Reuse ARC.md's, or change both and this list together.",
        VERDICTS,
        bad
    );
}

#[test]
fn every_gap_cites_an_issue() {
    let rows = rows();
    let mut uncited = Vec::new();
    for (id, verdict, evidence) in &rows {
        if verdict != "GAP" {
            continue;
        }
        /* `#NNN` anywhere in the evidence cell. A GAP is precisely the
         * state the document exists to make visible, and one with no
         * issue behind it is a note to nobody. */
        let cited = evidence
            .split('#')
            .skip(1)
            .any(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit()));
        if !cited {
            uncited.push(id.clone());
        }
    }
    assert!(
        uncited.is_empty(),
        "every `GAP` row must cite the issue tracking it, so the gap is on the tracker \
         and not only in a document: {:?}",
        uncited
    );
}

#[test]
fn every_cited_in_repo_path_exists() {
    let root = repo_root();
    /* The roots a citation is written relative to. `tests/` is implicit
     * for corpus cases (`behavior/cases/...`), because spelling it in
     * every cell would be noise. */
    let roots = [
        "",
        "tests/",
        "tools/oz2c/src/",
        "tools/oz2c/tests/",
        "docs/",
        "src/",
    ];
    let mut missing = Vec::new();
    let mut checked = 0usize;

    for tok in LEDGER.split('`').skip(1).step_by(2) {
        /* A citation is a path-shaped token: it contains a '/' or ends in
         * a source extension, and carries no spaces. An optional `:line`
         * or `:from-to` suffix is stripped. Prose like `_test.c` has no
         * directory and is not a citation -- requiring a '/' is what
         * keeps this from chasing words. A glob -- a token carrying a
         * star -- names a set rather than a file, so it is skipped too;
         * the per-case check is
         * `every_apple_case_has_a_driver_and_a_row`. */
        let path = tok.split(':').next().unwrap_or(tok);
        if path.contains(' ') || !path.contains('/') || path.contains('*') {
            continue;
        }
        if !path.ends_with(".rs")
            && !path.ends_with(".m")
            && !path.ends_with(".c")
            && !path.ends_with(".h")
            && !path.ends_with(".md")
            && !path.ends_with(".py")
        {
            continue;
        }
        checked += 1;
        if !roots.iter().any(|r| root.join(r).join(path).exists()) {
            missing.push(path.to_string());
        }
    }

    /* An absence check paired with a presence check: a regex that stopped
     * matching would report zero missing paths for the same reason a
     * correct document does. */
    assert!(
        checked > 20,
        "only {} path-shaped citations found in docs/OBJECTIVE_C_DIALECT.md -- the \
         document cites far more than that, so this check has stopped reading it",
        checked
    );
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "docs/OBJECTIVE_C_DIALECT.md cites {} path(s) that do not exist: {:?}\nA moved or \
         deleted test leaves the row it backed asserting nothing.",
        missing.len(),
        missing
    );
}

#[test]
fn every_apple_case_has_a_driver_and_a_row() {
    let root = repo_root();
    let dir = root.join("tests/adapted/apple_spec");
    let mut cases: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "m"))
        .map(|p| p.file_stem().unwrap().to_str().unwrap().to_string())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "found no Apple-derived cases to check");

    for stem in &cases {
        let driver = dir.join(format!("{}_test.c", stem));
        assert!(
            driver.is_file(),
            "{}.m has no {}_test.c -- the driver is where the claim is asserted, so a \
             case without one proves nothing",
            stem,
            stem
        );
        assert!(
            LEDGER.contains(&format!("apple_spec/{}.m", stem)),
            "tests/adapted/apple_spec/{}.m is cited by no row in \
             docs/OBJECTIVE_C_DIALECT.md -- an Apple-derived case exists to back a \
             documented construct (#596)",
            stem
        );
    }
}

/// The kinds `objc_node_disposition.rs` lists in one of its `const` tables.
fn disposition_kinds(table: &str) -> Vec<String> {
    let start = DISPOSITION
        .find(&format!("const {}: &[&str] = &[", table))
        .unwrap_or_else(|| panic!("objc_node_disposition.rs has no `{}` table", table));
    let body = &DISPOSITION[start..];
    let end = body.find("\n];").unwrap_or_else(|| panic!("`{}` is unterminated", table));
    body[..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| !s.contains(' ') && !s.is_empty())
        .map(String::from)
        .collect()
}

/// ObjC-only grammar kinds this document deliberately does not give a row.
///
/// Each is a spelling with no separate author-facing decision behind it --
/// it is a *part* of a construct documented elsewhere, not a construct.
/// Naming them here rather than widening the test is the point: an
/// exemption is a claim someone made, and a missing row is not.
const EXEMPT_KINDS: &[(&str, &str)] = &[
    ("method_type", "the parenthesised return type of a method, part of `method.declaration`"),
    ("method_parameter", "a parameter of a method, part of `method.declaration`"),
    ("method_identifier", "a selector piece, part of `method.declaration`"),
    ("keyword_declarator", "a labelled parameter, part of `method.declaration`"),
    ("protocol_reference_list", "the `<P, Q>` list, part of `protocol.conformance`"),
    ("protocol_qualifier", "`in`/`out`/`inout` on a parameter; part of the type, not a construct"),
    ("qualified_protocol_interface_declaration", "the grammar's spelling of `class.extension` with a protocol list"),
    ("instance_variables", "the `{ ... }` block, part of `ivar.declaration`"),
    ("property_attributes_declaration", "the `( ... )` list, part of the `property.*` rows"),
    ("property_attribute", "one attribute within it; see `property.attr-unknown`"),
    ("dictionary_pair", "a `k: v` pair, part of `literal.dictionary`"),
    ("at_expression", "the shared `@`-prefixed expression node behind `literal.*` and `at.defs`"),
    ("catch_clause", "part of `exc.try`"),
    ("finally_clause", "part of `exc.try`"),
    ("availability", "part of `at.available`"),
    ("availability_attribute_specifier", "part of `at.available`"),
    ("platform", "part of `at.available`"),
    ("version", "part of `at.available`"),
    ("version_number", "part of `at.available`"),
    ("block_pointer_declarator", "the type spelling for `block.non-capturing`"),
    ("abstract_block_pointer_declarator", "the same, unnamed"),
    ("parameterized_arguments", "part of `generics.parameterized`"),
    ("generic_arguments", "part of `generics.parameterized`"),
    ("string_literal", "plain C's spelling, shared; `literal.string` covers the `@`-prefixed form"),
];

/// Which row id covers each remaining ObjC-only kind.
///
/// This is the mapping the plan called the honest cost of mechanical
/// completeness: a grammar kind is not an author-facing construct, so
/// something has to say which row answers for which kind. A kind absent
/// from both this table and `EXEMPT_KINDS` fails the test.
const KIND_TO_ROW: &[(&str, &str)] = &[
    ("class_interface", "class.interface"),
    ("class_implementation", "class.interface"),
    ("class_declaration", "class.forward"),
    ("implementation_definition", "category.methods"),
    ("protocol_declaration", "protocol.declaration"),
    ("protocol_forward_declaration", "protocol.forward"),
    ("compatibility_alias_declaration", "class.alias"),
    ("module_import", "preproc.at-import"),
    ("instance_variable", "ivar.declaration"),
    ("visibility_specification", "ivar.visibility"),
    ("property_declaration", "property.scalar"),
    ("property_implementation", "property.synthesize"),
    ("method_declaration", "method.declaration"),
    ("method_definition", "method.declaration"),
    ("message_expression", "send.instance"),
    ("selector_expression", "selector.literal"),
    ("encode_expression", "reflection.encode"),
    ("available_expression", "at.available"),
    ("atdef_field", "at.defs"),
    ("array_literal", "literal.array"),
    ("dictionary_literal", "literal.dictionary"),
    ("objc_bridge", "bridge.plain"),
    ("synchronized_statement", "sync.synchronized"),
    ("try_statement", "exc.try"),
    ("throw_statement", "exc.throw"),
    ("block_literal", "block.non-capturing"),
];

#[test]
fn every_objc_grammar_kind_has_a_row_or_a_named_exemption() {
    let ids: BTreeSet<String> = rows().into_iter().map(|(id, _, _)| id).collect();

    let mut kinds: Vec<String> = Vec::new();
    for table in ["GATED", "OBJC_ONLY_VIA_PARENT", "CLANG_EXTENSION_OR_SHARED"] {
        kinds.extend(disposition_kinds(table));
    }
    /* Presence check first. The tables are read out of another test file by
     * string search, and a rename there would silently yield nothing --
     * whereupon every kind would be "covered" and this test would pass
     * having checked no kind at all. */
    assert!(
        kinds.len() > 45,
        "read only {} ObjC-only kinds out of objc_node_disposition.rs -- the three \
         tables hold more than that, so the extraction has broken rather than the \
         document",
        kinds.len()
    );

    let mut undocumented = Vec::new();
    for kind in &kinds {
        if EXEMPT_KINDS.iter().any(|(k, _)| k == kind) {
            continue;
        }
        match KIND_TO_ROW.iter().find(|(k, _)| k == kind) {
            Some((_, row)) => assert!(
                ids.contains(*row),
                "grammar kind `{}` is mapped to row id `{}`, which does not exist in \
                 docs/OBJECTIVE_C_DIALECT.md",
                kind,
                row
            ),
            None => undocumented.push(kind.clone()),
        }
    }

    undocumented.sort();
    undocumented.dedup();
    assert!(
        undocumented.is_empty(),
        "tree-sitter-objc names {} Objective-C construct(s) that docs/OBJECTIVE_C_DIALECT.md \
         neither documents nor exempts: {:?}\n\nAdd a row for each, or -- if it is a part of \
         a construct documented elsewhere rather than a construct of its own -- add it to \
         EXEMPT_KINDS with the reason. This is #582's argument one level up: a construct \
         nobody classified is how `@dynamic` came to be silently accepted.",
        undocumented.len(),
        undocumented
    );
}
