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
