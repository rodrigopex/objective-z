// SPDX-License-Identifier: Apache-2.0
//
// main.rs - CLI entry point for the OZ-091 Track B spike.
//
// Wired into CMake by cmake/oz_static.cmake (CONFIG_OBJZ_BACKEND_STATIC).
// Run directly for manual experimentation:
//   cargo run --manifest-path tools/oz_static/Cargo.toml -- <input.m> <outdir>

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage: ozcc [-I <dir>]... [--impl-dir <dir>]... [--manifest <path>] <input.m> <outdir>"
    );
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut extra_include_dirs: Vec<PathBuf> = Vec::new();
    let mut extra_impl_dirs: Vec<PathBuf> = Vec::new();
    let mut manifest_path: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-I" => {
                let Some(dir) = args.get(i + 1) else { return usage() };
                extra_include_dirs.push(PathBuf::from(dir));
                i += 2;
            }
            "--impl-dir" => {
                let Some(dir) = args.get(i + 1) else { return usage() };
                extra_impl_dirs.push(PathBuf::from(dir));
                i += 2;
            }
            "--manifest" => {
                let Some(path) = args.get(i + 1) else { return usage() };
                manifest_path = Some(PathBuf::from(path));
                i += 2;
            }
            arg => {
                positional.push(arg.to_string());
                i += 1;
            }
        }
    }
    if positional.len() != 2 {
        return usage();
    }
    let input_path = &positional[0];
    let outdir = Path::new(&positional[1]);

    let source = match fs::read_to_string(input_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("oz_static: error: cannot read '{}': {}", input_path, e);
            return ExitCode::FAILURE;
        }
    };

    // `#import` resolution needs a real filesystem, so it lives outside
    // the core (pure, filesystem-free) `transpile()`/`transpile_split()`
    // pipeline -- see `imports.rs`. Default search paths: this repo's
    // own `include/oz_sdk` (headers) and `src` (their sibling `.m`
    // implementations), the same layout every Foundation class lives in.
    // `-I`/`--impl-dir` extend these for a caller's own project-local
    // headers -- mirroring gcc's `-I`, plus a second flag because
    // `find_sibling_impl` (imports.rs) only searches `impl_dirs`, never
    // a header's own directory.
    let source_dir = Path::new(input_path).parent().unwrap_or(Path::new(".")).to_path_buf();
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut include_dirs = vec![repo_root.join("include/oz_sdk")];
    include_dirs.extend(extra_include_dirs);
    let mut impl_dirs = vec![repo_root.join("src")];
    impl_dirs.extend(extra_impl_dirs);
    let stem = Path::new(input_path).file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    let resolved =
        match oz_static::imports::resolve_imports(&source, &source_dir, &include_dirs, &impl_dirs, stem) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("oz_static: error: {}", e);
                return ExitCode::FAILURE;
            }
        };

    match oz_static::transpile_split(&resolved.text, &resolved.origins) {
        Ok(out) => {
            // Foundation/SDK-origin files land in their own subdirectory,
            // matching the Python pipeline's own `outdir/Foundation/`
            // layout -- the caller's own project-local files stay at
            // `outdir/` directly. Every generated `#include` is still
            // just a bare filename (see `emit::emit_split`), so whatever
            // compiles this needs both `outdir` and `outdir/Foundation`
            // on its include search path -- see cmake/oz_static.cmake.
            let foundation_dir = outdir.join("Foundation");
            if let Err(e) = fs::create_dir_all(&foundation_dir) {
                eprintln!("oz_static: error: cannot create '{}': {}", foundation_dir.display(), e);
                return ExitCode::FAILURE;
            }
            let mut written: Vec<PathBuf> = Vec::new();
            for (file_stem, header_h, source_c) in &out.files {
                let target_dir =
                    if resolved.foundation_stems.contains(file_stem) { &foundation_dir } else { outdir };
                let h_path = target_dir.join(format!("{}.h", file_stem));
                let c_path = target_dir.join(format!("{}.c", file_stem));
                let _ = fs::write(&h_path, header_h);
                let _ = fs::write(&c_path, source_c);
                written.push(h_path);
                written.push(c_path);
            }
            let dispatch_h = foundation_dir.join("oz_static_dispatch.h");
            let dispatch_c = foundation_dir.join("oz_static_dispatch.c");
            let _ = fs::write(&dispatch_h, out.companion_h);
            let _ = fs::write(&dispatch_c, out.companion_c);
            written.push(dispatch_h);
            written.push(dispatch_c);
            if let Some(path) = &manifest_path {
                let manifest_text: String =
                    written.iter().map(|p| format!("{}\n", p.display())).collect();
                if let Err(e) = fs::write(path, manifest_text) {
                    eprintln!("oz_static: error: cannot write manifest '{}': {}", path.display(), e);
                    return ExitCode::FAILURE;
                }
            }
            eprintln!("oz_static: {} files generated in {}", written.len(), outdir.display());
            ExitCode::SUCCESS
        }
        Err(diags) => {
            for d in &diags {
                eprintln!("oz_static: error: {}", d);
            }
            ExitCode::FAILURE
        }
    }
}
