// SPDX-License-Identifier: Apache-2.0
//
// report.rs - turns pass boundaries into the lines a build prints.
//
// All clock reading and all printing live here, in the binary, so that
// `lib.rs` and the passes stay free of both (see `progress`). That is what
// keeps the phase-sequence testable -- the sequence is deterministic and
// asserted; the durations are not and are never asserted.
//
// Everything goes to **stdout**. stderr belongs to diagnostics:
// `tests/tools/oz_static_build.py` and `tools/oz_static/tests/
// corpus_parity.rs` report its first line as the reason a transpile failed,
// so progress on stderr would displace the real error. Every automated
// consumer captures stdout and discards it, which is also why progress can
// be on by default without making CI noisier.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use oz_static::progress::{Observer, Phase};

/// How much to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Nothing on stdout, and the summary on stderr exactly as oz2c
    /// printed it before this existed. For anything parsing our output.
    Quiet,
    /// A header, a line per AST dump, and a summary. The default.
    Normal,
    /// The above plus a duration per dump and a per-phase table.
    Timings,
}

/// Accumulates spans and prints progress.
pub struct Reporter {
    level: Level,
    started: Instant,
    /// When the phase currently running began.
    phase_at: Instant,
    current: Option<Phase>,
    /// Closed spans, in the order the phases were first entered. A `Vec`
    /// rather than a map because the order *is* the pipeline order, which
    /// is the only order worth printing them in.
    spans: Vec<(Phase, Duration)>,
    /// One label per `--ast` path, indexed as `Options::ast_json` is.
    ast_labels: Vec<String>,
    ast_total_bytes: u64,
    ast_seen: usize,
    /// When the dump currently being ingested began, for the per-dump
    /// duration column.
    ast_at: Instant,
}

impl Reporter {
    pub fn new(level: Level) -> Self {
        let now = Instant::now();
        Reporter {
            level,
            started: now,
            phase_at: now,
            current: None,
            spans: Vec::new(),
            ast_labels: Vec::new(),
            ast_total_bytes: 0,
            ast_seen: 0,
            ast_at: now,
        }
    }

    fn quiet(&self) -> bool {
        self.level == Level::Quiet
    }

