// SPDX-License-Identifier: Apache-2.0
//
// ast_facts.rs - the Clang AST as the authority on ivar ownership.
//
// tree-sitter gives oz_static syntax, not resolved types, so on its own it
// cannot tell whether `id _thing` is an object the class owns. Guessing
// either way is unsafe: releasing a non-object corrupts memory, and skipping
// every `id`-typed ivar silently leaks it. Clang already knows, and with
// `-fobjc-arc` writes the answer into each declaration's `qualType`, so
// `--ast` hands oz_static that answer (see `astinfo`).
//
// The AST JSON here is written by hand rather than produced by running
// clang. It is a faithful excerpt -- every `qualType` string below was
// copied from a real `clang -Xclang -ast-dump=json -fobjc-arc` run over this
// repo's own sources (see `astinfo::tests`) -- and keeping it inline means
// these tests neither need a clang on PATH nor care which one it is. The
// end-to-end path with a real dump is exercised by
// `tests/tools/cross_backend.py`, which dumps one per case anyway.

mod common;
use common::ozobject_src;

/// A program with three ivars covering the cases that matter: an owned
/// `id`, an unretained `id`, and a scalar.
fn source() -> String {
    format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Holder : OZObject {
	id _thing;
	__unsafe_unretained id _backref;
	int _count;
}
@end
@implementation Holder
@end
"
    )
}

/// Clang's own spelling for those three ivars under `-fobjc-arc`.
fn ast_json() -> &'static str {
    r#"{
      "kind": "TranslationUnitDecl",
      "inner": [
        {"kind": "ObjCInterfaceDecl", "name": "Holder", "inner": [
          {"kind": "ObjCIvarDecl", "name": "_thing", "type": {"qualType": "__strong id"}},
          {"kind": "ObjCIvarDecl", "name": "_backref",
           "type": {"qualType": "__unsafe_unretained id"}},
          {"kind": "ObjCIvarDecl", "name": "_count", "type": {"qualType": "int"}}
        ]}
      ]
    }"#
}

