// SPDX-License-Identifier: Apache-2.0
//
// objc_node_disposition.rs -- every Objective-C node kind is lowered or
// refused, and the generated output is C (#582, #574).
//
// `emit::walk_top_level` has a named arm per construct it knows and copies
// everything else through as the author's bytes. That passthrough is the
// product -- it carries `ZBUS_CHAN_DECLARE`, `K_TIMER_DEFINE` and an
// unexpanded `#define` into the output intact -- and it is also how an
// Objective-C construct nobody named reaches GCC as `stray '@' in
// program`, in a generated file, with oz2c exiting 0.
//
// One issue per keyword was the pattern: #563 (`@encode`, `@throw`,
// `@available`), #573 (an `@interface` inside `#ifdef`), #574 (`@dynamic`).
// #582 says stop, and gate the property.
//
// **The gate found three more on its first run**, which is the evidence it
// is worth having:
//
//   * `@protocol Foo;` -- `protocol_forward_declaration` had no arm. GCC:
//     `expected identifier or '('`. #582 predicted exactly this ("any
//     future tree-sitter-objc node this repo has not named") and nobody
//     had filed it. Now lowered to a comment, the analogue of `@class`.
//   * `struct S { @defs(D) };` -- `atdef_field`. `@defs` was *already*
//     refused, but only in its `at_expression` spelling, and the leaking
//     one is the idiomatic position. The comment on `is_defs_shape`
//     asserted the grammar "has no `@defs` rule at all"; it has two.
//   * `@import Foundation;` -- `module_import`, which `staticbar` argued
//     was deliberately Clang's to refuse. Clang does refuse it, but a
//     source declaring no class never gets an AST dump
//     (`check_ast_present` returns early on an empty class table) and
//     `--allow-missing-ast` skips it. Now oz2c's, with a message naming
//     `#import`.
//
// Two things this file holds, and they are different claims:
//
//   1. `outputbar` detects unlowered Objective-C -- asserted in *both*
//      directions, because a detector that only ever answers "no" is the
//      failure mode, not the fix.
//   2. Every named kind in the grammar has a classification, pinned to the
//      grammar's own count so a `tree-sitter-objc` bump cannot quietly add
//      an unclassified kind. "Unknown" is what #582 rules out.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// Every named node kind `tree-sitter-objc` defines, as of 3.0.2.
///
/// Pinned deliberately. The point of the table below is completeness
/// against the grammar, and a grammar that grew a kind without anyone
/// classifying it is the hole #582 describes -- so the count is asserted
/// and a bump fails this test until the new kind is placed.
const GRAMMAR_NAMED_KIND_COUNT: usize = 191;

/// ObjC-only kinds the output gate looks for. Must equal
/// `outputbar::OBJC_ONLY_KINDS` exactly -- the table is the record, the
/// const is the mechanism, and a test that lets them drift is worth
/// nothing.
const GATED: &[&str] = &[
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
    "synchronized_statement",
    "try_statement",
    "catch_clause",
    "finally_clause",
    "throw_statement",
];

/// ObjC-only, but reachable *only* inside a gated parent, so the gate
/// catches the parent and never needs to name these. `available_expression`
/// owns all of them (`@available(macos 10, *)` -> platform, version).
/// `walk` returns on its first match rather than descending, which is what
/// makes one cause one finding.
const OBJC_ONLY_VIA_PARENT: &[&str] = &[
    "availability",
    "availability_attribute_specifier",
    "platform",
    "version",
    "version_number",
];

/// Clang C extensions and one shared spelling -- documented exemptions,
/// each a fact about this backend rather than an oversight. See
/// `outputbar::OBJC_ONLY_KINDS`' doc comment for why each is out.
const CLANG_EXTENSION_OR_SHARED: &[&str] = &[
    "block_literal",
    "block_pointer_declarator",
    "abstract_block_pointer_declarator",
    "generic_arguments",
    "parameterized_arguments",
    "string_literal",
];