    /// Remember what each `--ast` index refers to.
    ///
    /// The label is the filename as passed, with a trailing `.ast.json`
    /// trimmed. Deliberately not un-mangled back to a source name:
    /// `cmake/oz_static.cmake` builds these through
    /// `string(MAKE_C_IDENTIFIER)`, so `main.m` arrives as `main_m`, and
    /// from oz2c's side an `--ast` argument has no guaranteed relation to
    /// any source at all. The name as given is the only truthful label, and
    /// it is the one a reader can grep for.
    pub fn note_ast_inputs(&mut self, paths: &[PathBuf], sizes: &[usize]) {
        self.ast_labels = paths
            .iter()
            .map(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string());
                let name = name.unwrap_or_else(|| p.display().to_string());
                name.strip_suffix(".ast.json").unwrap_or(&name).to_string()
            })
            .collect();
        self.ast_total_bytes = sizes.iter().map(|n| *n as u64).sum();
    }

    /// The one line printed before the long part starts.
    ///
    /// It states the scale up front -- how many dumps and how many
    /// megabytes -- because that is the explanation for the pause that
    /// follows, and a reader who sees it does not need the timings to
    /// understand where the time went.
    pub fn header(&mut self, entries: usize, origins: usize, resolved_bytes: usize) {
        if self.quiet() {
            return;
        }
        let out = std::io::stdout();
        let mut out = out.lock();
        let ast = if self.ast_labels.is_empty() {
            "no AST dumps".to_string()
        } else {
            format!("{} AST dumps, {}", self.ast_labels.len(), human_bytes(self.ast_total_bytes))
        };
        let _ = writeln!(
            out,
            "oz_static: {} entry source{}, {} origins, {} resolved; {}",
            entries,
            if entries == 1 { "" } else { "s" },
            origins,
            human_bytes(resolved_bytes as u64),
            ast
        );
    }

    /// Close the running span and print the summary, plus the table under
    /// `--timings`.
    ///
    /// `listed_only` distinguishes a `--manifest-only` run, which computes
    /// the file list without writing any of it. Saying "generated" there
    /// would be a plain lie about what is on disk, and the configure-time
    /// build step is exactly where someone reads this line while wondering
    /// why the transpiler appears to run twice.
    pub fn finish(&mut self, written: usize, outdir: &Path, listed_only: bool) {
        self.close_span();
        let elapsed = self.started.elapsed();

        /* Quiet reproduces the pre-#299 line, on stderr, byte for byte --
         * that is the contract anything scraping our output relies on. */
        if self.quiet() {
            if listed_only {
                eprintln!("oz_static: {} files listed for {}", written, outdir.display());
            } else {
                eprintln!("oz_static: {} files generated in {}", written, outdir.display());
            }
            return;
        }

        let out = std::io::stdout();
        let mut out = out.lock();
        let _ = writeln!(
            out,
            "oz_static: {} files {} {} ({:.2}s)",
            written,
            if listed_only { "listed for" } else { "generated in" },
            outdir.display(),
            elapsed.as_secs_f64()
        );

        if self.level != Level::Timings {
            return;
        }
        /* "spans, not a partition": the phases main.rs reports bracket the
         * ones the library reports, and neither covers process startup, so
         * the rows do not sum to the wall clock and must not be read as if
         * they did. */
        let _ = writeln!(
            out,
            "oz_static: timings (of {:.2}s wall; rows are spans, not a partition)",
            elapsed.as_secs_f64()
        );
        let wall = elapsed.as_secs_f64();
        for (phase, took) in &self.spans {
            let secs = took.as_secs_f64();
            let share = if wall > 0.0 { secs / wall * 100.0 } else { 0.0 };
            let note = if *phase == Phase::AstIngest && !self.ast_labels.is_empty() {
                format!("   ({} dumps, {})", self.ast_labels.len(), human_bytes(self.ast_total_bytes))
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "oz_static:   {:<16} {:>7.2}s   {:>5.1}%{}",
                phase.label(),
                secs,
                share,
                note
            );
        }
    }

    fn close_span(&mut self) {
        if let Some(phase) = self.current.take() {
            let took = self.phase_at.elapsed();
            /* Summed rather than appended if a phase is somehow entered
             * twice, so one row per phase either way. */
            match self.spans.iter_mut().find(|(p, _)| *p == phase) {
                Some((_, total)) => *total += took,
                None => self.spans.push((phase, took)),
            }
        }
    }
}

impl Observer for Reporter {
    fn enter(&mut self, phase: Phase) {
        self.close_span();
        self.current = Some(phase);
        self.phase_at = Instant::now();
        if phase == Phase::AstIngest {
            self.ast_at = Instant::now();
        }
    }

    fn ast_dump(&mut self, index: usize, json_bytes: usize) {
        self.ast_seen += 1;
        let took = self.ast_at.elapsed();
        self.ast_at = Instant::now();
        if self.quiet() {
            return;
        }
        let label = self.ast_labels.get(index).map(String::as_str).unwrap_or("<unknown>");
        let out = std::io::stdout();
        let mut out = out.lock();
        /* The x/N counter is the point of this line: it turns a ten-second
         * stall into a visible rate. */
        let counter = format!("{:>2}/{}", self.ast_seen, self.ast_labels.len().max(self.ast_seen));
        if self.level == Level::Timings {
            let _ = writeln!(
                out,
                "oz_static: ast {}  {:<30} {:>9}   {:.2}s",
                counter,
                label,
                human_bytes(json_bytes as u64),
                took.as_secs_f64()
            );
        } else {
            let _ = writeln!(
                out,
                "oz_static: ast {}  {:<30} {:>9}",
                counter,
                label,
                human_bytes(json_bytes as u64)
            );
        }
    }
}

/// `41 KB`, `38.4 MB` -- enough precision to compare two dumps, not enough
/// to distract.
fn human_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let n = n as f64;
    if n >= MB {
        format!("{:.1} MB", n / MB)
    } else if n >= KB {
        format!("{:.0} KB", n / KB)
    } else {
        format!("{} B", n as u64)
    }
}
