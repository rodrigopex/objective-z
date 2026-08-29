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
    // the core (pure, filesystem-free) `transpile()` pipeline -- see
    // `imports.rs`. Default search paths: this repo's own
    // `include/oz_sdk` (headers) and `src` (their sibling `.m`
    // implementations), the same layout every Foundation class lives in.
    let source_dir = Path::new(input_path).parent().unwrap_or(Path::new(".")).to_path_buf();
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let include_dirs = vec![repo_root.join("include/oz_sdk")];
    let impl_dirs = vec![repo_root.join("src")];
    let source = match oz_static::imports::resolve_imports(&source, &source_dir, &include_dirs, &impl_dirs) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("oz_static: error: {}", e);
            return ExitCode::FAILURE;
        }
    };

    match oz_static::transpile(&source) {
        Ok(out) => {
            if let Err(e) = fs::create_dir_all(outdir) {
                eprintln!("oz_static: error: cannot create '{}': {}", outdir.display(), e);
                return ExitCode::FAILURE;
            }
            let stem = Path::new(input_path).file_stem().and_then(|s| s.to_str()).unwrap_or("out");
            let _ = fs::write(outdir.join(format!("{}.c", stem)), out.source_c);
            let _ = fs::write(outdir.join("oz_static_dispatch.h"), out.companion_h);
            let _ = fs::write(outdir.join("oz_static_dispatch.c"), out.companion_c);
            eprintln!("oz_static: 3 files generated in {}", outdir.display());
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