/// Grammar supertypes and hidden rules. These never appear as a concrete
/// node kind on a real tree, so a disposition would be unobservable.
const GRAMMAR_INTERNAL: &[&str] = &[
    "_abstract_declarator",
    "_declarator",
    "_field_declarator",
    "_type_declarator",
    "expression",
    "statement",
    "type_specifier",
    "specifier_qualifier",
    "declaration_list",
];

/// Plain C and preprocessor kinds. These *are* the product: they ride the
/// passthrough into the output on purpose, which is how a Zephyr macro and
/// an unexpanded `#define` survive.
const C_AND_PREPROCESSOR: &[&str] = &[
    "abstract_array_declarator",
    "abstract_function_declarator",
    "abstract_parenthesized_declarator",
    "abstract_pointer_declarator",
    "alignas_qualifier",
    "alignof_expression",
    "argument_list",
    "array_declarator",
    "array_type_specifier",
    "assignment_expression",
    "atomic_declaration",
    "attribute",
    "attribute_declaration",
    "attribute_specifier",
    "attributed_declarator",
    "attributed_statement",
    "binary_expression",
    "bitfield_clause",
    "break_statement",
    "call_expression",
    "case_statement",
    "cast_expression",
    "char_literal",
    "character",
    "comma_expression",
    "comment",
    "compound_literal_expression",
    "compound_statement",
    "concatenated_string",
    "conditional_expression",
    "continue_statement",
    "declaration",
    "do_statement",
    "else_clause",
    "enum_specifier",
    "enumerator",
    "enumerator_list",
    "escape_sequence",
    "expression_statement",
    "extension_expression",
    "false",
    "field_declaration",
    "field_declaration_list",
    "field_designator",
    "field_expression",
    "field_identifier",
    "for_statement",
    "function_declarator",
    "function_definition",
    "generic_expression",
    "generic_specifier",
    "gnu_asm_clobber_list",
    "gnu_asm_expression",
    "gnu_asm_goto_list",
    "gnu_asm_input_operand",
    "gnu_asm_input_operand_list",
    "gnu_asm_output_operand",
    "gnu_asm_output_operand_list",
    "gnu_asm_qualifier",
    "goto_statement",
    "identifier",
    "if_statement",
    "init_declarator",
    "initializer_list",
    "initializer_pair",
    "labeled_statement",
    "linkage_specification",
    "macro_type_specifier",
    "ms_asm_block",
    "ms_based_modifier",
    "ms_call_modifier",
    "ms_declspec_modifier",
    "ms_pointer_modifier",
    "ms_restrict_modifier",
    "ms_signed_ptr_modifier",
    "ms_unaligned_ptr_modifier",
    "ms_unsigned_ptr_modifier",
    "null",
    "number_literal",
    "offsetof_expression",
    "parameter_declaration",
    "parameter_list",
    "parenthesized_declarator",
    "parenthesized_expression",
    "pointer_declarator",
    "pointer_expression",
    "preproc_arg",
    "preproc_call",
    "preproc_def",
    "preproc_defined",
    "preproc_directive",
    "preproc_elif",
    "preproc_elifdef",
    "preproc_else",
    "preproc_function_def",
    "preproc_if",
    "preproc_ifdef",
    "preproc_include",
    "preproc_linemarker",
    "preproc_params",
    "preproc_undef",
    "primitive_type",
    "range_expression",
    "return_statement",
    "sized_type_specifier",
    "sizeof_expression",
    "statement_identifier",
    "storage_class_specifier",
    "string_content",
    "struct_declaration",
    "struct_declarator",
    "struct_specifier",
    "subscript_designator",
    "subscript_expression",
    "subscript_range_designator",
    "switch_statement",
    "system_lib_string",
    "translation_unit",
    "true",
    "type_definition",
    "type_descriptor",
    "type_identifier",
    "type_name",
    "type_qualifier",
    "typedefed_specifier",
    "typeof_specifier",
    "unary_expression",
    "union_specifier",
    "update_expression",
    "va_arg_expression",
    "variadic_parameter",
    "while_statement",
];


