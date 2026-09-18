// SPDX-License-Identifier: Apache-2.0
//
// progress_observer.rs - the pipeline reports which pass it has reached.
//
// A px-keyboard build spends ~30s in the oz2c path and prints two
// lines, which from the outside is indistinguishable from a hang (#299).
// `progress::Observer` is how the pipeline says where it is.
//
// What is asserted here is the *sequence*, which is deterministic. Nothing
// asserts a duration: those live in the binary's `report` module precisely
// so that the library half stays testable. A test that asserted a
// magnitude, or that the phases sum to the wall clock, would be flaky by
// construction -- the rows are spans that bracket each other, not a
// partition.

mod common;
use common::ozobject_src;

use oz2c::progress::{Observer, Phase};

/// Records what it is told, in order.
#[derive(Default)]
struct Recorder {
    phases: Vec<Phase>,
    dumps: Vec<(usize, usize)>,
}

impl Observer for Recorder {
    fn enter(&mut self, phase: Phase) {
        self.phases.push(phase);
    }
    fn ast_dump(&mut self, index: usize, json_bytes: usize) {
        self.dumps.push((index, json_bytes));
    }
}

fn holder_src() -> String {
    format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Holder : OZObject {
	int _count;
}
- (int)count;
@end
@implementation Holder
- (int)count { return _count; }
@end
"
    )
}

/// The library reports exactly the passes it runs, once each, in pipeline
/// order.
///
/// This is the test that catches a pass being added without a phase, a
/// phase entered twice, or the order changing -- the sequence is
/// order-dependent for real reasons (the repair must precede anything
/// reading a byte offset, `ast-ingest` must precede `arc` because ARC reads
/// the facts it attaches, `generics` must see a populated `Program`).
///
/// `import-resolve`, `root-class-check`, `ast-read` and `write` are absent
/// on purpose: they are filesystem phases and belong to `main.rs`, which is
/// outside the pure pipeline this exercises.
#[test]
fn the_library_reports_its_passes_in_pipeline_order() {
    let mut rec = Recorder::default();
    oz2c::transpile_observed(&holder_src(), &oz2c::Options::default(), &mut rec)
        .expect("the fixture transpiles");

    assert_eq!(
        rec.phases,
        vec![
            Phase::Repair,
            Phase::Collect,
            Phase::AstIngest,
            Phase::Arc,
            Phase::Generics,
            Phase::Pools,
            Phase::Emit,
        ]
    );
}

/// `ast-ingest` is entered even with no dumps supplied.
///
/// So the sequence has one shape whether or not `--ast` was given, which is
/// what lets the test above compare against a single literal list -- and
/// what stops a build's phase table changing shape depending on its flags.
#[test]
fn the_ast_phase_is_entered_even_with_no_dumps() {
    let mut rec = Recorder::default();
    oz2c::transpile_observed(&holder_src(), &oz2c::Options::default(), &mut rec)
        .expect("transpiles");
    assert!(rec.phases.contains(&Phase::AstIngest), "got:\n{:#?}", rec.phases);
    assert!(rec.dumps.is_empty(), "no dumps were supplied, so none should be reported");
}

/// Each dump is reported once, by its index in `Options::ast_json` and with
/// its own byte length.
///
/// The index is the contract a caller's label depends on: `main.rs` names
/// the file by looking up the same position in its own `--ast` list, and the
/// library never sees a path. If these ever drifted, every progress line
/// would name the wrong file -- which no other test would notice, because
/// the generated C would be unchanged.
#[test]
fn each_ast_dump_is_reported_by_index_and_size() {
    let first = r#"{"kind": "TranslationUnitDecl", "inner": [
        {"kind": "ObjCImplementationDecl", "name": "Holder", "inner": [
          {"kind": "ObjCIvarDecl", "name": "_count", "type": {"qualType": "int"}}
        ]}
      ]}"#;
    let second = r#"{"kind": "TranslationUnitDecl", "inner": [
        {"kind": "ObjCInterfaceDecl", "name": "Holder", "inner": [
          {"kind": "ObjCIvarDecl", "name": "_other", "type": {"qualType": "__strong id"}}
        ]}
      ]}"#;

    let mut rec = Recorder::default();
    let options = oz2c::Options {
        ast_json: vec![first.to_string(), second.to_string()],
        ..Default::default()
    };
    oz2c::transpile_observed(&holder_src(), &options, &mut rec).expect("transpiles");

    assert_eq!(rec.dumps, vec![(0, first.len()), (1, second.len())]);
}