fn generated(options: &oz_static::Options) -> String {
    let out = oz_static::transpile_with_options(&source(), options)
        .unwrap_or_else(|d| panic!("{}", d.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")));
    format!("{}{}", out.source_c, out.companion_c)
}

/// Without an AST there is nothing to resolve `id` with, so the ivar is left
/// alone. That leaks, and leaking is the deliberate choice: the alternative
/// guess -- releasing anything spelled like a pointer -- would hand a
/// non-object to a release call.
#[test]
fn id_ivar_is_not_released_without_an_ast() {
    let all = generated(&oz_static::Options::default());
    assert!(
        !all.contains("Holder_oz_release_ivars"),
        "expected no release function for an unresolvable id ivar, got:\n{}",
        all
    );
}

/// With the AST, Clang's ownership qualifier decides: `__strong id` is
/// released, `__unsafe_unretained id` is not, and a scalar never was.
#[test]
fn ast_makes_owned_id_ivar_released_and_unretained_one_not() {
    let all = generated(&oz_static::Options {
        ast_json: vec![ast_json().to_string()],
        ..Default::default()
    });
    assert!(
        all.contains("void Holder_oz_release_ivars(struct Holder *self)\n{\n\toz_static_release((struct OZObject *)self->_thing);\n}"),
        "expected exactly the owned id ivar to be released, got:\n{}",
        all
    );
    assert!(
        !all.contains("self->_backref"),
        "an __unsafe_unretained ivar must never be released -- that is the double free \
         the qualifier exists to prevent:\n{}",
        all
    );
    assert!(!all.contains("self->_count"), "a scalar is not an object:\n{}", all);
}

/// A dump that is not JSON is a hard error, not a quiet fall-back to the
/// narrower built-in rule: the caller asked for Clang's answer, and
/// substituting a guess would change which ivars get released with no
/// indication why.
#[test]
fn malformed_ast_is_rejected() {
    let result = oz_static::transpile_with_options(
        &source(),
        &oz_static::Options { ast_json: vec!["not json at all".to_string()], ..Default::default() },
    )
    ;
    let Err(err) = result else { panic!("a malformed AST should be rejected") };
    let joined = err.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
    assert!(joined.contains("not valid Clang AST JSON"), "diagnostics: {}", joined);
}

/// A well-formed dump of the *wrong* thing -- valid JSON describing no ivars
/// -- is rejected too. Accepting it would silently behave exactly as if no
/// AST had been passed, which is the failure mode `--ast` exists to remove.
#[test]
fn ast_describing_no_ivars_is_rejected() {
    let result = oz_static::transpile_with_options(
        &source(),
        &oz_static::Options {
            ast_json: vec![r#"{"kind": "TranslationUnitDecl", "inner": []}"#.to_string()],
            ..Default::default()
        },
    )
    ;
    let Err(err) = result else { panic!("an AST with no ivars should be rejected") };
    let joined = err.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
    assert!(joined.contains("describe no ivars"), "diagnostics: {}", joined);
}

/// `--dump-ast-facts` prints every fact the dumps carry, and prints it the
/// same way every time.
///
/// This is the equivalence oracle for changing how the dumps are ingested
/// (#299). Ingest is 97.5% of oz2c's wall clock on px-keyboard, so it is
/// where the optimisation pressure is, and a mistake there does not fail a
/// build -- it makes the oracle answer one question fewer, which silently
/// leaks an ivar. So the property under test is not the wording of a line
/// but that the whole set is complete and byte-stable: `diff` over two runs
/// has to be a proof.
#[test]
fn dumped_facts_cover_every_set_and_are_sorted() {
    let facts = oz_static::astinfo::AstFacts::from_json(ast_json())
        .expect("the fixture is a faithful excerpt of a real dump");
    let lines = facts.dump_lines();

    assert_eq!(
        lines,
        vec![
            "class Holder".to_string(),
            "ivar Holder _backref unowned".to_string(),
            "ivar Holder _count unowned".to_string(),
            "ivar Holder _thing owned".to_string(),
        ],
        "every ivar, and the class itself, must appear -- a set dropped by a \
         refactor would be invisible in generated C until something read it \
         again"
    );

    let mut sorted = lines.clone();
    sorted.sort();
    assert_eq!(lines, sorted, "the output has to be sorted to be diffable");
}

/// The same facts reached through two dumps rather than one produce the
/// same lines.
///
/// `merge` unions each set (`astinfo::AstFacts::merge`), and that is what
/// makes the per-file dumps a partial view rather than a contradiction --
/// so the dump of a merged set must not depend on how many files it came
/// from, nor on the order they were merged in.
#[test]
fn dumped_facts_do_not_depend_on_how_the_dumps_were_split() {
    let one = r#"{"kind": "TranslationUnitDecl", "inner": [
        {"kind": "ObjCImplementationDecl", "name": "Holder", "inner": [
          {"kind": "ObjCIvarDecl", "name": "_thing", "type": {"qualType": "__strong id"}}
        ]}
      ]}"#;
    let two = r#"{"kind": "TranslationUnitDecl", "inner": [
        {"kind": "ObjCInterfaceDecl", "name": "Other", "inner": [
          {"kind": "ObjCMethodDecl", "name": "run", "inner": [{"kind": "CompoundStmt"}]}
        ]}
      ]}"#;

    let merge = |a: &str, b: &str| {
        let mut facts = oz_static::astinfo::AstFacts::from_json(a).expect("a");
        facts.merge(oz_static::astinfo::AstFacts::from_json(b).expect("b"));
        facts.dump_lines()
    };

    assert_eq!(
        merge(one, two),
        merge(two, one),
        "merge unions its sets, so the dump must not record the order"
    );
    let lines = merge(one, two);
    assert!(lines.contains(&"impl Holder".to_string()), "got:\n{:#?}", lines);
    assert!(lines.contains(&"method Other run".to_string()), "got:\n{:#?}", lines);
}

