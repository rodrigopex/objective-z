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
