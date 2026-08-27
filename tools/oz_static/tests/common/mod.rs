// SPDX-License-Identifier: Apache-2.0
//
// common/mod.rs - shared test helper: transpile, compile with the real
// PAL host headers, link, and run. Validates the static subset produces
// real, working C -- not just text that merely parses.
//
// Each test binary in tests/ compiles this module separately and uses
// only a subset of it, so per-binary "never used" warnings are expected.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// `tools/oz_static/../../include` -- the repo's real platform headers.
fn include_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../include")
}

/// Transpile `source`, compile the primary output + companion file against
/// the real PAL (host backend), link, run, and return captured stdout.
/// Panics with full diagnostics/compiler output on any failure.
pub fn compile_and_run(source: &str, stem: &str) -> String {
    let out = oz_static::transpile(source).unwrap_or_else(|diags| {
        panic!(
            "transpile('{}') was expected to succeed but produced diagnostics:\n{}",
            stem,
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });

    let dir = std::env::temp_dir().join(format!("oz_static_test_{}", stem));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let main_c = dir.join(format!("{}.c", stem));
    fs::write(&main_c, &out.source_c).unwrap();
    fs::write(dir.join("oz_static_dispatch.h"), &out.companion_h).unwrap();
    let dispatch_c = dir.join("oz_static_dispatch.c");
    fs::write(&dispatch_c, &out.companion_c).unwrap();

    let main_o = dir.join("main.o");
    let dispatch_o = dir.join("dispatch.o");
    let bin = dir.join("bin");

    cc(&["-DOZ_PLATFORM_HOST", "-I", include_dir().to_str().unwrap(), "-I",
         dir.to_str().unwrap(), "-c", main_c.to_str().unwrap(), "-o", main_o.to_str().unwrap()]);
    cc(&["-DOZ_PLATFORM_HOST", "-I", include_dir().to_str().unwrap(), "-I",
         dir.to_str().unwrap(), "-c", dispatch_c.to_str().unwrap(), "-o", dispatch_o.to_str().unwrap()]);
    cc(&[main_o.to_str().unwrap(), dispatch_o.to_str().unwrap(), "-o", bin.to_str().unwrap()]);

    let run = Command::new(&bin).output().unwrap_or_else(|e| panic!("failed to run binary: {}", e));
    assert!(run.status.success(), "binary exited non-zero: {:?}\nstdout: {}\nstderr: {}",
            run.status, String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));

    String::from_utf8(run.stdout).unwrap()
}

fn cc(args: &[&str]) {
    let output = Command::new("cc").args(args).output().unwrap_or_else(|e| panic!("failed to run cc: {}", e));
    assert!(
        output.status.success(),
        "cc {:?} failed:\n{}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Transpile `source`, expecting it to be rejected. Returns the joined
/// diagnostic messages for substring assertions.
pub fn expect_reject(source: &str) -> String {
    match oz_static::transpile(source) {
        Ok(_) => panic!("expected transpile to be rejected by the static bar, but it succeeded"),
        Err(diags) => diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"),
    }
}
