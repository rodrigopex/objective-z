// SPDX-License-Identifier: Apache-2.0
//
// import_resolution.rs - OZ-094: tests for oz2c::imports::resolve_imports,
// the filesystem-aware '#import' resolver that lives outside the core
// (pure, filesystem-free) transpile()/transpile_split() pipeline. Also
// covers OZ-096's origin-range tracking (`ResolvedSource::origins`),
// added alongside the merged text itself. The other half of that
// provenance -- the per-byte map back to the `.m`/`.h` line a byte was
// spliced from (`ResolvedSource::source_location`, #305) -- is covered by
// `tests/source_map.rs`.

use std::fs;
use std::path::PathBuf;

use oz2c::imports::resolve_imports;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oz2c_import_test_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn resolves_quoted_import_from_same_dir() {
    let dir = scratch_dir("quoted");
    fs::write(dir.join("Helper.h"), "@interface Helper\n@end\n").unwrap();
    let src = "#import \"Helper.h\"\nint x;\n";
    let out = resolve_imports(src, &dir, &[], &[], "main").unwrap();
    assert!(out.text.contains("@interface Helper"), "output: {}", out.text);
    assert!(out.text.contains("int x;"), "output: {}", out.text);
    assert!(!out.text.contains("#import"), "output: {}", out.text);
}

#[test]
fn resolves_angle_import_via_include_dir() {
    let source_dir = scratch_dir("angle_src");
    let include_root = scratch_dir("angle_include");
    fs::create_dir_all(include_root.join("Sub")).unwrap();
    fs::write(include_root.join("Sub/Bar.h"), "@interface Bar\n@end\n").unwrap();
    let src = "#import <Sub/Bar.h>\n";
    let out = resolve_imports(src, &source_dir, &[include_root], &[], "main").unwrap();
    assert!(out.text.contains("@interface Bar"), "output: {}", out.text);
}

#[test]
fn dedups_repeated_import() {
    let dir = scratch_dir("dedup");
    fs::write(dir.join("Shared.h"), "@interface Shared\n@end\n").unwrap();
    let src = "#import \"Shared.h\"\n#import \"Shared.h\"\n";
    let out = resolve_imports(src, &dir, &[], &[], "main").unwrap();
    assert_eq!(out.text.matches("@interface Shared").count(), 1, "output: {}", out.text);
    assert!(out.text.contains("already resolved"), "output: {}", out.text);
}

#[test]
fn pulls_in_sibling_impl() {
    let header_dir = scratch_dir("with_impl_headers");
    let impl_dir = scratch_dir("with_impl_srcs");
    fs::write(header_dir.join("Foo.h"), "@interface Foo\n- (void)run;\n@end\n").unwrap();
    fs::write(impl_dir.join("Foo.m"), "@implementation Foo\n- (void)run {\n}\n@end\n").unwrap();
    let src = "#import \"Foo.h\"\n";
    let out = resolve_imports(src, &header_dir, &[], &[impl_dir], "main").unwrap();
    assert!(out.text.contains("@interface Foo"), "output: {}", out.text);
    assert!(out.text.contains("@implementation Foo"), "output: {}", out.text);
    // Both the header's and its sibling impl's contribution are tagged
    // with the header's own stem ("Foo"), not "main" -- one file, one origin.
    assert!(out.origins.iter().any(|(s, _)| s == "Foo"), "origins: {:?}", out.origins);
    assert!(!out.origins.iter().any(|(s, _)| s == "main"), "origins: {:?}", out.origins);
}

