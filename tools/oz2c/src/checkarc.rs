// SPDX-License-Identifier: Apache-2.0
//
// checkarc.rs - `oz2c --check-arc`: audit oz2c's ownership decisions
// against the marks Clang already wrote into the AST dump.
//
// Not a build gate, deliberately. The two models legitimately differ: Clang
// retains on binding and emits the redundant releases for an LLVM pass to
// delete, while `arc.rs` elides at source level because nothing downstream
// of oz2c will ever do that (docs/STATUS.md, "The hybrid model"). A
// positional diff of *releases* would therefore report a discrepancy at
// every elided site, which is every site. What this compares instead is the
// two things both models answer independently:
//
//   - **ivar ownership**, where Clang's qualifier and oz2c's fallback rule
//     are two verdicts on one question. This is a mechanical diff.
//   - **which syntactic position** each transfer sits in, against the
//     positions oz2c has a handler for. A position Clang marks and oz2c
//     declares no handler for is the work queue this tool exists to
//     produce.
//
// And it states what it cannot answer, because a reader who over-trusts it
// will conclude #361 is answerable when it is not.

use crate::astinfo::{ArcMark, AstFacts, QualifierScope};
use crate::model::Program;
use std::collections::BTreeMap;

/// The positions oz2c asks the ownership question in, and the function
/// that asks it.
///
/// A declared list, and the tool says so in its own output -- this is the
/// one part of the audit that is not mechanical. Deriving it instead would
/// mean `emit.rs` recording each decision as it renders, which is 47 call
/// sites into `arc.rs` and a `&mut` threaded through every recursive
/// renderer. The list is small, reviewable, and the direction it fails in
/// is safe: a position missing from it is *reported*, never hidden.
const HANDLED_POSITIONS: &[(&str, &str)] = &[
    ("VarDecl", "arc::binds_ownership"),
    ("ParmVarDecl", "arc::owning_argument_value"),
    ("FieldDecl", "model::Program::owned_object_ivars"),
    ("ObjCIvarDecl", "model::Program::owned_object_ivars"),
    ("ReturnStmt", "arc::return_needs_retain / return_hands_back_ownership"),
    ("BinaryOperator", "arc::binds_ownership (store), arc::hoists_owning_operand"),
    ("CompoundAssignOperator", "arc::hoists_owning_operand"),
    ("ObjCMessageExpr", "arc::owning_argument_value / receiver_owning_value"),
    ("CallExpr", "arc::owning_argument_value"),
    ("CStyleCastExpr", "arc::value_behind_casts"),
    /* A `+1` whose value is dropped at statement level has no enclosing
     * expression to sit in, so Clang marks the consume against the
     * enclosing `CompoundStmt`. Found by running this audit over the
     * corpus: `[t copy];` in `discarded_owning_return.m` reported as
     * having no handler when `arc::discarded_owning_value` has handled it
     * since #322 -- the list was incomplete, which is the direction this
     * table is allowed to fail in. */
    ("CompoundStmt", "arc::discarded_owning_value (discarded +1, #322)"),
    ("ArraySubscriptExpr", "arc::binds_ownership (array store, #360)"),
    ("ObjCArrayLiteral", "arc::is_owning_expr (collection element, #449)"),
    ("ObjCDictionaryLiteral", "arc::is_owning_expr (collection element, #449)"),
    ("ObjCBoxedExpr", "arc::is_owning_expr"),
    ("ObjCForCollectionStmt", "arc::owned_local_live_at (loop escape, #433)"),
    ("ConditionalOperator", "arc::is_owning_expr"),
    ("InitListExpr", "arc::is_owning_expr"),
];

/// One line of the ivar table: the same ivar, as each model sees it.
struct IvarRow {
    class: String,
    ivar: String,
    /// `None` when the dump says nothing about this ivar -- which is not
    /// "not owned", and is reported as its own state rather than folded
    /// into one.
    clang: Option<bool>,
    fallback: bool,
    /// Did the dump cover this ivar's class at all?
    ///
    /// This is what separates the two reasons Clang can have no answer,
    /// and they call for opposite reactions. If the dump covered the class
    /// and not the ivar, **oz2c synthesized the ivar** -- a property
    /// backing store, or the root class's `oz_prop_lock`
    /// (`collect::resolve_properties`) -- and Clang could not possibly
    /// have an opinion, so there is nothing to chase. If the dump never
    /// saw the class, the dumps do not cover this program and every row
    /// for that class is uninformative.
    ///
    /// Found by running this audit: `OZObject.oz_prop_lock` reported as
    /// "no Clang answer" and sent a reader looking for an ivar that exists
    /// in no source file.
    class_in_dump: bool,
}

