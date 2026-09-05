// SPDX-License-Identifier: Apache-2.0
//
// cli_progress.rs - oz2c's progress output, and the stream it goes on.
//
// The routing is the part that can break something else. stderr belongs to
// diagnostics: `tests/tools/oz_static_build.py` takes
// `stderr.strip().splitlines()` and reports `err[0]` as the reason a
// transpile failed, and `corpus_parity.rs` returns the whole trimmed
// stderr. A progress line on stderr would displace a real error message in
// both -- silently, and only for failing builds, which is the worst time to
// lose the reason. So these tests assert *where* output goes, not what it
// says.
//
// Nothing here asserts a duration. Timings are nondeterministic; the phase
// row labels and their order are not.

use std::path::PathBuf;
use std::process::Command;

fn oz2c_binary() -> PathBuf {
    // The test binary lives in target/<profile>/deps/, so the CLI binary
    // is two levels up. Built by the same `cargo test` invocation.
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    path.pop();
    path.join("oz2c")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A minimal real `.m` on disk, plus an outdir, in a fresh temp directory.
fn fixture(name: &str, body: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("oz_cli_progress_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let src = dir.join("main.m");
    std::fs::write(&src, body).expect("write source");
    (src, dir.join("out"))
}

/// Deliberately not `-count`: the SDK's `OZArray` declares that selector
/// returning `size_t`, and one dynamically-dispatched selector cannot have
/// two return types (#290, #294). A fixture that collides with the SDK
/// fails for a reason that has nothing to do with what is under test.
const GOOD: &str = "\
#import <Foundation/Foundation.h>

@interface Widget : OZObject {
	int _widgetTally;
}
- (int)widgetTally;
@end

@implementation Widget
- (int)widgetTally { return _widgetTally; }
@end
";

/// Two classes sharing a dynamically-dispatched selector with incompatible
/// return types -- a located hard error (#290, #297).
const BAD: &str = "\
#import <Foundation/Foundation.h>

struct alpha { int a; };
struct beta { long b; };

@interface Alpha : OZObject
- (const struct alpha *)spec;
@end
@interface Beta : OZObject
- (const struct beta *)spec;
@end

@implementation Alpha
- (const struct alpha *)spec { return (const struct alpha *)0; }
@end
@implementation Beta
- (const struct beta *)spec { return (const struct beta *)0; }
@end
";

fn run(args: &[&str], src: &PathBuf, out: &PathBuf) -> std::process::Output {
    let sdk = repo_root().join("include/oz_sdk");
    Command::new(oz2c_binary())
        .args(args)
        .arg("-I")
        .arg(&sdk)
        .arg(src)
        .arg(out)
        .output()
        .expect("run oz2c")
}

/// A successful run writes nothing to stderr.
///
/// This is the regression test for the whole design. If progress ever moves
/// to stderr, `oz_static_build.py`'s `err[0]` stops being the failure
/// reason and starts being a progress line.
#[test]
fn a_successful_run_leaves_stderr_empty() {
    let (src, out) = fixture("success", GOOD);
    let result = run(&[], &src, &out);
    assert!(result.status.success(), "stderr:\n{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(
        String::from_utf8_lossy(&result.stderr),
        "",
        "stderr is for diagnostics only; progress belongs on stdout"
    );
    assert!(
        !String::from_utf8_lossy(&result.stdout).is_empty(),
        "progress is on by default, so stdout should not be empty"
    );
}

/// Every progress line is recognisable as ours.
///
/// Build logs get grepped, and a bare line with no prefix is indistinguishable
/// from output of whatever else the build is running.
#[test]
fn every_progress_line_is_prefixed() {
    let (src, out) = fixture("prefix", GOOD);
    let result = run(&[], &src, &out);
    let stdout = String::from_utf8_lossy(&result.stdout);
    for line in stdout.lines() {
        assert!(line.starts_with("oz_static: "), "unprefixed line: {:?}", line);
    }
}

/// `--quiet` restores exactly the pre-#299 behaviour: nothing on stdout,
/// and the one summary line on stderr.
///
/// The old line is a contract, not a nicety -- it is what a caller that
/// captured our output has always seen.
#[test]
fn quiet_prints_nothing_on_stdout_and_the_old_summary_on_stderr() {
    let (src, out) = fixture("quiet", GOOD);
    let result = run(&["--quiet"], &src, &out);
    assert!(result.status.success());
    assert_eq!(String::from_utf8_lossy(&result.stdout), "");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.starts_with("oz_static: ") && stderr.contains("files generated in"),
        "got: {:?}",
        stderr
    );
}

/// `--quiet` wins over `--timings`, in either order.
///
/// Both are hand-parsed in one loop, so without an explicit rule the
/// result would depend on which came last on the command line.
#[test]
fn quiet_beats_timings_in_either_order() {
    for args in [["--quiet", "--timings"], ["--timings", "--quiet"]] {
        let (src, out) = fixture("precedence", GOOD);
        let result = run(&args, &src, &out);
        assert!(result.status.success());
        assert_eq!(
            String::from_utf8_lossy(&result.stdout),
            "",
            "--quiet must win regardless of order, got args {:?}",
            args
        );
    }
}

/// `--timings` prints a row per phase, in pipeline order.
///
/// The labels and their order are asserted; the numbers are not. The rows
/// are spans that bracket one another rather than a partition of the
/// process, so they do not sum to the wall clock and asserting that they do
/// would be wrong as well as flaky.
#[test]
fn timings_prints_the_phase_rows_in_pipeline_order() {
    let (src, out) = fixture("timings", GOOD);
    let result = run(&["--timings"], &src, &out);
    assert!(result.status.success(), "stderr:\n{}", String::from_utf8_lossy(&result.stderr));
    let stdout = String::from_utf8_lossy(&result.stdout);

    let expected = [
        "import-resolve",
        "ast-read",
        "repair",
        "collect",
        "ast-ingest",
        "arc",
        "generics",
        "pools",
        "emit",
        "write",
    ];
    let mut from = 0usize;
    for label in expected {
        let at = stdout[from..]
            .find(label)
            .unwrap_or_else(|| panic!("no '{}' row, or out of order, in:\n{}", label, stdout));
        from += at + label.len();
    }
    assert!(stdout.contains("rows are spans, not a partition"), "in:\n{}", stdout);
}

/// A rejected source still reports its reason as the first line of stderr,
/// with progress on.
///
/// Mirrors exactly what `tests/tools/oz_static_build.py` does with the
/// output, so a regression there fails here first.
#[test]
fn a_rejected_source_reports_its_reason_first_on_stderr() {
    let (src, out) = fixture("rejected", BAD);
    let result = run(&[], &src, &out);
    assert!(!result.status.success(), "this source must be rejected");
    let stderr = String::from_utf8_lossy(&result.stderr);
    let first = stderr.lines().next().unwrap_or("");
    assert!(
        first.contains("error:"),
        "the first stderr line is what a harness reports as the reason, got: {:?}",
        first
    );
}