/// An unresolvable `#import` is an error, and **located** (#550).
///
/// The assertion on the message is the original; the position is what #550
/// added. It used to be the one unlocated diagnostic the front end could
/// produce -- clear about the path and the search dirs, silent about the
/// file and line that asked -- so the error carries a `Diagnostic` now
/// rather than a bare `String`.
#[test]
fn unresolvable_import_errors() {
    let dir = scratch_dir("unresolvable");
    /* A line before it, so a position of 1 cannot pass by accident. */
    let src = "#import \"Present.h\"\n#import \"DoesNotExist.h\"\n";
    std::fs::write(dir.join("Present.h"), "/* empty */\n").unwrap();
    let err = resolve_imports(src, &dir, &[], &[], "main").unwrap_err();
    assert!(
        err.diagnostic.message.contains("DoesNotExist.h"),
        "error: {}",
        err
    );
    assert_eq!(err.diagnostic.line, 2, "the line that wrote it: {}", err);
    assert_eq!(err.diagnostic.col, 1, "the directive starts the line: {}", err);
    assert!(
        err.diagnostic.file.is_some(),
        "a file, or `render` cannot draw the frame and falls back to a bare line: {}",
        err
    );
    /* The span has to index the buffer the error carries, or the snippet
     * `render` draws comes from the wrong text. */
    let span = err.diagnostic.span.clone().expect("a span");
    assert_eq!(
        &err.source[span],
        "#import \"DoesNotExist.h\"",
        "the span must cover the directive in the carried source"
    );
}

/// An I/O failure has **no** position, and that is correct rather than a
/// gap: "cannot read 'X': permission denied" is about the filesystem, and a
/// caret would point at a directive that is right.
///
/// Asserted so the two cases cannot be collapsed again -- treating them the
/// same is what #550 was.
#[test]
fn an_unreadable_file_is_reported_without_a_position() {
    let err = oz2c::imports::resolve_entry_files(
        &[std::path::PathBuf::from("/nonexistent/nowhere/absent.m")],
        &[],
        &[],
    )
    .unwrap_err();
    assert!(err.diagnostic.message.contains("cannot read"), "error: {}", err);
    assert!(
        err.diagnostic.span.is_none() && err.diagnostic.file.is_none(),
        "an I/O error has no source position: {}",
        err
    );
}

#[test]
fn leaves_plain_include_untouched() {
    let dir = scratch_dir("plain_include");
    let src = "#include <stdio.h>\nint main(void) { return 0; }\n";
    let out = resolve_imports(src, &dir, &[], &[], "main").unwrap();
    assert_eq!(out.text, src);
    // No imports at all -- the whole file is one "main" origin.
    assert_eq!(out.origins, vec![("main".to_string(), 0..src.len())]);
}

#[test]
fn unwraps_clang_guard_in_resolved_header() {
    let dir = scratch_dir("clang_guard");
    fs::write(
        dir.join("Aliased.h"),
        "@interface Aliased\n@end\n\n#ifdef __clang__\n@compatibility_alias NSAliased Aliased;\n#endif\n",
    )
    .unwrap();
    let src = "#import \"Aliased.h\"\n";
    let out = resolve_imports(src, &dir, &[], &[], "main").unwrap();
    assert!(out.text.contains("@compatibility_alias NSAliased Aliased;"), "output: {}", out.text);
    assert!(!out.text.contains("#ifdef"), "output: {}", out.text);
    assert!(!out.text.contains("#endif"), "output: {}", out.text);
}

/// The main file's own origin can be non-contiguous: lines before and
/// after an `#import` both belong to "main", as two separate ranges.
#[test]
fn main_stem_origin_is_non_contiguous_around_an_import() {
    let dir = scratch_dir("split_origin");
    fs::write(dir.join("Helper.h"), "@interface Helper\n@end\n").unwrap();
    let src = "int before;\n#import \"Helper.h\"\nint after;\n";
    let out = resolve_imports(src, &dir, &[], &[], "main").unwrap();
    let main_ranges: Vec<_> = out.origins.iter().filter(|(s, _)| s == "main").collect();
    assert_eq!(main_ranges.len(), 2, "origins: {:?}", out.origins);
    assert!(out.text[main_ranges[0].1.clone()].contains("before"), "output: {}", out.text);
    assert!(out.text[main_ranges[1].1.clone()].contains("after"), "output: {}", out.text);
    let helper_range = out.origins.iter().find(|(s, _)| s == "Helper").unwrap();
    assert!(out.text[helper_range.1.clone()].contains("@interface Helper"), "output: {}", out.text);
}

