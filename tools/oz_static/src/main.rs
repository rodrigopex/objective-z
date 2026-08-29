// SPDX-License-Identifier: Apache-2.0
//
// main.rs - CLI entry point for the OZ-091 Track B spike.
//
// Standalone cargo crate: not wired into justfile/CMake/west yet (see
// OZ-091). Run directly for manual experimentation:
//   cargo run --manifest-path tools/oz_static/Cargo.toml -- <input.m> <outdir>

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: oz_static <input.m> <outdir>");
        return ExitCode::FAILURE;
    }
    let input_path = &args[1];
    let outdir = Path::new(&args[2]);

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
    let source_dir = Path::new(input_path).parent().unwrap_or(Path::new(".")).to_path_buf();
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let include_dirs = vec![repo_root.join("include/oz_sdk")];
    let impl_dirs = vec![repo_root.join("src")];
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
            // eventually compiles this needs both `outdir` and
            // `outdir/Foundation` on its include search path -- not
            // wired into justfile/CMake/west yet (see OZ-091).
            let foundation_dir = outdir.join("Foundation");
            if let Err(e) = fs::create_dir_all(&foundation_dir) {
                eprintln!("oz_static: error: cannot create '{}': {}", foundation_dir.display(), e);
                return ExitCode::FAILURE;
            }
            let mut count = 0;
            for (file_stem, header_h, source_c) in &out.files {
                let target_dir =
                    if resolved.foundation_stems.contains(file_stem) { &foundation_dir } else { outdir };
                let _ = fs::write(target_dir.join(format!("{}.h", file_stem)), header_h);
                let _ = fs::write(target_dir.join(format!("{}.c", file_stem)), source_c);
                count += 2;
            }
            let _ = fs::write(foundation_dir.join("oz_static_dispatch.h"), out.companion_h);
            let _ = fs::write(foundation_dir.join("oz_static_dispatch.c"), out.companion_c);
            count += 2;
            eprintln!("oz_static: {} files generated in {}", count, outdir.display());
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