/// The classification covers the grammar exactly -- no kind in two
/// buckets, none missing, and the total pinned.
///
/// This is the assertion that makes "every ObjC node kind has a
/// disposition" a checked claim rather than a belief. A `tree-sitter-objc`
/// upgrade that adds a node kind fails here, and whoever does the upgrade
/// has to say which bucket it belongs in.
#[test]
fn every_grammar_kind_is_classified() {
    let buckets: [(&str, &[&str]); 5] = [
        ("GATED", GATED),
        ("OBJC_ONLY_VIA_PARENT", OBJC_ONLY_VIA_PARENT),
        ("CLANG_EXTENSION_OR_SHARED", CLANG_EXTENSION_OR_SHARED),
        ("GRAMMAR_INTERNAL", GRAMMAR_INTERNAL),
        ("C_AND_PREPROCESSOR", C_AND_PREPROCESSOR),
    ];

    let mut seen: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for (name, kinds) in buckets {
        for k in kinds {
            if let Some(first) = seen.insert(k, name) {
                panic!("'{}' is classified twice: {} and {}", k, first, name);
            }
        }
    }

    assert_eq!(
        seen.len(),
        GRAMMAR_NAMED_KIND_COUNT,
        "the classification covers {} kinds but the grammar has {}. If tree-sitter-objc \
         was upgraded, put each new kind in a bucket in this file (and, if it is \
         Objective-C with no C spelling, in outputbar::OBJC_ONLY_KINDS) rather than \
         adjusting the count -- an unclassified kind is exactly the hole #582 closes",
        seen.len(),
        GRAMMAR_NAMED_KIND_COUNT
    );
}

/// The table's gated set and the gate's own list are the same set.
///
/// Without this they are two records of one fact, free to disagree, and
/// the table would document a gate that does something else.
#[test]
fn the_table_and_the_gate_agree() {
    let mut table: Vec<&str> = GATED.to_vec();
    let mut gate: Vec<&str> = oz2c::outputbar::OBJC_ONLY_KINDS.to_vec();
    table.sort_unstable();
    gate.sort_unstable();
    assert_eq!(table, gate, "GATED and outputbar::OBJC_ONLY_KINDS have drifted apart");
}

/// **The presence half.** The detector finds Objective-C in text that
/// holds it.
///
/// Written against a hand-made string rather than a transpile, because
/// every construct that used to leak is now lowered or refused -- so
/// nothing oz2c can currently produce would exercise it, and a test that
/// cannot fail is not a test. This is the shape #573 emitted verbatim.
#[test]
fn the_gate_detects_objc_in_output() {
    let not_c = "\
/* Auto-generated by oz2c -- do not edit */
#include \"oz2c_dispatch.h\"

@interface Leaked : OZObject
- (int)run;
@end
";
    let found = oz2c::outputbar::unlowered_objc(not_c);
    assert!(!found.is_empty(), "an @interface in generated C must be detected");
    assert_eq!(found[0].kind, "class_interface");

    let diags = oz2c::outputbar::check_output_is_c(not_c, "source");
    assert_eq!(diags.len(), 1, "one construct, one diagnostic");
    assert!(
        diags[0].message.contains("class_interface") && diags[0].message.contains("source"),
        "the refusal should name the kind and the artifact: {}",
        diags[0].message
    );
    assert!(
        diags[0].help.iter().any(|h| h.contains("issues")),
        "reaching the gate is an oz2c gap, so the remedy is to file it: {:?}",
        diags[0].help
    );
}

/// One cause, one finding: a copied-through `@implementation` holds a
/// `method_definition` per method, and reporting each would bury the
/// cause under its own children.
#[test]
fn the_gate_reports_the_outermost_construct_only() {
    let not_c = "\
@implementation Leaked
- (int)a { return 1; }
- (int)b { return 2; }
@end
";
    let found = oz2c::outputbar::unlowered_objc(not_c);
    assert_eq!(found.len(), 1, "expected one finding, got {:?}",
        found.iter().map(|f| f.kind).collect::<Vec<_>>());
    assert_eq!(found[0].kind, "class_implementation");
}

