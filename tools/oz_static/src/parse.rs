// SPDX-License-Identifier: Apache-2.0
//
// parse.rs - tree-sitter-objc parsing wrapper.

use tree_sitter::{Parser, Tree};

pub fn parse(source: &str) -> Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_objc::LANGUAGE.into())
        .expect("failed to load the Objective-C grammar");
    parser.parse(source, None).expect("tree-sitter parse returned None")
}

/// (line, col) 1-based, for diagnostics.
pub fn line_col(source: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, b) in source.as_bytes().iter().enumerate() {
        if i == byte_offset {
            break;
        }
        if *b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Terminate a top-level function-like macro invocation written without a
/// trailing `;`, so it stops absorbing the construct that follows it.
///
/// `ZBUS_OBS_DECLARE(x)` is spelled without a semicolon -- that is Zephyr's
/// own idiom, what `oz_sdk/Foundation/OZMacro.h` documents, and what
/// `samples/zbus_service/src/main.m` writes. To tree-sitter it then reads as
/// a *type*, so the next construct becomes the declarator of a function
/// definition and everything up to the following `{ ... }` is swallowed
/// into one node:
///
/// ```text
/// function_definition
///   macro_type_specifier   ZBUS_OBS_DECLARE(lis_led_status)
///   function_declarator    ZBUS_CHAN_ADD_OBS(chan, lis_led_status, 3);
///   compound_statement     <- the absorbed @implementation's first method body
/// ```
///
/// The absorbed node never reaches its own arm in `emit::walk_top_level`,
/// and what it was decides the symptom: an `@implementation` emitted
/// verbatim (#288), a second `OZM`'s block literal surviving at its call
/// site (#289), or a `static Foo *p;` reaching the C compiler untagged
/// (OZ-004, #37). One cause, and the victim is simply whatever came next.
///
/// The repair is **offset-preserving**: the whole emitter is keyed on byte
/// offsets into this text, and `origins`/`header_ranges` are ranges over
/// it, so a repair that inserted a byte would shift every span past it. A
/// single whitespace byte after the macro's `)` is overwritten with `;`
/// instead, which changes no length and no offset. The `;` then arrives at
/// `walk_top_level` as a lone top-level `;` node, which it already drops
/// rather than copying -- so the generated C gains nothing, not even the
/// `;;` that would cost an ISO C diagnostic.
///
/// Repeats until the tree stops changing: one file commonly has several of
/// these, and each repair is what reveals the next.
///
/// Returns the repaired text and the offsets written to, because the `;` is
/// for the *parse* only and must not reach the output: `ZBUS_OBS_DECLARE`
/// terminates its own expansion, so a second `;` would leave a stray one at
/// file scope -- `-Wextra-semi`, and an ISO C violation `just test-pedantic`
/// gates on. `emit::walk_top_level` writes a space back over each offset
/// when it copies the text through.
///
/// The narrow case this cannot serve is a genuine macro *return type* whose
/// declarator is on a later line -- `MY_RESULT(int)\nfoo(void) { ... }`.
/// That is why a newline is required before the declarator rather than any
/// whitespace: a real one-line `MY_RESULT(int) foo(void)` is untouched.
pub fn repair_bare_macro_statements(source: &str) -> (String, Vec<usize>) {
    /* One pass per repair, since fixing the first changes the parse of
     * everything after it. The cap is a safety net against a shape that
     * somehow reports a repair without making progress, not an expected
     * limit -- the loop normally ends because no candidate is left. */
    const MAX_PASSES: usize = 64;

    let mut text = source.to_string();
    let mut inserted = Vec::new();
    for _ in 0..MAX_PASSES {
        let Some(offset) = first_bare_macro_semicolon_slot(&text) else {
            return (text, inserted);
        };
        inserted.push(offset);
        /* Safe: `first_bare_macro_semicolon_slot` only returns the offset
         * of an ASCII whitespace byte, so this neither splits nor grows a
         * UTF-8 sequence. */
        unsafe {
            text.as_bytes_mut()[offset] = b';';
        }
    }
    (text, inserted)
}

/// Offset of the whitespace byte to overwrite with `;`, for the first
/// unterminated top-level macro invocation in `source`.
fn first_bare_macro_semicolon_slot(source: &str) -> Option<usize> {
    let tree = parse(source);
    let root = tree.root_node();
    let bytes = source.as_bytes();

    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        /* Which node kind absorbs the neighbour depends only on whether a
         * `{ ... }` follows: with one, tree-sitter builds a
         * `function_definition` and takes the braces for the body; with
         * none it settles for a `declaration` whose declarator ran long.
         * Both start with the unterminated macro read as the type. */
        if node.kind() != "function_definition" && node.kind() != "declaration" {
            continue;
        }
        let mut inner = node.walk();
        let children: Vec<tree_sitter::Node> = node.children(&mut inner).collect();
        let Some(macro_type) = children.first() else {
            continue;
        };
        if macro_type.kind() != "macro_type_specifier" {
            continue;
        }
        let end = macro_type.end_byte();
        if end >= bytes.len() || !bytes[end].is_ascii_whitespace() {
            /* Nowhere to put the `;` without shifting offsets. Left alone
             * deliberately: the construct after it is still absorbed, and
             * the C compiler's complaint about generated code is worse than
             * silence -- but inventing a byte here would corrupt every
             * offset in the file, which is worse than both. */
            continue;
        }
        /* A genuine macro return type keeps its declarator on the same
         * line; this bug always has a line break, because the two lines
         * were written as separate statements. */
        let gap_has_newline = source[end..]
            .bytes()
            .take_while(|b| b.is_ascii_whitespace())
            .any(|b| b == b'\n');
        if !gap_has_newline {
            continue;
        }
        return Some(end);
    }
    None
}
