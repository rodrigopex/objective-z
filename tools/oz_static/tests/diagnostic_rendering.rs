// SPDX-License-Identifier: Apache-2.0
//
// diagnostic_rendering.rs -- #457: a diagnostic is printed the way a
// compiler prints one, so the defect in the author's code is pointed at
// rather than described.
//
// #456 made the position real (a file, a line, a column). This asserts
// the *rendering* of it: the `-->` header, the offending line, an
// underline under the construct, and the reason and remedies on their own
// lines.
//
// These assert structure and alignment, never whole-output equality. A
// golden-text test on a frame like this fails on every message reword and
// teaches nothing about whether the caret points at the right thing --
// which is the only property that matters and the only one a reader
// cannot check by eye.
//
// The alignment cases are the ones a naive renderer gets wrong:
//
//   - a **tab-indented** line. 58 of the 81 behaviour cases are
//     tab-indented, so this is the common case here, not an edge one. A
//     caret padded one space per byte lands four columns short of an
//     8-column tab.
//   - a span covering **more than one line**. Underlining a whole method
//     body buries the `help:` that says what to do, so it is clipped.

use std::fs;
use std::path::PathBuf;

use oz_static::imports::{resolve_entry_files, ResolvedSource};
use oz_static::model::Diagnostic;
use oz_static::render::render;
use oz_static::Options;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oz_static_diag_render_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("inc")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

/// Resolve a two-file program and render its sole diagnostic, exactly as
/// `oz2c` does: resolve the position through the map, then render.
fn render_sole_diagnostic(dir: &PathBuf, entry: &str) -> String {
    let resolved: ResolvedSource = resolve_entry_files(
        &[dir.join(entry)],
        &[dir.join("inc"), repo_root().join("include/oz_sdk")],
        &[dir.join("src"), repo_root().join("src")],
    )
    .unwrap_or_else(|e| panic!("resolution failed: {}", e));

    let options =
        Options { header_ranges: resolved.header_ranges.clone(), ..Default::default() };
    let diags = oz_static::transpile_split_with_options(
        &resolved.text,
        &resolved.origins,
        &options,
    )
    .map(|_| ())
    .expect_err("expected the program to be rejected");
    assert_eq!(diags.len(), 1, "expected one diagnostic, got {:?}", diags);

    let mut d = diags.into_iter().next().unwrap();
    d.resolve_in(&resolved.source_map);
    render(&d, &resolved.text)
}

/// The line of `rendered` holding `needle`.
fn line_with<'a>(rendered: &'a str, needle: &str) -> &'a str {
    rendered
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no rendered line contains {:?}\n{}", needle, rendered))
}

/// The underline row is the one made only of gutter, spaces and carets.
fn caret_line<'a>(rendered: &'a str) -> &'a str {
    rendered
        .lines()
        .find(|l| l.contains('^'))
        .unwrap_or_else(|| panic!("no caret row in:\n{}", rendered))
}

/// The whole frame, on a space-indented body: header, source line,
/// underline, and the tiers. The caret must sit under the construct and
/// be as wide as it.
#[test]
fn the_frame_points_at_the_offending_construct() {
    let dir = scratch_dir("frame");
    fs::write(
        dir.join("inc/Keyboard.h"),
        "#import <Foundation/Foundation.h>\n\n@interface Keyboard : OZObject\n\
         - (void)press;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Keyboard.m"),
        "#import \"Keyboard.h\"\n\n@implementation Keyboard\n- (void)press\n{\n\
         \x20       id obj = [[Keyboard alloc] init];\n\x20       [obj retain];\n}\n@end\n",
    )
    .unwrap();

    let out = render_sole_diagnostic(&dir, "src/Keyboard.m");

    assert!(out.starts_with("oz2c error: "), "first line must be the summary:\n{}", out);
    assert!(
        out.lines().next().unwrap().contains("'-retain' cannot be sent"),
        "the summary must stand alone -- oz_static_build.py reads only it:\n{}",
        out
    );

    /* The header names the real file, not the merged buffer. */
    let header = line_with(&out, "-->");
    assert!(header.contains("src/Keyboard.m:7:"), "header names the wrong place: {}", header);

    /* The caret sits under `[obj retain]` and is exactly as wide. */
    let code = line_with(&out, "[obj retain]");
    let caret = caret_line(&out);
    assert_eq!(
        code.find('[').unwrap(),
        caret.find('^').unwrap(),
        "caret is not under the construct:\n{}",
        out
    );
    assert_eq!(
        caret.matches('^').count(),
        "[obj retain]".len(),
        "underline is not the width of the construct:\n{}",
        out
    );

    /* The remedies are separate lines, not one fused sentence. */
    assert!(out.contains("\nnote: "), "no note tier:\n{}", out);
    assert_eq!(
        out.lines().filter(|l| l.starts_with("help: ")).count(),
        3,
        "the retain rejection carries three distinct remedies:\n{}",
        out
    );
    assert!(out.contains("__unsafe_unretained"), "the opt-out remedy is missing:\n{}", out);
    assert!(
        out.contains("oz_static_retain_count"),
        "the refcount-reading remedy is missing:\n{}",
        out
    );
}

