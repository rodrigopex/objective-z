// SPDX-License-Identifier: Apache-2.0
//
// check_arc_audit.rs - `oz2c --check-arc` end to end, over real sources and
// a real Clang dump.
//
// The unit tests in `astinfo` use hand-written excerpts, which is what lets
// them pin one location shape at a time. They cannot tell you that the
// shapes they pin are the shapes Clang actually emits. These tests run the
// real binary over sources already in the corpus, with a dump produced by
// the same clang every other harness here uses, so a change in Clang's
// output shows up as a failure rather than as an audit quietly reporting
// nothing.

mod common;

use std::path::PathBuf;
use std::process::Command;

fn oz2c_binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    path.pop();
    path.join("oz2c")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Audit one corpus case, dumping its AST with the harness's own clang.
fn audit(case: &str) -> String {
    let root = repo_root();
    let src = root.join(case);
    assert!(src.is_file(), "no such corpus case: {}", src.display());

    let dir = std::env::temp_dir().join(format!(
        "oz_check_arc_{}",
        src.file_stem().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let ast = dir.join("case.ast.json");
    common::ast_dump_file(&src, &ast);

    let out = Command::new(oz2c_binary())
        .arg("--check-arc")
        .arg("-I")
        .arg(root.join("tests/behavior/include"))
        .arg("--ast")
        .arg(&ast)
        .arg(&src)
        .output()
        .expect("run oz2c --check-arc");

    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "--check-arc is an audit, not a gate, so it must succeed on correct \
         source.\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 report")
}

/// The audit resolves marks to positions in the file it was asked about.
///
/// `reassign_releases_old.m` is three reassignments of one local, so its
/// marks are one binding and two stores -- and Clang's positions for those
/// are `VarDecl` and `BinaryOperator`. Verified against the source bytes:
/// offsets 352, 371 and 390 are lines 20, 21 and 22.
#[test]
fn attributes_marks_to_the_positions_clang_puts_them_in() {
    let report = audit("tests/behavior/cases/arc/reassign_releases_old.m");
    assert!(
        report.contains("ARCConsumeObject           VarDecl"),
        "the binding on line 20:\n{}",
        report
    );
    assert!(
        report.contains("ARCConsumeObject           BinaryOperator"),
        "the two reassignments:\n{}",
        report
    );
}

/// Marks in the SDK's own sources are counted apart from the file under
/// audit.
///
/// Not cosmetic: across the `arc`, `memory` and `lifecycle` corpora, 45 of
/// 152 marks -- 29.6% -- are in `src/*.m` rather than the case, and all 45
/// are one shape (`ARCProduceObject` on a `ReturnStmt`). An unfiltered
/// report's largest row would be SDK boilerplate every time.
#[test]
fn separates_this_file_from_the_rest_of_the_dump() {
    let report = audit("tests/behavior/cases/arc/reassign_releases_old.m");
    let line = report
        .lines()
        .find(|l| l.contains("ARC transfers by position"))
        .unwrap_or_else(|| panic!("no transfer section:\n{}", report));
    assert!(
        line.contains("in this file") && line.contains("elsewhere in the dumps"),
        "both counts must be stated: {}",
        line
    );
    /* The SDK's `alloc` and `init` are in every dump of every case, so
     * "elsewhere" is never zero here -- if it were, the filter would be
     * matching nothing and the in-file count would be the whole dump. */
    assert!(
        !line.contains("0 elsewhere in the dumps"),
        "the filter is not narrowing anything: {}",
        line
    );
}

/// A `+1` discarded at statement level has a handler, and the audit knows
/// which.
///
/// This case is why `CompoundStmt` is in `HANDLED_POSITIONS`: Clang marks
/// the consume of `[t copy];` against the enclosing compound statement,
/// because a dropped value sits in no expression position. Running the
/// audit over the corpus is what found it -- the list was incomplete and
/// said so, which is the direction it is allowed to fail in.
#[test]
fn a_discarded_plus_one_is_attributed_to_its_statement() {
    let report = audit("tests/behavior/cases/arc/discarded_owning_return.m");
    assert!(
        report.contains("ARCConsumeObject           CompoundStmt"),
        "`[t copy];` on line 40:\n{}",
        report
    );
    assert!(
        report.contains("arc::discarded_owning_value"),
        "and it has a declared handler:\n{}",
        report
    );
    assert!(
        !report.contains("NO HANDLER DECLARED"),
        "nothing in this case should be unhandled:\n{}",
        report
    );
}

/// An ivar oz2c synthesized is reported as such, not as a missing fact.
///
/// `OZObject.oz_prop_lock` is added to the root class by
/// `collect::resolve_properties` and appears in no source file, so no dump
/// can describe it. The first run of this audit called it "no Clang
/// answer", which sends a reader looking for something that does not
/// exist. The distinction is `knows_class`: the dump covered `OZObject`
/// and not this ivar, so oz2c is where the ivar came from.
#[test]
fn a_synthesized_ivar_is_not_reported_as_a_gap() {
    let report = audit("tests/behavior/cases/arc/owning_argument.m");
    let row = report
        .lines()
        .find(|l| l.contains("oz_prop_lock"))
        .unwrap_or_else(|| panic!("no oz_prop_lock row:\n{}", report));
    assert!(
        row.contains("synthesized by oz2c"),
        "it must say where the ivar came from: {}",
        row
    );
    assert!(
        !row.contains("DISAGREE"),
        "an ivar Clang cannot see is not a disagreement: {}",
        row
    );
    /* And an ivar both models do see is still diffed. */
    assert!(
        report.contains("Holder._thing") && report.contains("clang=owned"),
        "the real ivar is still compared:\n{}",
        report
    );
}

/// The audit states what it cannot answer.
///
/// Required by #453, and load-bearing rather than boilerplate: the marks do
/// not distinguish a `+1` class send from a `+0` one at a call site, nor
/// the create-rule family from the autorelease convention. A reader who
/// takes the tables as complete concludes #361 is answerable from this
/// dump.
#[test]
fn states_its_own_limits() {
    let report = audit("tests/behavior/cases/arc/reassign_releases_old.m");
    for expected in [
        "what this audit cannot tell you",
        "ARCReclaimReturnedObject on all three",
        "#361 is not answerable",
        "create-rule family",
        "declared list in `checkarc.rs`",
    ] {
        assert!(report.contains(expected), "missing {:?}:\n{}", expected, report);
    }
}

/// Without a dump there is nothing to audit against, and that is an error
/// rather than an empty report.
///
/// An audit that silently compares against nothing is the failure mode this
/// repo keeps paying for -- a gate that passes because it measured the
/// wrong thing.
#[test]
fn refuses_to_audit_with_no_dump() {
    let root = repo_root();
    let out = Command::new(oz2c_binary())
        .arg("--check-arc")
        .arg("-I")
        .arg(root.join("tests/behavior/include"))
        .arg(root.join("tests/behavior/cases/arc/reassign_releases_old.m"))
        .output()
        .expect("run oz2c --check-arc");
    assert!(!out.status.success(), "no dump must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("needs at least") && stderr.contains("--ast"),
        "and say what is missing: {}",
        stderr
    );
}