impl IvarRow {
    /// Is this a disagreement worth a reader's time?
    ///
    /// Only where Clang has an answer. An ivar Clang never saw is a gap in
    /// the dump, reported separately: calling it a disagreement would put
    /// every ivar of every class the dump does not cover into the queue.
    fn disagrees(&self) -> bool {
        matches!(self.clang, Some(owned) if owned != self.fallback)
    }

    /// An ivar oz2c added to the class itself, which no dump can describe.
    fn synthesized(&self) -> bool {
        self.clang.is_none() && self.class_in_dump
    }

    /// An ivar whose class the dumps do not cover -- the one state here
    /// that is a real gap rather than an explanation.
    fn uncovered(&self) -> bool {
        self.clang.is_none() && !self.class_in_dump
    }
}

/// Render the audit for one program against one set of dumps.
///
/// `suffix` narrows every table to the source under audit -- see
/// `AstFacts::marks_in` for why that matters: 45 of 152 marks over three
/// of this repo's corpora are in the SDK's own sources, all of one shape,
/// so an unfiltered report's largest row is boilerplate.
pub fn report(program: &Program, facts: &AstFacts, suffix: &str) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!("oz2c --check-arc: {}", suffix));
    out.push(String::new());

    out.extend(ivar_table(program, facts));
    out.push(String::new());
    out.extend(position_table(facts, suffix));
    out.push(String::new());
    out.extend(qualifier_table(facts, suffix));
    out.push(String::new());
    out.extend(limits());
    out
}

/// The mechanical half: every ivar, as Clang and as the fallback rule see
/// it.
fn ivar_table(program: &Program, facts: &AstFacts) -> Vec<String> {
    let mut rows = Vec::new();
    for class in &program.class_order {
        let Some(info) = program.classes.get(class) else {
            continue;
        };
        for (ivar, c_type) in &info.own_ivars {
            rows.push(IvarRow {
                class: class.clone(),
                ivar: ivar.clone(),
                clang: facts.is_owned_object_ivar(class, ivar),
                fallback: program.fallback_owns_object_ivar(class, ivar, c_type),
                class_in_dump: facts.knows_class(class),
            });
        }
    }

    let mut out = vec![
        "  ivar ownership -- a mechanical diff: both models answer this one".to_string(),
    ];
    if rows.is_empty() {
        out.push("    (this program declares no ivars)".to_string());
        return out;
    }
    for row in &rows {
        let clang = match row.clang {
            Some(true) => "owned",
            Some(false) => "unowned",
            None if row.synthesized() => "n/a",
            None => "class not in dump",
        };
        let fallback = if row.fallback { "owned" } else { "unowned" };
        let verdict = if row.disagrees() {
            "  <-- DISAGREE"
        } else if row.synthesized() {
            "  (synthesized by oz2c; in no source, so Clang cannot see it)"
        } else if row.uncovered() {
            "  <-- the dumps do not cover this class"
        } else {
            ""
        };
        /* One padded field, not two: padding the ivar alone let the class
         * name's length shift the columns row to row. */
        out.push(format!(
            "    {:<40} clang={:<18} fallback={:<8}{}",
            format!("{}.{}", row.class, row.ivar),
            clang,
            fallback,
            verdict
        ));
    }
    let disagreements = rows.iter().filter(|r| r.disagrees()).count();
    let uncovered = rows.iter().filter(|r| r.uncovered()).count();
    let synthesized = rows.iter().filter(|r| r.synthesized()).count();
    out.push(format!(
        "    {} ivar{}, {} disagreement{}, {} synthesized, {} in a class the dumps miss",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
        disagreements,
        if disagreements == 1 { "" } else { "s" },
        synthesized,
        uncovered
    ));
    /* Spelled out because the number is easy to read as reassurance. A
     * disagreement here is not a style question: the fallback skips every
     * `id`-typed ivar, and skipping one leaks the object it holds. */
    if disagreements > 0 {
        out.push(
            "    a disagreement is a leak or an over-release, not a preference: \
             Clang's answer is the one that ships (#299)"
                .to_string(),
        );
    }
    out
}