/// The same, tab-indented. A tab is 8 columns (`.clang-format`), so a
/// renderer padding one space per byte puts the caret at column 1 instead
/// of 8 -- and 58 of the 81 behaviour cases are tab-indented.
#[test]
fn the_caret_is_aligned_in_display_columns_over_tabs() {
    let dir = scratch_dir("tabs");
    fs::write(
        dir.join("inc/Pad.h"),
        "#import <Foundation/Foundation.h>\n\n@interface Pad : OZObject\n- (void)tap;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Pad.m"),
        "#import \"Pad.h\"\n\n@implementation Pad\n- (void)tap\n{\n\
         \tid obj = [[Pad alloc] init];\n\t[obj retain];\n}\n@end\n",
    )
    .unwrap();

    let out = render_sole_diagnostic(&dir, "src/Pad.m");
    let code = line_with(&out, "[obj retain]");
    let caret = caret_line(&out);

    assert!(!code.contains('\t'), "the shown line must have tabs expanded:\n{}", out);
    assert_eq!(
        code.find('[').unwrap(),
        caret.find('^').unwrap(),
        "caret misaligned over a tab -- the whole point of this case:\n{}",
        out
    );
}

/// A span wider than its first line is clipped, with a marker, so a
/// rejection covering a long construct cannot bury the `help:` beneath
/// it.
#[test]
fn a_multi_line_span_is_clipped_to_the_first_line() {
    let src = "one two three\nfour five six\nseven\n";
    /* A span from the first line running into the third. */
    let mut d = Diagnostic::spanning("spans three lines", src, 4..src.len() - 1)
        .with_help("do something else");
    /* A frame needs a resolved file, which `resolve_in` would supply for
     * a real diagnostic; this one is synthetic so it is set directly. */
    d.file = Some(PathBuf::from("spans.m"));
    let out = render(&d, src);

    let caret = caret_line(&out);
    assert!(out.contains("..."), "a clipped span must say it continues:\n{}", out);
    assert_eq!(
        out.lines().filter(|l| l.contains('^')).count(),
        1,
        "only the first line is underlined:\n{}",
        out
    );
    assert!(
        caret.matches('^').count() <= "one two three".len(),
        "the underline must not run past the first line:\n{}",
        out
    );
}

/// A diagnostic with nothing to point at degrades to the summary rather
/// than drawing a frame around nothing. The three unanchored
/// whole-program checks take this path.
#[test]
fn an_unanchored_diagnostic_renders_without_a_frame() {
    let d = Diagnostic::new("no node to blame", 1, 1).with_help("pass --ast");
    let out = render(&d, "whatever\n");

    assert!(out.starts_with("oz2c error: no node to blame"), "{}", out);
    assert!(!out.contains("-->"), "no frame without a position:\n{}", out);
    assert!(!out.contains('^'), "no caret without a span:\n{}", out);
    assert!(out.contains("help: pass --ast"), "the remedy still shows:\n{}", out);
}
