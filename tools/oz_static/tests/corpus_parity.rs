// SPDX-License-Identifier: Apache-2.0
//
// corpus_parity.rs - runs the *shared* behavior corpus through oz_static.
//
// `tests/behavior/cases/*/*.m` is the Python pipeline's own 73-case
// behavior suite. Rather than maintaining a separate, smaller, hand-picked
// set of fixtures for oz_static, this drives those same files: they are
// the definition of what the mature backend supports, so they are the
// honest measure of parity. A case that oz_static cannot handle shows up
// here rather than being quietly absent.
//
// Two levels are checked, because they fail for different reasons:
//
//   1. every case must *transpile* -- this exercises real `#import`
//      resolution over the real SDK headers and sources, the whole
//      collect/generics/emit pipeline, and each case's own
//      `/* oz-pool: ... */` directive.
//   2. the generated C must *compile*. Transpiling proves the input was
//      understood; compiling proves the output is real C. A few cases
//      still fail here, listed and explained in `KNOWN_CC_FAILURES` --
//      an allowlist rather than a skip, so anything new fails loudly and
//      fixing one of these fails too, forcing the list to be updated.
//
// Not covered here: *running* the cases. Each has a Unity `_test.c`
// driver, and wiring those up (plus a generated test main) is the
// cross-backend behavioral comparison this does not yet do. Compiling is
// the strongest check available without that harness, and it is
// deliberately not described as behavioral equivalence.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Cases whose generated C does not compile yet, each with the reason.
/// Both causes are understood and tracked; neither is a mystery.
const KNOWN_CC_FAILURES: &[(&str, &str)] = &[
    // The companion header declares OZTimer's initialiser with `struct
    // k_timer` parameters, but the source's own `#include
    // <zephyr/kernel.h>` is not carried into that header, so C invents a
    // prototype-scoped type and the real definition then conflicts. The
    // oracle propagates the include (see
    // tests/zephyr/generated/OZTimer_ozh.h:7).
    ("foundation/timer_basic.m", "source #include not propagated into companion header"),
    ("foundation/timer_zephyr.m", "source #include not propagated into companion header"),
    // OZHeap.h's `struct oz_heap_inner` and platform/oz_platform.h's
    // fallback are guarded on the same macro, which neither defines
    // outside OZ_HEAP_SUPPORT, so both fire. Needs the heap-support path
    // (`allocWithHeap:`), which oz_static does not emit.
    ("memory/heap_alloc.m", "oz_heap_inner redefinition; no allocWithHeap: support"),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn oz2c_binary() -> PathBuf {
    // The test binary lives in target/<profile>/deps/, so the CLI binary
    // is two levels up. Built by the same `cargo test` invocation.
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    path.pop();
    path.join("oz2c")
}

fn corpus_cases() -> Vec<PathBuf> {
    let cases_dir = repo_root().join("tests/behavior/cases");
    let mut out = Vec::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&cases_dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", cases_dir.display(), e))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "m"))
            .collect();
        files.sort();
        out.extend(files);
    }
    out
}

/// `<category>/<file>.m`, the form `KNOWN_CC_FAILURES` and failure
/// messages use.
fn case_id(case: &Path) -> String {
    let file = case.file_name().unwrap().to_str().unwrap();
    let category = case.parent().unwrap().file_name().unwrap().to_str().unwrap();
    format!("{}/{}", category, file)
}

fn transpile_case(case: &Path, outdir: &Path) -> Result<(), String> {
    let root = repo_root();
    let output = Command::new(oz2c_binary())
        .arg("-I")
        .arg(root.join("include/oz_sdk"))
        .arg("-I")
        .arg(root.join("tests/behavior/include"))
        .arg("--impl-dir")
        .arg(root.join("src"))
        .arg(case)
        .arg(outdir)
        .output()
        .map_err(|e| format!("cannot run oz2c: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn generated_c_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            out.extend(generated_c_files(&path));
        } else if path.extension().is_some_and(|e| e == "c") {
            out.push(path);
        }
    }
    out.sort();
    out
}

fn compile_generated(dir: &Path) -> Result<(), String> {
    let root = repo_root();
    for c_file in generated_c_files(dir) {
        let output = Command::new("cc")
            .args(["-DOZ_PLATFORM_HOST", "-I"])
            .arg(root.join("include"))
            .arg("-I")
            .arg(root.join("tests/behavior/include/zephyr_stubs"))
            .arg("-I")
            .arg(dir)
            .arg("-I")
            .arg(dir.join("Foundation"))
            .arg("-c")
            .arg(&c_file)
            .args(["-o", "/dev/null"])
            .output()
            .map_err(|e| format!("cannot run cc: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let first = stderr.lines().find(|l| l.contains("error:")).unwrap_or("(no error line)");
            return Err(format!("{}: {}", c_file.file_name().unwrap().to_string_lossy(), first));
        }
    }
    Ok(())
}

/// Every case in the shared corpus must transpile. No allowlist: a case
/// oz_static cannot even read is a parity gap, not a known limitation.
#[test]
fn every_corpus_case_transpiles() {
    let cases = corpus_cases();
    assert!(cases.len() >= 70, "expected the full corpus, found {} cases", cases.len());

    let tmp = std::env::temp_dir().join("oz_static_corpus_transpile");
    let _ = std::fs::remove_dir_all(&tmp);

    let mut failures = Vec::new();
    for case in &cases {
        let outdir = tmp.join(case_id(case).replace('/', "_").replace(".m", ""));
        std::fs::create_dir_all(&outdir).unwrap();
        if let Err(why) = transpile_case(case, &outdir) {
            failures.push(format!("{}: {}", case_id(case), why));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} corpus cases failed to transpile:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// The generated C must compile, except for the cases listed in
/// `KNOWN_CC_FAILURES`. Those are asserted to *still* fail, so fixing one
/// without updating the list is itself a failure -- the list cannot rot
/// into a set of silently-skipped cases.
#[test]
fn corpus_generated_c_compiles() {
    let cases = corpus_cases();
    let tmp = std::env::temp_dir().join("oz_static_corpus_compile");
    let _ = std::fs::remove_dir_all(&tmp);

    let mut unexpected_failures = Vec::new();
    let mut unexpected_successes = Vec::new();

    for case in &cases {
        let id = case_id(case);
        let expected_to_fail = KNOWN_CC_FAILURES.iter().any(|(known, _)| *known == id);
        let outdir = tmp.join(id.replace('/', "_").replace(".m", ""));
        std::fs::create_dir_all(&outdir).unwrap();

        if transpile_case(case, &outdir).is_err() {
            // Covered by `every_corpus_case_transpiles`; nothing to add.
            continue;
        }
        match (compile_generated(&outdir), expected_to_fail) {
            (Err(why), false) => unexpected_failures.push(format!("{}: {}", id, why)),
            (Ok(()), true) => unexpected_successes.push(id),
            _ => {}
        }
    }

    assert!(
        unexpected_failures.is_empty(),
        "generated C stopped compiling for {} case(s):\n{}",
        unexpected_failures.len(),
        unexpected_failures.join("\n")
    );
    assert!(
        unexpected_successes.is_empty(),
        "these cases now compile and should be removed from KNOWN_CC_FAILURES:\n{}",
        unexpected_successes.join("\n")
    );
}