/// Every in-file mark, grouped by transfer and position, against the
/// handler oz2c declares for that position.
fn position_table(facts: &AstFacts, suffix: &str) -> Vec<String> {
    let marks = facts.marks_in(suffix);
    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for mark in &marks {
        *counts.entry((mark.kind.as_str(), mark.position.as_str())).or_default() += 1;
    }

    let mut out = vec![format!(
        "  ARC transfers by position -- {} in this file, {} elsewhere in the dumps",
        marks.len(),
        facts.arc_marks().len() - marks.len()
    )];
    if counts.is_empty() {
        out.push("    (Clang marked no transfer in this file)".to_string());
        return out;
    }
    let mut unhandled: Vec<(&str, &str, usize)> = Vec::new();
    for ((kind, position), count) in &counts {
        let handler = HANDLED_POSITIONS.iter().find(|(p, _)| p == position);
        match handler {
            Some((_, who)) => {
                out.push(format!("    {:<26} {:<24} {:>3}   {}", kind, position, count, who))
            }
            None => {
                out.push(format!(
                    "    {:<26} {:<24} {:>3}   <-- NO HANDLER DECLARED",
                    kind, position, count
                ));
                unhandled.push((kind, position, *count));
            }
        }
    }
    if unhandled.is_empty() {
        out.push(
            "    every position Clang marked here has a declared handler".to_string(),
        );
    } else {
        out.push(format!(
            "    {} position{} Clang asks the ownership question in and oz2c \
             declares no handler for -- this is the work queue",
            unhandled.len(),
            if unhandled.len() == 1 { "" } else { "s" }
        ));
        for (kind, position, count) in &unhandled {
            out.push(format!("      {} at {} ({} site(s))", kind, position, count));
        }
    }
    out
}

/// Every ownership qualifier Clang wrote in this file.
///
/// Two of the four are hard errors in accepted source (#448 refuses
/// `__weak` and `__autoreleasing` wherever they are written), so seeing one
/// here means the audit is reading a dump of source oz2c would not accept
/// -- worth saying out loud rather than listing quietly.
///
/// **But only when the qualifier is the declaration's own.** ARC infers
/// `__autoreleasing` on an indirect parameter, so
/// `+ (id)arrayWithObjects:(const id *)objects` -- which writes no
/// qualifier at all -- is dumped as `const __autoreleasing id *`. The first
/// version of this table read that as the declaration's own and labelled it
/// "oz2c refuses this (#448)", accusing the author of writing a qualifier
/// that `git grep` finds in no `.h`, `.m` or `.c` in this repo.
///
/// The per-file filter means the SDK's own three rows never reached this
/// report; what did was any audited file declaring an `id *` parameter
/// itself, which is exactly the shape those factories use. Pointee-scoped
/// qualifiers are listed under their own heading rather than dropped,
/// because a real `__weak id *` buffer is worth seeing -- and because an
/// inferred `__autoreleasing` is precisely where #448's refusal must *not*
/// fire, so listing it makes this a check on the refusal instead of a
/// duplicate of it.
fn qualifier_table(facts: &AstFacts, suffix: &str) -> Vec<String> {
    let quals = facts.quals_in(suffix);
    let (own, pointee): (Vec<&crate::astinfo::OwnershipQual>, Vec<&crate::astinfo::OwnershipQual>) =
        quals.iter().copied().partition(|q| q.scope == QualifierScope::Declaration);

    let mut out = vec![format!(
        "  ownership qualifiers -- {} on a declaration, {} on a pointee",
        own.len(),
        pointee.len()
    )];
    if quals.is_empty() {
        out.push("    (Clang wrote none in this file)".to_string());
        return out;
    }
    for qual in &own {
        let note = match qual.qualifier.as_str() {
            "__weak" | "__autoreleasing" => "  <-- oz2c refuses this (#448)",
            _ => "",
        };
        out.push(format!(
            "    {:<14} {:<20} {:<14} {}{}",
            qual.qualifier, qual.name, qual.decl_kind, qual.at, note
        ));
    }
    let refused = own
        .iter()
        .filter(|q| matches!(q.qualifier.as_str(), "__weak" | "__autoreleasing"))
        .count();
    out.push(format!(
        "    {} that oz2c refuses outright (`__weak`, `__autoreleasing`: #448)",
        refused
    ));
    if !pointee.is_empty() {
        out.push(
            "    on the pointee of a pointer-to-object, NOT written by the author --".to_string(),
        );
        out.push(
            "    ARC infers `__autoreleasing` on an indirect parameter, so `(const id *)x`"
                .to_string(),
        );
        out.push(
            "    dumps as `const __autoreleasing id *`. #448 does not apply to these."
                .to_string(),
        );
        for qual in &pointee {
            out.push(format!(
                "      {:<14} {:<20} {:<14} {}",
                qual.qualifier, qual.name, qual.decl_kind, qual.at
            ));
        }
    }
    out
}