/// The sequence stops at the pass that failed.
///
/// oz2c has no soft-diagnostic mode, so the first pass to produce a
/// diagnostic is the last one that runs. A report must not claim a later
/// phase was entered -- that would attribute time to work that never
/// happened.
#[test]
fn a_generics_refusal_no_longer_ends_the_sequence() {
    /* Two classes, one dynamically-dispatched selector, incompatible
     * return types -- rejected by `generics::check_program` (#290).
     *
     * **This assertion is reversed.** It read
     * `[Repair, Collect, AstIngest, Arc, Generics]` with the message
     * "generics rejected, so pools and emit must not be reported", and it
     * was right about the pipeline as it stood. #540 changed the pipeline:
     * a whole-program name check that returned before emit ran was hiding
     * unrelated per-site refusals -- a `@try` or a capture three classes
     * away -- so each rebuild revealed one layer. Generics and pools now
     * defer their diagnostics instead of gating on them, and the caller
     * fails on the union.
     *
     * The phases the sequence *still* stops at are asserted below, in
     * `a_collect_refusal_still_ends_the_sequence`. That half did not
     * change and must not: a `collect` diagnostic means a `Program` whose
     * `superclass` strings may not be keys in `classes`, and emit indexes
     * those directly (#205, #501). */
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
struct alpha { int a; };
struct beta { long b; };
@interface Alpha : OZObject
- (const struct alpha *)spec;
@end
@interface Beta : OZObject
- (const struct beta *)spec;
@end
@implementation Alpha
- (const struct alpha *)spec { return (const struct alpha *)0; }
@end
@implementation Beta
- (const struct beta *)spec { return (const struct beta *)0; }
@end
"
    );
    let mut rec = Recorder::default();
    let diags = match oz2c::transpile_observed(&src, &oz2c::Options::default(), &mut rec)
    {
        Err(diags) => diags,
        Ok(_) => panic!("a return-type collision must be rejected"),
    };
    assert!(!diags.is_empty());

    assert_eq!(
        rec.phases,
        vec![
            Phase::Repair,
            Phase::Collect,
            Phase::AstIngest,
            Phase::Arc,
            Phase::Generics,
            Phase::Pools,
            Phase::Emit
        ],
        "a generics refusal no longer ends the sequence: pools and emit still run so \
         their own diagnostics can be reported in the same build (#540)"
    );
}

/// Observing changes nothing.
///
/// The whole design rests on the observer being inert -- the library
/// reports boundaries and does no work for them. This is also what guards
/// the shared `front_end()` the two entry points now call: if observing
/// perturbed anything, it would show up here as a difference in output
/// rather than as a mysterious build failure later.
#[test]
fn observing_does_not_change_the_output() {
    let src = holder_src();
    let plain = oz2c::transpile(&src).expect("transpiles");

    let mut rec = Recorder::default();
    let observed = oz2c::transpile_observed(&src, &oz2c::Options::default(), &mut rec)
        .expect("transpiles");

    assert_eq!(plain.source_c, observed.source_c);
    assert_eq!(plain.companion_h, observed.companion_h);
    assert_eq!(plain.companion_c, observed.companion_c);
}

/// The half of the old assertion that did **not** change, and must not.
///
/// #540 stopped generics and pools from gating, so that emit's per-site
/// refusals reach the same build. The two gates above them stay hard, and
/// for a reason that is not stylistic: a `collect` diagnostic means the
/// `Program` may be inconsistent -- a `superclass` string that is not a key
/// in `classes` -- and emit indexes those directly, so it would panic with
/// no location rather than report anything (#205, #501).
///
/// So the sequence still ends at `Collect` here, and if a later change makes
/// it continue, this test is the one that should fail first.
#[test]
fn a_collect_refusal_still_ends_the_sequence() {
    /* A superclass this translation unit never declares -- refused by
     * `collect`, which is the gate that has to stay hard. Deliberately not
     * using `ozobject_src()`: the point is that `OZObject` is undeclared. */
    let src = "@interface Orphan : OZObject\n- (void)greet;\n@end\n\
               @implementation Orphan\n- (void)greet {\n}\n@end\n";
    let mut rec = Recorder::default();
    let diags = match oz2c::transpile_observed(src, &oz2c::Options::default(), &mut rec) {
        Err(diags) => diags,
        Ok(_) => panic!("an undeclared superclass must be rejected"),
    };
    assert!(!diags.is_empty());
    assert_eq!(
        rec.phases,
        vec![Phase::Repair, Phase::Collect],
        "a collect refusal still ends the sequence -- the Program is not safe for \
         later passes to walk (#205, #501)"
    );
}