/// `foundation_stems` (OZ-096) distinguishes a header/impl resolved
/// from inside `include_dirs`/`impl_dirs` (SDK content) from the
/// caller's own project-local files -- lets a caller mirror the Python
/// pipeline's `outdir/Foundation/` split when writing output files.
#[test]
fn foundation_stems_tracks_sdk_origin_vs_project_local() {
    let project_dir = scratch_dir("foundation_project");
    let sdk_include = scratch_dir("foundation_sdk_include");
    let sdk_src = scratch_dir("foundation_sdk_src");
    fs::write(project_dir.join("Local.h"), "@interface Local\n@end\n").unwrap();
    fs::write(sdk_include.join("SdkClass.h"), "@interface SdkClass\n@end\n").unwrap();
    fs::write(sdk_src.join("SdkClass.m"), "@implementation SdkClass\n@end\n").unwrap();
    let src = "#import \"Local.h\"\n#import <SdkClass.h>\n";

    let out = resolve_imports(src, &project_dir, &[sdk_include], &[sdk_src], "main").unwrap();
    assert!(out.foundation_stems.contains("SdkClass"), "foundation_stems: {:?}", out.foundation_stems);
    assert!(!out.foundation_stems.contains("Local"), "foundation_stems: {:?}", out.foundation_stems);
    assert!(!out.foundation_stems.contains("main"), "foundation_stems: {:?}", out.foundation_stems);
}

/// Real-file regression test for OZ-094's motivating case: resolving
/// samples/hello_world/src/main.m's own `#import <Foundation/Foundation.h>`
/// against the real project layout no longer panics (the original bug,
/// #205/OZ-093). The umbrella eagerly pulls in OZArray/OZDictionary, whose
/// real `@property (readonly) uint16_t iterIdx;` + `@synthesize iterIdx =
/// _iterIdx;` used to be a hard-rejected, separate, accepted limitation --
/// OZ-095 closed that gap, so this now transpiles cleanly end to end.
#[test]
fn hello_world_sample_transpiles_successfully() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sample_path = repo_root.join("samples/hello_world/src/main.m");
    let source = fs::read_to_string(&sample_path).unwrap();
    let source_dir = sample_path.parent().unwrap().to_path_buf();
    let include_dirs = vec![repo_root.join("include/oz_sdk")];
    let impl_dirs = vec![repo_root.join("src")];

    let resolved = resolve_imports(&source, &source_dir, &include_dirs, &impl_dirs, "main")
        .unwrap_or_else(|e| panic!("expected resolution to succeed, got: {}", e));

    oz2c::transpile(&resolved.text).unwrap_or_else(|diags| {
        panic!(
            "expected the real hello_world sample to transpile cleanly, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}

/// Companion to the above: importing a single Foundation class directly
/// (not the eager Foundation.h umbrella) resolves and transpiles clean
/// end to end -- the umbrella's own limitation is about what it pulls
/// in, not a defect in resolution itself.
#[test]
fn direct_single_class_import_transpiles_successfully() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = scratch_dir("direct_ozobject");
    fs::write(
        dir.join("main.m"),
        "#import <Foundation/OZObject.h>\n\n\
         @interface Foo : OZObject\n- (void)run;\n@end\n\
         @implementation Foo\n- (void)run {\n}\n@end\n",
    )
    .unwrap();
    let source = fs::read_to_string(dir.join("main.m")).unwrap();
    let include_dirs = vec![repo_root.join("include/oz_sdk")];
    let impl_dirs = vec![repo_root.join("src")];

    let resolved = resolve_imports(&source, &dir, &include_dirs, &impl_dirs, "main")
        .unwrap_or_else(|e| panic!("expected resolution to succeed, got: {}", e));
    oz2c::transpile(&resolved.text).unwrap_or_else(|diags| {
        panic!(
            "expected a direct single-class import to transpile cleanly, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}
