// SPDX-License-Identifier: Apache-2.0
//
// progress.rs - where the pipeline says which pass it has reached.
//
// A px-keyboard build spends ~30s in the oz_static path and prints two
// lines: one CMake `COMMENT`, and oz2c's own summary once it has finished.
// From the outside that is indistinguishable from a hang, which is how it
// was reported (#299).
//
// The pipeline announces its boundaries here; it does not print them and it
// does not read a clock. That split is deliberate and load-bearing:
//
//   - The library stays pure. `lib.rs` and every pass module contain no
//     `println!` and no `Instant`, so the whole ~57-binary test suite keeps
//     testing transpilation rather than formatting, and a test can assert
//     the *sequence* of passes deterministically while durations -- which
//     are untestable -- live entirely in the binary (`main.rs`'s `report`).
//   - Output routing stays one decision in one place. Progress must go to
//     stdout, because `tests/tools/oz_static_build.py` reports the first
//     *stderr* line as the reason a transpile failed; a stray `eprintln!`
//     in a pass would displace a real diagnostic there.
//
// Passes deliberately absent from `Phase`, so nobody adds them later
// expecting a row to appear:
//
//   - `staticbar` -- called from inside `collect` and `emit`
//     (`collect.rs`'s `message_selector`, `emit.rs`'s `check_method_body` /
//     `check_function_body` / `check_macro_body`), so it has no top-level
//     span to report.
//   - `companion` -- the shared dispatch header and per-class slabs come
//     out of the `emit` result, not a pass of their own.
//   - `parse` -- five separate callers, each already inside some other
//     pass's span. A row for it would double-count rather than explain.

/// A pass boundary worth reporting.
///
/// Ordered as the pipeline runs, so a recorded sequence reads in pipeline
/// order and a test can compare against a literal list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// `#import` resolution -- filesystem work, so `main.rs` reports it.
    ImportResolve,
    /// The extra `collect` that `--root-class` costs. `main.rs`.
    RootClassCheck,
    /// `parse::repair_bare_macro_statements`.
    Repair,
    /// `collect::collect`.
    Collect,
    /// Reading the `--ast` files off disk. `main.rs`.
    AstRead,
    /// Parsing and merging them -- 97.5% of the wall clock on px-keyboard,
    /// and the reason this module exists.
    AstIngest,
    /// `arc::analyze`.
    Arc,
    /// `generics::check_program`.
    Generics,
    /// Slab and element-pool sizing.
    Pools,
    /// `emit::emit` or `emit::emit_split`.
    Emit,
    /// Writing the generated files and the manifest. `main.rs`.
    Write,
}

impl Phase {
    /// The name a report prints. Kebab-case and stable -- these end up in
    /// build logs that get grepped and diffed.
    pub fn label(self) -> &'static str {
        match self {
            Phase::ImportResolve => "import-resolve",
            Phase::RootClassCheck => "root-class-check",
            Phase::Repair => "repair",
            Phase::Collect => "collect",
            Phase::AstRead => "ast-read",
            Phase::AstIngest => "ast-ingest",
            Phase::Arc => "arc",
            Phase::Generics => "generics",
            Phase::Pools => "pools",
            Phase::Emit => "emit",
            Phase::Write => "write",
        }
    }
}

/// Told where the pipeline has got to.
///
/// `enter` marks a *transition*, not a scope: the previous phase ends where
/// the next begins. One call per pass rather than two is what makes this
/// safe on the early-return paths -- the front end bails out at whichever
/// pass first produces a diagnostic, and a begin/end pair would leak an
/// unclosed span every time. It also means every instant between the first
/// `enter` and the end is attributed to some phase, so a report has no
/// invisible gaps.
///
/// Both methods default to doing nothing, so `Silent` is empty and a future
/// callback breaks no existing implementor.
pub trait Observer {
    /// The pipeline has reached `phase`; whatever was running has ended.
    fn enter(&mut self, _phase: Phase) {}

    /// One `Options::ast_json` entry has been parsed and merged.
    ///
    /// `index` is that entry's position in the caller's own list, which for
    /// the CLI is the order its `--ast` flags were given -- that is what
    /// lets a report name the file without the library knowing any paths.
    /// `json_bytes` is its length, because the size of these dumps is the
    /// explanation for the pause, not an incidental detail.
    fn ast_dump(&mut self, _index: usize, _json_bytes: usize) {}
}

/// Reports nothing -- what every existing entry point passes, so they keep
/// behaving exactly as they did before there was an observer.
pub struct Silent;

impl Observer for Silent {}
