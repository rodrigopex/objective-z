// SPDX-License-Identifier: Apache-2.0
//
// import_resolution.rs - OZ-094: tests for oz_static::imports::resolve_imports,
// the filesystem-aware '#import' resolver that lives outside the core
// (pure, filesystem-free) transpile() pipeline.

use std::fs;
use std::path::PathBuf;

use oz_static::imports::resolve_imports;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oz_static_import_test_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn resolves_quoted_import_from_same_dir() {
    let dir = scratch_dir("quoted");
    fs::write(dir.join("Helper.h"), "@interface Helper\n@end\n").unwrap();
    let src = "#import \"Helper.h\"\nint x;\n";
    let out = resolve_imports(src, &dir, &[], &[]).unwrap();
    assert!(out.contains("@interface Helper"), "output: {}", out);
    assert!(out.contains("int x;"), "output: {}", out);
    assert!(!out.contains("#import"), "output: {}", out);
}

#[test]
fn resolves_angle_import_via_include_dir() {
    let source_dir = scratch_dir("angle_src");
    let include_root = scratch_dir("angle_include");
    fs::create_dir_all(include_root.join("Sub")).unwrap();
    fs::write(include_root.join("Sub/Bar.h"), "@interface Bar\n@end\n").unwrap();
    let src = "#import <Sub/Bar.h>\n";
    let out = resolve_imports(src, &source_dir, &[include_root], &[]).unwrap();
    assert!(out.contains("@interface Bar"), "output: {}", out);
}

#[test]
fn dedups_repeated_import() {
    let dir = scratch_dir("dedup");
    fs::write(dir.join("Shared.h"), "@interface Shared\n@end\n").unwrap();
    let src = "#import \"Shared.h\"\n#import \"Shared.h\"\n";
    let out = resolve_imports(src, &dir, &[], &[]).unwrap();
    assert_eq!(out.matches("@interface Shared").count(), 1, "output: {}", out);
    assert!(out.contains("already resolved"), "output: {}", out);
}

#[test]
fn pulls_in_sibling_impl() {
    let header_dir = scratch_dir("with_impl_headers");
    let impl_dir = scratch_dir("with_impl_srcs");
    fs::write(header_dir.join("Foo.h"), "@interface Foo\n- (void)run;\n@end\n").unwrap();
    fs::write(impl_dir.join("Foo.m"), "@implementation Foo\n- (void)run {\n}\n@end\n").unwrap();
    let src = "#import \"Foo.h\"\n";
    let out = resolve_imports(src, &header_dir, &[], &[impl_dir]).unwrap();
    assert!(out.contains("@interface Foo"), "output: {}", out);
    assert!(out.contains("@implementation Foo"), "output: {}", out);
}

#[test]
fn unresolvable_import_errors() {
    let dir = scratch_dir("unresolvable");
    let src = "#import \"DoesNotExist.h\"\n";
    let err = resolve_imports(src, &dir, &[], &[]).unwrap_err();
    assert!(err.contains("DoesNotExist.h"), "error: {}", err);
}

#[test]
fn leaves_plain_include_untouched() {
    let dir = scratch_dir("plain_include");
    let src = "#include <stdio.h>\nint main(void) { return 0; }\n";
    let out = resolve_imports(src, &dir, &[], &[]).unwrap();
    assert_eq!(out, src);
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
    let out = resolve_imports(src, &dir, &[], &[]).unwrap();
    assert!(out.contains("@compatibility_alias NSAliased Aliased;"), "output: {}", out);
    assert!(!out.contains("#ifdef"), "output: {}", out);
    assert!(!out.contains("#endif"), "output: {}", out);
}

/// Real-file regression test for OZ-094's motivating case: resolving
/// samples/hello_world/src/main.m's own `#import <Foundation/Foundation.h>`
/// against the real project layout no longer panics (the original bug,
/// #205/OZ-093) -- it now reaches the real, accepted, still-outstanding
/// limitation instead: the umbrella eagerly pulls in OZArray/OZDictionary,
/// whose real @property/@synthesize are unconditionally hard-rejected
/// (property support is a separate, larger, not-yet-scoped gap -- see
/// OZ-094's issue body).
#[test]
fn hello_world_sample_resolves_to_the_known_property_limitation() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sample_path = repo_root.join("samples/hello_world/src/main.m");
    let source = fs::read_to_string(&sample_path).unwrap();
    let source_dir = sample_path.parent().unwrap().to_path_buf();
    let include_dirs = vec![repo_root.join("include/oz_sdk")];
    let impl_dirs = vec![repo_root.join("src")];

    let resolved = resolve_imports(&source, &source_dir, &include_dirs, &impl_dirs)
        .unwrap_or_else(|e| panic!("expected resolution to succeed, got: {}", e));

    match oz_static::transpile(&resolved) {
        Ok(_) => panic!("expected the known @property limitation, but transpile succeeded"),
        Err(diags) => {
            let joined = diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n");
            assert!(joined.contains("@property"), "diagnostics: {}", joined);
            assert!(!joined.contains("no entry found for key"), "diagnostics: {}", joined);
        }
    }
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

    let resolved = resolve_imports(&source, &dir, &include_dirs, &impl_dirs)
        .unwrap_or_else(|e| panic!("expected resolution to succeed, got: {}", e));
    oz_static::transpile(&resolved).unwrap_or_else(|diags| {
        panic!(
            "expected a direct single-class import to transpile cleanly, got:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });
}
