// SPDX-License-Identifier: Apache-2.0
//
// diagnostic_locations.rs -- #456: a diagnostic names the file and line
// the defect was written at, not a position in the merged buffer.
//
// Every pass walks one `#import`-spliced buffer and raises diagnostics at
// offsets into it, so `Diagnostic.line` was a line in that buffer and
// nothing else -- for a 9-line `.m` with 33 spliced origins, line 1989.
// There is no file a reader can open to find it. `Diagnostic::resolve_in`
// maps the offset back through `imports::SourceMap`.
//
// These tests assert the **mapping**, never the text, and every expected
// line is read back off disk with `line_of` rather than hand-counted --
// the shape `tests/line_directives.rs` established for exactly this
// question.
//
// The two cases a naive implementation gets wrong are both about not
// counting lines locally:
//
//   - a defect in a file reached through an `#import`, where splicing has
//     moved the merged line far from the source line. A single-file case
//     passes whether or not the mapping works, so it would prove nothing;
//   - a defect in a file whose bare macro invocation
//     `parse::repair_bare_macro_statements` repairs. That repair
//     overwrites one ASCII whitespace byte in place, so it preserves
//     every byte offset while *eating a line* whenever the byte it
//     overwrites is a newline. Anything that carried a line instead of an
//     offset is quietly wrong here, and only here.

use std::fs;
use std::path::PathBuf;

use oz_static::imports::{resolve_entry_files, ResolvedSource};
use oz_static::Options;

/// The repository root, so the real `include/oz_sdk` and `src` resolve
/// however cargo was invoked -- the shape `tests/line_directives.rs:283`
/// uses. A bare relative path resolves against the crate directory and
/// not the repo, which is one directory too deep.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oz_static_diag_locations_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("inc")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

/// 1-based line of `text` holding `needle`, so an expected line is read
/// off disk and never written out here.
fn line_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line contains {:?}", needle))
        + 1
}

/// The sole diagnostic of a rejected resolution, resolved to a source
/// position the way `oz2c` resolves it before printing.
fn sole_resolved_diagnostic(resolved: &ResolvedSource) -> oz_static::model::Diagnostic {
    let options =
        Options { header_ranges: resolved.header_ranges.clone(), ..Default::default() };
    let diags = oz_static::transpile_split_with_options(
        &resolved.text,
        &resolved.origins,
        &options,
    )
    .map(|_| ())
    .expect_err("expected the resolution to be rejected");
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic, got {:?}", diags);
    let mut d = diags.into_iter().next().unwrap();
    d.resolve_in(&resolved.source_map);
    d
}

/// A `weak` property declared in an `#import`ed header is reported at that
/// header and that line -- the case the merged position cannot express,
/// since the header's text sits wherever splicing put it.
#[test]
fn a_rejection_inside_an_imported_header_names_the_header() {
    let dir = scratch_dir("imported_header");
    let header = "#import <Foundation/Foundation.h>\n\
                  \n\
                  @interface Keyboard : OZObject\n\
                  @property (nonatomic, weak) id delegate;\n\
                  - (void)press;\n\
                  @end\n";
    fs::write(dir.join("inc/Keyboard.h"), header).unwrap();
    fs::write(
        dir.join("src/Keyboard.m"),
        "#import \"Keyboard.h\"\n\
         \n\
         @implementation Keyboard\n\
         - (void)press\n\
         {\n\
         }\n\
         @end\n",
    )
    .unwrap();

    let entries = vec![dir.join("src/Keyboard.m")];
    let resolved = resolve_entry_files(
        &entries,
        &[dir.join("inc"), repo_root().join("include/oz_sdk")],
        &[dir.join("src"), repo_root().join("src")],
    )
    .unwrap_or_else(|e| panic!("resolution failed: {}", e));

    let d = sole_resolved_diagnostic(&resolved);
    assert!(
        d.message.contains("'weak' property 'delegate'"),
        "unexpected diagnostic: {}",
        d.message
    );
    assert_eq!(
        d.file.as_deref(),
        Some(dir.join("inc/Keyboard.h").as_path()),
        "diagnostic named the wrong file: {:?}",
        d.file
    );
    assert_eq!(
        d.line,
        line_of(header, "weak"),
        "diagnostic named the wrong line in {:?}",
        d.file
    );
}

/// The same question for a file the macro repair rewrites. The repair
/// eats the newline it overwrites, so the repaired buffer `collect` walks
/// has one line fewer than the file on disk from that point on -- and the
/// reported line must still be the file's.
#[test]
fn a_rejection_after_a_repaired_bare_macro_names_the_source_line() {
    let dir = scratch_dir("repaired_macro");
    let header = "#import <Foundation/Foundation.h>\n\
                  \n\
                  #define ANNOUNCE(x) OZLog(@\"%@\", x)\n\
                  \n\
                  @interface Pad : OZObject\n\
                  - (void)tap;\n\
                  @end\n";
    fs::write(dir.join("inc/Pad.h"), header).unwrap();
    /* The bare `ANNOUNCE(...)` with no trailing semicolon is what
     * `repair_bare_macro_statements` rewrites, and it sits *before* the
     * rejected property so the eaten line is between the file start and
     * the defect. */
    let impl_src = "#import \"Pad.h\"\n\
                    \n\
                    @interface Pad ()\n\
                    @property (nonatomic, weak) id observer;\n\
                    @end\n\
                    \n\
                    @implementation Pad\n\
                    - (void)tap\n\
                    {\n\
                    \tANNOUNCE(@\"tap\")\n\
                    }\n\
                    @end\n";
    fs::write(dir.join("src/Pad.m"), impl_src).unwrap();

    let entries = vec![dir.join("src/Pad.m")];
    let resolved = resolve_entry_files(
        &entries,
        &[dir.join("inc"), repo_root().join("include/oz_sdk")],
        &[dir.join("src"), repo_root().join("src")],
    )
    .unwrap_or_else(|e| panic!("resolution failed: {}", e));

    let d = sole_resolved_diagnostic(&resolved);
    assert!(
        d.message.contains("'weak' property 'observer'"),
        "unexpected diagnostic: {}",
        d.message
    );
    assert_eq!(
        d.file.as_deref(),
        Some(dir.join("src/Pad.m").as_path()),
        "diagnostic named the wrong file: {:?}",
        d.file
    );
    assert_eq!(
        d.line,
        line_of(impl_src, "weak"),
        "diagnostic named the wrong line in {:?}",
        d.file
    );
}

/// A diagnostic raised by a whole-program check with no node to blame
/// keeps the position it always had, and says so by carrying no file --
/// rather than being given a plausible-looking one. Three such checks
/// remain (`attach_ast`, an unknown `--pool-sizes` class, an unsizable
/// slab cycle); giving them real anchors is tracked separately.
#[test]
fn an_unanchored_diagnostic_reports_no_file() {
    let source = "@interface OZObject\n@end\n@implementation OZObject\n@end\n\
                  @interface Thing : OZObject\n@end\n@implementation Thing\n@end\n";
    let options = Options {
        pool_sizes: [("NoSuchClass".to_string(), 4usize)].into_iter().collect(),
        ..Default::default()
    };
    let diags = oz_static::transpile_with_options(source, &options)
        .map(|_| ())
        .expect_err("expected --pool-sizes on an unknown class to be rejected");
    let d = diags.iter().find(|d| d.message.contains("--pool-sizes names")).unwrap_or_else(
        || panic!("expected the unknown-override diagnostic, got {:?}", diags),
    );
    assert_eq!(d.offset, None, "this check has no node to anchor to");
    assert_eq!(d.file, None, "an unanchored diagnostic must not name a file");
}
