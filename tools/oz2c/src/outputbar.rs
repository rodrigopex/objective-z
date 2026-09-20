// SPDX-License-Identifier: Apache-2.0
//
// outputbar.rs - the generated output is C.
//
// `staticbar` asks whether the *input* is in the subset. This asks the
// dual question about the *output*, and it exists because the two are not
// the same claim: `emit::walk_top_level` has a named arm per construct it
// knows and copies everything else through as the author's bytes.
//
// That passthrough is the product -- it is what carries `ZBUS_CHAN_DECLARE`,
// `K_TIMER_DEFINE` and an unexpanded `#define` into the output intact. It is
// also, unavoidably, how an Objective-C construct nobody has named reaches
// GCC as `stray '@' in program`, pointing into a generated file the author
// never wrote, with oz2c exiting 0.
//
// One issue per keyword was the pattern that followed: #563 (`@encode`,
// `@throw`, `@available`), #573 (an `@interface` inside `#ifdef`), #574
// (`@dynamic`), and `@protocol Foo;` which #582 predicted and nobody had
// filed -- it produced `expected identifier or '('` on a generated line.
// #582's instruction is to stop filing them and gate the property instead.
//
// So this runs on every transpile and is a hard error, not a test-only
// check. The corpora do not contain these shapes (that is precisely why
// the property was ungated), so a test over them would assert nothing
// about the next keyword. A build-time refusal reaches every consumer,
// including out-of-tree ones, and turns `stray '@'` on generated line 383
// into a located oz2c diagnostic naming the construct.
//
// It cannot refuse a program that previously worked: every shape it
// catches is one the C compiler was already going to reject. The only
// change is which tool reports it, and how well.

use std::ops::Range;

use tree_sitter::Node;

use crate::model::Diagnostic;

/// Node kinds that are Objective-C and have no C spelling, so finding one
/// in generated output means a construct was copied through instead of
/// lowered or refused.
///
/// **Not simply "every ObjC kind in the grammar."** Three families are
/// deliberately absent, and each absence is a fact about this backend
/// rather than an oversight:
///
/// * **Blocks** (`block_literal`, `block_pointer_declarator`,
///   `abstract_block_pointer_declarator`). A Clang C extension, not
///   Objective-C, and `include/platform` plus the PAL headers are entitled
///   to use them. oz2c lowers the ones it owns -- a literal to a hoisted
///   function, a declarator to a function pointer (#272) -- but a `^` that
///   survives from a spliced system header is not this gate's business.
/// * **Lightweight generics** (`generic_arguments`,
///   `parameterized_arguments`). `_Generic` is C11 and the grammar reuses
///   these for both, so a C translation unit can legitimately hold them.
/// * **`string_literal`**. The grammar gives `@"x"` and `"x"` the same
///   kind, so including it would flag every string in every output.
///
/// The list is therefore the kinds whose *presence is unambiguous*. It was
/// validated in both directions rather than reasoned about alone: zero
/// findings across all 145 transpilable sources in the tree, and a
/// confirmed finding on each shape the four issues above describe. A list
/// that only ever answers "no" is the failure mode this file is meant to
/// prevent, not an example of it.
pub const OBJC_ONLY_KINDS: &[&str] = &[
    /* Declarations */
    "class_interface",
    "class_implementation",
    "class_declaration",
    "implementation_definition",
    "protocol_declaration",
    "protocol_forward_declaration",
    "protocol_reference_list",
    "protocol_qualifier",
    "qualified_protocol_interface_declaration",
    "compatibility_alias_declaration",
    "module_import",
    /* Members */
    "instance_variables",
    "instance_variable",
    "visibility_specification",
    "property_declaration",
    "property_attributes_declaration",
    "property_attribute",
    "property_implementation",
    "method_declaration",
    "method_definition",
    "method_type",
    "method_parameter",
    "method_identifier",
    "keyword_declarator",
    /* Expressions */
    "message_expression",
    "selector_expression",
    "encode_expression",
    "available_expression",
    "at_expression",
    "atdef_field",
    "array_literal",
    "dictionary_literal",
    "dictionary_pair",
    "objc_bridge",
    /* Statements */
    "synchronized_statement",
    "try_statement",
    "catch_clause",
    "finally_clause",
    "throw_statement",
];

/// One unlowered construct found in generated output.
pub struct Unlowered {
    pub kind: &'static str,
    pub span: Range<usize>,
}

/// Every unlowered Objective-C construct in `generated`, outermost first.
///
/// Reparsed with the same grammar that produced the lowering rather than
/// scanned for `@`, because the two disagree on exactly the cases that
/// matter. Every generated class carries a banner comment holding the
/// original declaration:
///
/// ```text
/// /* =====================================
///  * @interface MT96Probe : OZObject
///  * ================================== */
/// ```
///
/// A grep for `@interface` matches that in all 30 output files of every
/// program. The parser calls it a `comment`, which is what it is. The same
/// argument covers a `@` inside a string literal.
///
/// Does not descend into a construct it has already reported: one
/// `@implementation` copied through holds a `method_definition` per method
/// and would otherwise report a dozen findings for one cause.
pub fn unlowered_objc(generated: &str) -> Vec<Unlowered> {
    let tree = crate::parse::parse(generated);
    let mut found = Vec::new();
    walk(tree.root_node(), &mut found);
    found
}

fn walk(node: Node, found: &mut Vec<Unlowered>) {
    if let Some(kind) = OBJC_ONLY_KINDS.iter().find(|k| **k == node.kind()) {
        found.push(Unlowered { kind, span: node.byte_range() });
        return;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children {
        walk(child, found);
    }
}

/// Refuse generated output that still holds Objective-C.
///
/// `label` names the artifact for the message -- the generated `.c`, the
/// companion header -- since by this point there is no source position to
/// point at: the offsets are into text oz2c wrote, not into anything on
/// disk. The construct is quoted instead, which is what lets an author
/// recognise what they wrote.
///
/// Phrased as an oz2c defect rather than an author error, because that is
/// what it is. Reaching here means a construct has no disposition: it was
/// neither lowered by `emit` nor refused by `staticbar`. The author's
/// program may well be valid Objective-C, and the actionable request is an
/// issue, so the message asks for one.
pub fn check_output_is_c(generated: &str, label: &str) -> Vec<Diagnostic> {
    unlowered_objc(generated)
        .into_iter()
        .map(|found| {
            let text = crate::emit::one_line(&generated[found.span.clone()]);
            let text: String = text.chars().take(72).collect();
            Diagnostic::new(
                format!(
                    "internal: an Objective-C '{}' was copied into the generated {} \
                     instead of being lowered or refused -- '{}'",
                    found.kind, label, text
                ),
                1,
                1,
            )
            .with_note(
                "this construct has no disposition in oz2c: `emit::walk_top_level` has no \
                 arm for it and `staticbar` does not refuse it, so it fell to the \
                 passthrough that carries Zephyr macros and `#define`s through intact. \
                 Left in place the C compiler would report `stray '@' in program` against \
                 a generated file, which is why this is caught here instead",
            )
            .with_help(
                "this is an oz2c gap rather than a mistake in the source -- please file it \
                 at https://github.com/rodrigopex/objective-z/issues with the construct \
                 above, so the kind gets an arm or a located refusal (#582)",
            )
        })
        .collect()
}
