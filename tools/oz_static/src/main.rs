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
        "usage: oz2c [-I <dir>]... [--impl-dir <dir>]... [--manifest <path>] \
         [--root-class <name>] [--pool-sizes <Class=N,...>] [--ast <ast.json>]... \
         [--heap-support] \
         <input.m>... <outdir>"
    );
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut extra_include_dirs: Vec<PathBuf> = Vec::new();
    let mut extra_impl_dirs: Vec<PathBuf> = Vec::new();
    let mut manifest_path: Option<PathBuf> = None;
    let mut expected_root: Option<String> = None;
    let mut pool_overrides = oz_static::PoolOverrides::new();
    let mut ast_paths: Vec<PathBuf> = Vec::new();
    let mut heap_support = false;
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
            "--root-class" => {
                let Some(name) = args.get(i + 1) else { return usage() };
                expected_root = Some(name.clone());
                i += 2;
            }
            // Same spelling and meaning as the Python backend's flag.
            // Enables `+allocWithHeap:` and the heap-aware free path; the
            // generated code is additionally guarded by `OZ_HEAP_SUPPORT`,
            // which is what makes the PAL expose the heap it needs.
            "--heap-support" => {
                heap_support = true;
                i += 1;
            }
            // Clang resolves types; tree-sitter does not. Supplying the AST
            // is what lets oz_static know which ivars are objects the class
            // owns -- including `id`-typed ones, which it otherwise has to
            // skip rather than risk releasing a non-object. Produce it with
            // `-fobjc-arc`, or the dump carries no ownership at all.
            "--ast" => {
                let Some(path) = args.get(i + 1) else { return usage() };
                ast_paths.push(PathBuf::from(path));
                i += 2;
            }
            // Same spelling and meaning as the Python backend's flag, so a
            // sample's CMakeLists.txt needs no per-backend variant. Also
            // accepted as an `/* oz-pool: ... */` comment in the source
            // itself, which is what the oracle's own behavior cases use;
            // this flag wins for the classes it names (see
            // `pools::PoolSizes::set_overrides`).
            "--pool-sizes" => {
                let Some(spec) = args.get(i + 1) else { return usage() };
                match oz_static::pools::parse_pool_sizes(spec) {
                    Ok(sizes) => pool_overrides.extend(sizes),
                    Err(why) => {
                        eprintln!("oz_static: error: --pool-sizes: {}", why);
                        return ExitCode::FAILURE;
                    }
                }
                i += 2;
            }
            arg => {
                positional.push(arg.to_string());
                i += 1;
            }
        }
    }
    // Every positional but the last is an entry `.m`; the last is the
    // output directory. A build system lists every `.m` a target owns
    // (see `cmake/oz_static.cmake`), and all of them become one
    // translation unit -- see `imports::resolve_entry_files` for why one
    // unit rather than one run per file.
    if positional.len() < 2 {
        return usage();
    }
    let outdir = Path::new(positional.last().unwrap());
    let entry_paths: Vec<PathBuf> =
        positional[..positional.len() - 1].iter().map(PathBuf::from).collect();

    for path in &entry_paths {
        if !path.is_file() {
            eprintln!("oz_static: error: no such input file: '{}'", path.display());
            return ExitCode::FAILURE;
        }
    }

    // `#import` resolution needs a real filesystem, so it lives outside
    // the core (pure, filesystem-free) `transpile()`/`transpile_split()`
    // pipeline -- see `imports.rs`. Default search paths: this repo's
    // own `include/oz_sdk` (headers) and `src` (their sibling `.m`
    // implementations), the same layout every Foundation class lives in.
    // `-I`/`--impl-dir` extend these for a caller's own project-local
    // headers -- mirroring gcc's `-I`, plus a second flag because
    // `find_sibling_impl` (imports.rs) only searches `impl_dirs`, never
    // a header's own directory.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut include_dirs = vec![repo_root.join("include/oz_sdk")];
    include_dirs.extend(extra_include_dirs);
    let mut impl_dirs = vec![repo_root.join("src")];
    impl_dirs.extend(extra_impl_dirs);
    let resolved =
        match oz_static::imports::resolve_entry_files(&entry_paths, &include_dirs, &impl_dirs) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("oz_static: error: {}", e);
                return ExitCode::FAILURE;
            }
        };

    // oz_static infers the root class (the one class with no superclass)
    // rather than being told it, so `--root-class` is a cross-check on the
    // build system's expectation, not an input to codegen: it catches a
    // target configured for a root that isn't actually the root, which
    // would otherwise produce a working-but-differently-rooted program.
    // Only paid for when the flag is passed, since it needs its own
    // `collect` pass.
    if let Some(expected) = &expected_root {
        let (program, _) = oz_static::collect::collect(&resolved.text);
        match program.root_class() {
            Some(actual) if actual == expected => {}
            Some(actual) => {
                eprintln!(
                    "oz_static: error: --root-class '{}' does not match this program's root class \
                     '{}' (the root is inferred as the class with no superclass, not configured)",
                    expected, actual
                );
                return ExitCode::FAILURE;
            }
            None => {
                eprintln!(
                    "oz_static: error: --root-class '{}' was requested but this program declares \
                     no root class (every class has a superclass)",
                    expected
                );
                return ExitCode::FAILURE;
            }
        }
    }

    let mut ast_json: Vec<String> = Vec::new();
    for path in &ast_paths {
        match fs::read_to_string(path) {
            Ok(text) => ast_json.push(text),
            Err(e) => {
                eprintln!("oz_static: error: cannot read --ast '{}': {}", path.display(), e);
                return ExitCode::FAILURE;
            }
        }
    }

    match oz_static::transpile_split_with_options(
        &resolved.text,
        &resolved.origins,
        &oz_static::Options { pool_sizes: pool_overrides, ast_json, heap_support },
    ) {
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
                // A spliced pure-C header gets no output pair. Its text was
                // needed so the parse saw the whole program, but there is
                // nothing in it to transpile, and copying it back out is at
                // best duplication of a header the C compiler already has:
                // `cmake/oz_static.cmake` puts the module's own `include/`
                // on the path and links `src/OZLog.c` itself. At worst it
                // is a redefinition -- `include/oz_sdk/assert.h` is an
                // AST-analysis shim (its own comment: "The generated C
                // includes platform/oz_assert.h which provides the real
                // macros"), so its `static inline oz_assert_msg` came out
                // as a generated `assert.c` the PAL had already turned into
                // a function-like macro: "expected identifier or '('".
                if resolved.pure_c_stems.contains(file_stem) {
                    continue;
                }
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