/// Concatenated top-level objects parse to the union of their facts.
///
/// `clang -Xclang -ast-dump-filter=NAME` emits one JSON object per matching
/// declaration, back to back, which is not a single document --
/// `serde_json::from_str` rejects it as "trailing characters". Reading the
/// input as a stream accepts both shapes, and a single document is a stream
/// of one, so this is a strict superset of what was accepted before (#299).
///
/// That matters because filtering is how these dumps stop being enormous:
/// on px-keyboard the filtered set is 29 MB against 742 MB unfiltered, for
/// a byte-identical fact set. Without this the filter cannot be used at
/// all.
#[test]
fn concatenated_top_level_objects_are_read_as_a_stream() {
    let concatenated = r#"{
      "kind": "ObjCInterfaceDecl", "name": "First",
      "inner": [
        {"kind": "ObjCIvarDecl", "name": "_a", "type": {"qualType": "__strong id"}}
      ]
    }
    {
      "kind": "ObjCImplementationDecl", "name": "Second",
      "inner": [
        {"kind": "ObjCIvarDecl", "name": "_b",
         "type": {"qualType": "__unsafe_unretained id"}}
      ]
    }"#;

    let facts = oz_static::astinfo::AstFacts::from_json(concatenated)
        .expect("concatenated dumps are what -ast-dump-filter produces");
    assert_eq!(
        facts.dump_lines(),
        vec![
            "class First".to_string(),
            "class Second".to_string(),
            "impl Second".to_string(),
            "ivar First _a owned".to_string(),
            "ivar Second _b unowned".to_string(),
        ]
    );
}

/// The fields Clang writes and the oracle ignores are skipped, not
/// mis-parsed.
///
/// A real dump carries `id`, `loc`, `range`, `mangledName`, `valueCategory`
/// and more on nearly every node. Deserializing into a narrow struct means
/// serde walks past them without allocating -- which is the whole point,
/// since materialising them as a `serde_json::Value` tree cost 1.30 GB
/// resident on px-keyboard. This pins that ignoring them does not change
/// what is read.
#[test]
fn unrelated_clang_fields_are_ignored() {
    let noisy = r#"{
      "id": "0x7f8b1", "kind": "TranslationUnitDecl",
      "loc": {"offset": 12, "file": "x.m", "line": 3, "col": 1},
      "range": {"begin": {"offset": 0}, "end": {"offset": 99}},
      "inner": [
        {"id": "0x7f8b2", "kind": "ObjCImplementationDecl", "name": "Noisy",
         "mangledName": "_OBJC_CLASS_Noisy", "valueCategory": "prvalue",
         "inner": [
           {"id": "0x7f8b3", "kind": "ObjCIvarDecl", "name": "_held",
            "loc": {"line": 4}, "isReferenced": true,
            "type": {"desugaredQualType": "id", "qualType": "__strong id",
                     "typeAliasDeclId": "0x7f8b9"},
            "access": "private", "bitwidth": 0}
         ]}
      ]
    }"#;

    let facts = oz_static::astinfo::AstFacts::from_json(noisy).expect("a realistic dump shape");
    assert_eq!(
        facts.dump_lines(),
        vec![
            "class Noisy".to_string(),
            "impl Noisy".to_string(),
            "ivar Noisy _held owned".to_string(),
        ],
        "the ignored fields must not change the facts -- note `qualType` is \
         read and the `desugaredQualType` beside it is not"
    );
}

/// An input with no top-level declaration is an error, not silently empty
/// facts.
///
/// Reading a stream makes this case reachable in a way one-document parsing
/// never was: an empty file is a valid stream of zero documents. Accepting
/// it would behave exactly as if no `--ast` had been passed, which is the
/// failure mode `--ast` exists to remove -- and it is precisely what a
/// truncated dump looks like, since Clang writes nothing at all when it
/// dies before `HandleTranslationUnit`.
#[test]
fn an_empty_dump_is_rejected() {
    assert!(oz_static::astinfo::AstFacts::from_json("").is_err());
    assert!(oz_static::astinfo::AstFacts::from_json("   \n\t ").is_err());
}