/// What the oracle cannot answer, stated in the tool's own output.
///
/// Required by #453 and not padding: every claim below is measured, and a
/// reader who takes the tables above as complete will draw a conclusion the
/// dump does not support.
///
/// **This text corrects #453's own premise, which is why it is spelled out
/// rather than summarised.** The issue says a `+1` class send, a `+0` class
/// send and a protocol send are "marked *identically* --
/// `ARCReclaimReturnedObject` on all three". They are not: a *family* send
/// carries `ARCConsumeObject`, and what actually collapses together is a
/// **non-family** factory and a `+0` send. #361 is still unanswerable, for
/// a different reason than the issue gives -- and the difference matters,
/// because "the marks say nothing at a call site" would have made this
/// whole section of the audit look pointless when the marks are in fact a
/// redundancy check on the family rule. See `docs/STATUS.md`, "Where
/// tree-sitter and the Clang AST each sit".
fn limits() -> Vec<String> {
    vec![
        "  what this audit cannot tell you".to_string(),
        "    At a call site the marks discriminate create-rule *family membership*, and".to_string(),
        "    nothing finer. Measured against the pinned clang, one send per line:".to_string(),
        "      [Thing alloc]         family +1 class send       ARCConsumeObject".to_string(),
        "      [a copy]              family +1 instance send    ARCConsumeObject".to_string(),
        "      [Thing factoryThing]  NON-family +1 class send   ARCReclaimReturnedObject".to_string(),
        "      [a borrowed]          +0 instance send           ARCReclaimReturnedObject".to_string(),
        "      [s supply]            protocol send              ARCReclaimReturnedObject".to_string(),
        "    So an ARCConsumeObject at a call site is a redundancy check on the family".to_string(),
        "    rule `arc::create_rule_family_of` already computes from the selector -- and".to_string(),
        "    computes with a return-type guard Clang lacks (docs/ARC.md s 3.1). What the".to_string(),
        "    marks cannot separate is the last three rows from each other: a non-family".to_string(),
        "    factory looks exactly like a +0 send. That is #361's question, so #361 stays".to_string(),
        "    unanswerable from this dump.".to_string(),
        String::new(),
        "    On the callee side the marks say less still. A method's object return carries".to_string(),
        "    ARCProduceObject whether it returns `[Thing alloc]`, a borrowed ivar, or its".to_string(),
        "    own parameter; and `newThing`, `copyWithZone:` and `makeThing` each carry one".to_string(),
        "    ARCProduceObject and one ARCConsumeObject identically, though only the first".to_string(),
        "    two return +1. The position is the discriminator there, not the mark.".to_string(),
        String::new(),
        "    The position-to-handler column is a declared list in `checkarc.rs`, not a".to_string(),
        "    reading of `emit.rs`. A position missing from it is reported, never hidden --".to_string(),
        "    but a handler listed there that has since stopped being called would still".to_string(),
        "    read as covered.".to_string(),
    ]
}

/// The mark kinds and positions this audit knows how to talk about, for a
/// caller that wants the raw rows rather than the rendered report.
pub fn unhandled_positions<'a>(facts: &'a AstFacts, suffix: &str) -> Vec<&'a ArcMark> {
    facts
        .marks_in(suffix)
        .into_iter()
        .filter(|m| !HANDLED_POSITIONS.iter().any(|(p, _)| *p == m.position))
        .collect()
}