/// **The absence half, and the one that would break first.** A generated
/// class carries a banner comment holding its original declaration, so a
/// grep for `@interface` matches every output file of every program. The
/// parser calls it a `comment`.
#[test]
fn a_banner_comment_is_not_a_finding() {
    let src = format!(
        "{}
@interface Banner : OZObject
- (int)run;
@end
@implementation Banner
- (int)run {{ return 1; }}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("an ordinary class transpiles");
    /* The banner really is there -- otherwise this proves nothing. */
    assert!(
        out.source_c.contains("@interface Banner : OZObject"),
        "expected the banner comment in the output"
    );
    for (label, text) in
        [("source", &out.source_c), ("companion_h", &out.companion_h), ("companion_c", &out.companion_c)]
    {
        let found = oz2c::outputbar::unlowered_objc(text);
        assert!(
            found.is_empty(),
            "{} false-positived on {:?}",
            label,
            found.iter().map(|f| f.kind).collect::<Vec<_>>()
        );
    }
}

/// #582's found instance: `@protocol Foo;` had no arm and was copied
/// through, so GCC met it in generated C.
///
/// Lowered rather than refused: `@class Foo;` becomes a comment plus
/// `struct Foo;` because a class is a C type, and a protocol never is --
/// it is compile-time only. So a comment, and no tag.
#[test]
fn a_forward_declared_protocol_is_lowered_to_a_comment() {
    let src = format!(
        "{}
@protocol PFwd;
@interface FwdUser : OZObject
- (int)run;
@end
@implementation FwdUser
- (int)run {{ return 1; }}
@end
",
        PREAMBLE()
    );
    let out = oz2c::transpile(&src).expect("a forward-declared protocol is accepted");
    let text = format!("{}{}", out.companion_h, out.source_c);
    assert!(
        text.contains("/* @protocol PFwd -- forward declaration, compile-time only */"),
        "expected the lowered comment:\n{}",
        text
    );
    /* No `struct PFwd;` -- a protocol is not a C type. */
    assert!(!text.contains("struct PFwd"), "a protocol needs no C tag:\n{}", text);
}

/// And it compiles. Before the arm existed this failed with
/// `expected identifier or '('` on a generated line, which is the whole
/// defect -- reading the output would not have shown it.
#[test]
fn a_forward_declared_protocol_compiles_and_runs() {
    let src = format!(
        "{}
@protocol PFwd2;
@interface FwdRun : OZObject
- (int)run;
@end
@implementation FwdRun
- (int)run {{ return 42; }}
@end

#include <stdio.h>

int main(void) {{
	FwdRun *f = [FwdRun alloc];
	printf(\"r=%d\\n\", [f run]);
	return 0;
}}
",
        PREAMBLE()
    );
    assert_eq!(compile_and_run(&src, "objc_disp_protocol_fwd").trim(), "r=42");
}

/// #574: `@dynamic` asks for accessors *not* to be synthesized. oz2c
/// commented the directive out and synthesized them anyway -- measured
/// before the fix, the program ran and printed the synthesized value, so
/// nothing would have reported it.
#[test]
fn dynamic_is_refused() {
    let src = format!(
        "{}
@interface MT95Probe : OZObject
@property int value;
- (int)run;
@end
@implementation MT95Probe
@dynamic value;
- (int)run {{ self.value = 95; return self.value; }}
@end
",
        PREAMBLE()
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("'@dynamic' is not in the static subset"),
        "expected the @dynamic refusal:\n{}",
        err
    );
    assert!(
        err.contains("@synthesize"),
        "the remedy is the directive that asks for what this backend does:\n{}",
        err
    );
}

/// The refusal is located, and told apart from `@synthesize` by the
/// directive token rather than by matching the node's text.
#[test]
fn the_dynamic_refusal_is_located() {
    let src = format!(
        "{}
@interface Loc : OZObject
@property int v;
@end
@implementation Loc
@dynamic v;
@end
",
        PREAMBLE()
    );
    match oz2c::transpile(&src) {
        Ok(_) => panic!("@dynamic must be refused"),
        Err(diags) => {
            let d = diags
                .iter()
                .find(|d| d.message.contains("'@dynamic'"))
                .expect("the @dynamic refusal should be present");
            assert!(d.span.is_some(), "the refusal needs a span");
            assert!(d.line > 1, "expected a real line, not the (1,1) fallback");
        }
    }
}

/// `@synthesize` is untouched, and is the remedy the refusal names. If
/// this breaks, the discriminator is keying on the wrong thing.
#[test]
fn synthesize_is_still_accepted() {
    let src = format!(
        "{}
@interface Syn : OZObject
@property int value;
- (int)run;
@end
@implementation Syn
@synthesize value;
- (int)run {{ self.value = 7; return self.value; }}
@end

#include <stdio.h>

int main(void) {{
	Syn *s = [Syn alloc];
	printf(\"v=%d\\n\", [s run]);
	return 0;
}}
",
        PREAMBLE()
    );
    assert_eq!(compile_and_run(&src, "objc_disp_synthesize").trim(), "v=7");
}

/// #582's second found instance: `@defs` in a struct field is
/// `atdef_field`, a different node kind from the `at_expression` spelling
/// that was already refused -- so the idiomatic position leaked while the
/// rare one was caught. Both now route through one constructor.
#[test]
fn both_defs_spellings_are_refused() {
    let struct_field = format!(
        "{}
@interface DefsC : OZObject {{ int _x; }}
@end
@implementation DefsC
@end
struct Wrapper {{ @defs(DefsC) }};
",
        PREAMBLE()
    );
    let err = expect_reject(&struct_field);
    assert!(
        err.contains("'@defs' is not in the static subset"),
        "the struct-field spelling must be refused too:\n{}",
        err
    );
}

/// #582's third: `@import`. `staticbar` argued this was Clang's to
/// refuse, and Clang does -- but not every source reaches Clang, so the
/// disposition is oz2c's now. The message names `#import`, which is what
/// this backend resolves.
#[test]
fn at_import_is_refused_with_the_header_form_as_the_remedy() {
    let src = format!(
        "{}
@import Foundation;
@interface ImpProbe : OZObject
- (int)run;
@end
@implementation ImpProbe
- (int)run {{ return 1; }}
@end
",
        PREAMBLE()
    );
    let err = expect_reject(&src);
    assert!(
        err.contains("'@import' is not in the static subset"),
        "expected the @import refusal:\n{}",
        err
    );
    assert!(err.contains("#import"), "the remedy is the header form:\n{}", err);
}

/// **The gate speaks only when oz2c thought it had succeeded.**
///
/// `emit` copies a construct through whether or not `staticbar` refused
/// it, so already-refused source reaches the output check too. Running it
/// there would hand the author an "internal, please file an issue" beside
/// the correct refusal of the very same `@try` -- noise at best, and at
/// worst an invitation to file an issue for a construct that is working as
/// designed.
///
/// `emitter_agreement.rs` caught this: the split emitter labels a finding
/// per origin file (`source for 'audit'`) and the single-file one cannot,
/// so the two refused *differently* and its equality assertion failed.
/// That test is the reason this property is stated here rather than
/// rediscovered.
#[test]
fn an_already_refused_construct_gets_one_diagnostic_not_two() {
    let src = format!(
        "{}
@interface Tried : OZObject {{ int _n; }}
- (void)run;
@end
@implementation Tried
- (void)run
{{
	@try {{ _n = 1; }} @catch (id e) {{ (void)e; }}
}}
@end
",
        PREAMBLE()
    );
    match oz2c::transpile(&src) {
        Ok(_) => panic!("@try must be refused"),
        Err(diags) => {
            assert!(
                diags.iter().any(|d| d.message.contains("@try/@catch is not supported")),
                "the real refusal must be present: {:?}",
                diags.iter().map(|d| &d.message).collect::<Vec<_>>()
            );
            assert!(
                !diags.iter().any(|d| d.message.contains("internal: an Objective-C")),
                "the output gate must stay silent when something was already refused: {:?}",
                diags.iter().map(|d| &d.message).collect::<Vec<_>>()
            );
        }
    }
}