/// Trailing garbage after a valid document is still an error.
///
/// Reading a stream must not become a licence to accept anything: the
/// second item has to parse as a declaration too.
#[test]
fn trailing_garbage_after_a_document_is_still_rejected() {
    let bad = r#"{"kind": "ObjCInterfaceDecl", "name": "A"} this is not json"#;
    assert!(oz_static::astinfo::AstFacts::from_json(bad).is_err());
}

// ---------------------------------------------------------------------------
// `--ast` as a requirement rather than an option
// ---------------------------------------------------------------------------
//
// `Options::require_ast` is the build contract: a source that declares a
// class is not transpiled without Clang's answer about its ivars. The tests
// below are the ones that make it a contract instead of a comment -- one
// that it fires, one that it says something actionable when it does, and
// two that bound it, because a check that fires on the wrong input is worse
// than no check.

/// With `require_ast` and no dump, a class is refused.
#[test]
fn a_class_with_no_ast_is_refused_when_the_ast_is_required() {
    let options = oz_static::Options { require_ast: true, ..Default::default() };
    let diagnostics = oz_static::transpile_with_options(&source(), &options)
        .err()
        .expect("a class with no AST must be refused when the AST is required");
    let joined = diagnostics.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
    assert!(
        joined.contains("no Clang AST dump was supplied"),
        "the diagnostic must name the missing dump, got: {}",
        joined
    );
}

/// The refusal is *located*, and at the first class rather than at line 1.
///
/// oz_static's standing rule is a hard, located error; a bare "line 1" on a
/// source assembled from several spliced files points at nothing an author
/// can act on. `source()` puts the whole of `OZObject` ahead of `Holder`,
/// so a diagnostic that had defaulted to 1 would be visibly wrong here.
#[test]
fn the_refusal_points_at_the_first_class() {
    let options = oz_static::Options { require_ast: true, ..Default::default() };
    let diagnostics = oz_static::transpile_with_options(&source(), &options)
        .err()
        .expect("a class with no AST must be refused");
    let first_class_line = source()
        .lines()
        .position(|l| l.contains("@interface") || l.contains("@implementation"))
        .expect("the fixture declares a class")
        + 1;
    assert_eq!(
        diagnostics[0].line, first_class_line,
        "expected the diagnostic at the first class keyword, got {}",
        diagnostics[0]
    );
    assert!(diagnostics[0].line > 1, "the fixture's first class is not on line 1");
}

/// A dump satisfies the requirement -- the same source, unchanged, with
/// `ast_json` supplied.
#[test]
fn a_dump_satisfies_the_requirement() {
    let options = oz_static::Options {
        require_ast: true,
        ast_json: vec![ast_json().to_string()],
        ..Default::default()
    };
    assert!(
        oz_static::transpile_with_options(&source(), &options).is_ok(),
        "a supplied dump must satisfy require_ast"
    );
}

/// A source with no class needs no dump, even under `require_ast`.
///
/// Only a class has ivars, so only a class has an ownership question. A
/// pure-C translation unit has nothing for the oracle to answer and must
/// not be made to produce a dump that would say nothing.
#[test]
fn a_source_with_no_class_needs_no_dump() {
    let options = oz_static::Options { require_ast: true, ..Default::default() };
    let source = "#include <stdio.h>\nint main(void) { return 0; }\n";
    assert!(
        oz_static::transpile_with_options(source, &options).is_ok(),
        "a class-free source must transpile with no dump"
    );
}

/// Without `require_ast` the same class still transpiles -- the default that
/// `transpile(source)` and the ~500-case Rust suite are built on.
#[test]
fn the_requirement_is_off_by_default() {
    assert!(
        oz_static::transpile(&source()).is_ok(),
        "Options::default() must not require an AST"
    );
}
