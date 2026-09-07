// SPDX-License-Identifier: Apache-2.0
//
// imports.rs - OZ-094: resolves '#import' directives before the source
// ever reaches the core pipeline (parse -> collect -> emit). Kept
// deliberately separate from `transpile()`, which stays a pure,
// filesystem-free function every existing test relies on calling
// directly with a pre-assembled string -- only `main.rs` (and any
// future caller that actually has a real file on disk) needs this.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

enum ImportTarget {
    Quoted(String),
    Angled(String),
}

impl ImportTarget {
    fn spelled(&self) -> String {
        match self {
            ImportTarget::Quoted(p) => format!("\"{}\"", p),
            ImportTarget::Angled(p) => format!("<{}>", p),
        }
    }
}

/// `#import "X.h"` or `#import <Framework/X.h>`, plus a quoted
/// `#include "X.h"` -- see `resolve_into` for why the latter is a
/// candidate and when it is declined. `None` for anything else.
fn parse_import(trimmed_line: &str) -> Option<ImportTarget> {
    let rest = trimmed_line
        .strip_prefix("#import")
        .or_else(|| trimmed_line.strip_prefix("#include"))?
        .trim_start();
    if let Some(inner) = rest.strip_prefix('"') {
        let end = inner.find('"')?;
        return Some(ImportTarget::Quoted(inner[..end].to_string()));
    }
    if let Some(inner) = rest.strip_prefix('<') {
        let end = inner.find('>')?;
        return Some(ImportTarget::Angled(inner[..end].to_string()));
    }
    None
}

/// Does this file's own text carry Objective-C declarations?
fn declares_objc(text: &str) -> bool {
    text.contains("@interface") || text.contains("@implementation") || text.contains("@protocol")
}

/// Can `path` reach any Objective-C declaration -- in its own text, or
/// through anything it imports?
///
/// This is the test for whether a resolved header is spliced at all.
/// Splicing exists so the core pipeline sees the Objective-C it has to
/// transpile; a header that reaches none is pure C, has nothing to
/// transpile, and is already on the C compiler's own search path. Leaving
/// it as an ordinary `#include` is both sufficient and more faithful than
/// copying it into the output.
///
/// It has to be transitive, not just a look at the file's own text.
/// `include/oz_sdk/objc/objc.h` declares nothing itself -- its entire body
/// is `#import <Foundation/OZObject.h>` -- yet declining it would hide
/// every class behind it. `include/oz_sdk/assert.h` reaches nothing, and
/// splicing it is actively wrong: it is an AST-analysis shim whose own
/// comment says so ("Declares oz_assert functions so Clang preserves calls
/// in the AST. The generated C includes platform/oz_assert.h which
/// provides the real macros"), so copying its `static inline oz_assert_msg`
/// into a generated `assert.c` could not compile -- the PAL had already
/// made that name a function-like macro, and the definition came out as
/// "expected identifier or '('".
///
/// A file being visited is memoised as `false` before recursing, so an
/// import cycle terminates: a cycle cannot itself introduce a declaration,
/// so assuming "no" for the back-edge is safe, and the real answer for
/// each file is still whatever its own text and its other imports say.
fn reaches_objc(
    path: &Path,
    include_dirs: &[PathBuf],
    impl_dirs: &[PathBuf],
    memo: &mut HashMap<PathBuf, bool>,
) -> bool {
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(known) = memo.get(&key) {
        return *known;
    }
    memo.insert(key.clone(), false);

    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let mut answer = declares_objc(&text);
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();

    // A pure-C header whose sibling implementation carries the Objective-C
    // still has to be spliced -- the sibling is only ever reached through
    // its header (see `find_sibling_impl`), so declining the header would
    // drop the implementation with it.
    if !answer {
        if let Some(impl_path) = find_sibling_impl(path, impl_dirs) {
            if let Ok(impl_text) = fs::read_to_string(&impl_path) {
                answer = declares_objc(&impl_text);
            }
        }
    }

    if !answer {
        for line in text.lines() {
            let Some(target) = parse_import(line.trim_start()) else {
                continue;
            };
            let Ok(next) = resolve_import_path(&target, &dir, include_dirs, impl_dirs) else {
                continue;
            };
            if reaches_objc(&next, include_dirs, impl_dirs, memo) {
                answer = true;
                break;
            }
        }
    }

    memo.insert(key, answer);
    answer
}

/// `impl_dirs` are searched as well as `include_dirs`, because an import
/// target can name an implementation directly: the behavior corpus's
/// shared base header does `#import "OZObject.m"`, so that the oracle's
/// Clang pass sees a complete AST. Without searching there, every one of
/// those 73 cases fails to resolve -- `.m` files live in `src`, which is
/// an `--impl-dir`, not an `-I`. Include dirs are tried first, so a header
/// is still found where a header is expected.
fn resolve_import_path(
    target: &ImportTarget,
    current_dir: &Path,
    include_dirs: &[PathBuf],
    impl_dirs: &[PathBuf],
) -> Result<PathBuf, String> {
    let search: Vec<&PathBuf> = include_dirs.iter().chain(impl_dirs.iter()).collect();
    match target {
        ImportTarget::Quoted(p) => {
            let local = current_dir.join(p);
            if local.is_file() {
                return Ok(local);
            }
            for dir in &search {
                let candidate = dir.join(p);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
            Err(format!(
                "cannot resolve #import \"{}\" -- not found in '{}' or any of {} search dir(s)",
                p,
                current_dir.display(),
                search.len()
            ))
        }
        ImportTarget::Angled(p) => {
            for dir in &search {
                let candidate = dir.join(p);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
            Err(format!("cannot resolve #import <{}> in any of {} search dir(s)", p, search.len()))
        }
    }
}

/// Every angled `#include <...>` line in `source`, deduplicated, in
/// first-seen order.
///
/// These are the includes `resolve_imports` deliberately leaves alone (only
/// `#import` is a resolution candidate), and the companion header needs
/// them: it declares a prototype for every method of every class, so a
/// parameter type that came from a system or RTOS header -- a
/// `struct k_timer` from `#include <zephyr/kernel.h>`, say -- is otherwise
/// named there before anything has declared it. C then invents a
/// prototype-scoped tag, and the class's own header, which does carry the
/// include, declares the same function with the real type, giving
/// `error: conflicting types for '...'`. The oracle propagates the include
/// for the same reason.
///
/// The case this was found on was OZTimer, retired in #267; the rule is
/// general and is pinned by `tests/import_resolution.rs` rather than by
/// that class.
///
/// Angled only, deliberately. An angled include resolves against the
/// compiler's include path, which is identical for the companion header
/// and for every other generated file, so copying one is always safe. A
/// quoted `#include "X.h"` resolves relative to the *original* source's
/// directory, which the companion header does not share, so copying it
/// could turn a working build into an unresolvable path; and a quoted
/// Objective-C header would have been an `#import`, already spliced in by
/// then. A quoted include that does define a prototype's type therefore
/// still fails -- but loudly, at the C compiler, not as wrong code.
///
/// Propagating the include is preferred over forward-declaring the struct
/// tag, which would fix this instance more narrowly but only this kind:
/// a forward declaration is not enough for a parameter passed by value,
/// nor for a typedef, enum, or macro the same header supplies.
pub fn collect_system_includes(source: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("#include") else {
            continue;
        };
        let rest = rest.trim_start();
        if !rest.starts_with('<') {
            continue;
        }
        let Some(end) = rest.find('>') else {
            continue;
        };
        let path = &rest[1..end];
        if seen.insert(path.to_string()) {
            out.push(format!("#include <{}>", path));
        }
    }
    out
}

/// A header's sibling implementation -- same basename, `.m` extension,
/// in one of `impl_dirs` -- if one exists. Without it, a class's own
/// real method bodies (e.g. `OZObject`'s `-init`) would be declared via
/// the header but never defined, trading an undefined-superclass
/// diagnostic for an undefined-symbol error at link time instead.
fn find_sibling_impl(header_path: &Path, impl_dirs: &[PathBuf]) -> Option<PathBuf> {
    let stem = header_path.file_stem()?.to_str()?;
    for dir in impl_dirs {
        let candidate = dir.join(format!("{}.m", stem));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Unwrap a `#ifdef __clang__` / ... / `#endif` guard to just its
/// middle line(s) -- several real headers wrap their
/// `@compatibility_alias` line in this (a compiler-portability check
/// meaningless once inlined directly, since oz_static has no
/// `#import`/`#include` resolution of its own to have made the
/// `@compatibility_alias` necessary in the first place). oz_static's
/// top-level emit pass elides a bare `compatibility_alias_declaration`
/// to a comment, but doesn't recurse into `#ifdef`/`#endif`
/// conditionals to find one nested inside, so left wrapped it would
/// pass through as invalid raw ObjC text. Shared with
/// `tests/common/mod.rs`'s hand-assembled fixtures, which hit the exact
/// same headers.
pub fn unwrap_clang_guard(src: &str) -> String {
    unwrap_clang_guard_tracked(src).0
}

/// `unwrap_clang_guard`, plus the 1-based line numbers of `src` it
/// dropped, ascending.
///
/// Deleting a line renumbers every line after it, so a caller mapping the
/// unwrapped text back to the file on disk (#305's source map) cannot use
/// its own iteration index as a file line number. This is what
/// `original_line` walks to undo the shift; it is two entries per guard,
/// so `resolve_into` carries a `&[usize]` and not a per-line table.
pub fn unwrap_clang_guard_tracked(src: &str) -> (String, Vec<usize>) {
    let mut out = String::new();
    let mut dropped = Vec::new();
    let mut skip_next_endif = false;
    for (idx, line) in src.lines().enumerate() {
        let t = line.trim();
        if t == "#ifdef __clang__" {
            skip_next_endif = true;
            dropped.push(idx + 1);
            continue;
        }
        if skip_next_endif && t == "#endif" {
            skip_next_endif = false;
            dropped.push(idx + 1);
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    (out, dropped)
}

/// The line of the file on disk that line `line` of an unwrapped text came
/// from, given the ascending line numbers `unwrap_clang_guard_tracked`
/// dropped.
///
/// Each dropped line at or before the answer pushes it down by one, and
/// the answer only grows, so one forward pass over the (tiny) list is
/// exact: with a `#ifdef __clang__` on file line 3 and its `#endif` on
/// line 5, unwrapped line 3 is file line 4 and unwrapped line 4 is file
/// line 6.
fn original_line(dropped: &[usize], line: usize) -> usize {
    let mut original = line;
    for &d in dropped {
        if d <= original {
            original += 1;
        } else {
            break;
        }
    }
    original
}

/// One run of merged text whose lines advance in step with one source
/// file's lines. The line at `merged_start` is line `source_line` of
/// `files[file]`, and each merged line after it is that file's next line,
/// until the next segment starts.
#[derive(Debug, Clone, Copy)]
struct Segment {
    merged_start: usize,
    file: usize,
    source_line: usize,
}

/// Where a byte of the merged buffer came from: which `.m`/`.h` on disk,
/// and which line of it (#305).
///
/// Splicing is not a copy. `resolve_into` drops `#pragma once`, replaces
/// an `#import` line with a whole file, deletes `#ifdef __clang__` guards
/// and pushes separator newlines, so a merged line number is not a source
/// line number and the delta varies *within* one file. Every one of those
/// sites is a point where the mapping shifts and nothing else is, so the
/// map is a **sorted `Vec` of segments** -- one per shift, found by binary
/// search -- rather than a per-line table of (file, line).
///
/// `line_starts` is the one per-line thing here: the newline index of the
/// merged buffer, recorded once by `index_lines` when resolution finishes.
/// It costs 8 bytes per merged line (~26 KB for px-keyboard's 3 300-line
/// buffer) and buys two things a segment table alone cannot. The lookup is
/// `O(log n)` instead of a scan from the segment's start, which matters to
/// a caller asking once per emitted statement; and `source_location` needs
/// no text argument, so no caller can pass the *wrong* text. That is a
/// live hazard, not a hypothetical one:
/// `parse::repair_bare_macro_statements` is offset-preserving but **not**
/// line-preserving -- the whitespace byte it overwrites with `;` is
/// normally the macro line's own `\n` (see
/// `first_bare_macro_semicolon_slot`, which requires a newline in the
/// gap), so the repaired text `emit` works on has *fewer* lines than the
/// merged text this map describes while every byte offset still agrees.
/// Resolving lines against the buffer as spliced removes the question.
#[derive(Debug, Default)]
pub struct SourceMap {
    files: Vec<PathBuf>,
    /// Ascending by `merged_start`, by construction: a segment is pushed
    /// as the merged buffer grows.
    segments: Vec<Segment>,
    /// Byte offset of the start of each line of the merged text, so line 1
    /// starts at `line_starts[0]`.
    line_starts: Vec<usize>,
    /// Length of the merged text the lines were indexed from, so an offset
    /// past its end answers `None` rather than the last line seen.
    text_len: usize,
}

impl SourceMap {
    /// The file, and the 1-based line of it, that the byte at
    /// `merged_offset` was written from. `None` if the offset is not
    /// covered: past the end of the merged text, or before the first
    /// recorded segment.
    ///
    /// `merged_offset` is an offset into `ResolvedSource::text`. Offsets
    /// into `parse::repair_bare_macro_statements`'s output are the same
    /// offsets -- that repair overwrites a byte in place and never moves
    /// one -- so an emitter keyed on the repaired text can pass its own
    /// offsets straight in.
    pub fn source_location(&self, merged_offset: usize) -> Option<(&Path, usize)> {
        let merged_line = self.merged_line(merged_offset)?;
        let after = self.segments.partition_point(|s| s.merged_start <= merged_offset);
        let seg = self.segments[..after].last()?;
        /* Every segment starts at a line boundary, so this is exact and
         * not a rounding of one. */
        let seg_line = self.merged_line(seg.merged_start)?;
        Some((self.files[seg.file].as_path(), seg.source_line + (merged_line - seg_line)))
    }

    /// How many segments the map holds. For tests and diagnostics: the
    /// point of the segment shape is that this stays far below the merged
    /// buffer's line count.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// 1-based line of the merged buffer holding `merged_offset`.
    fn merged_line(&self, merged_offset: usize) -> Option<usize> {
        if self.line_starts.is_empty() || merged_offset >= self.text_len {
            return None;
        }
        Some(self.line_starts.partition_point(|&start| start <= merged_offset))
    }

    fn file_index(&mut self, path: &Path) -> usize {
        if let Some(found) = self.files.iter().position(|p| p == path) {
            return found;
        }
        self.files.push(path.to_path_buf());
        self.files.len() - 1
    }

    fn push_segment(&mut self, merged_start: usize, file: usize, source_line: usize) {
        debug_assert!(
            self.segments.last().map(|s| s.merged_start <= merged_start).unwrap_or(true),
            "segments must be recorded in merged-buffer order"
        );
        self.segments.push(Segment { merged_start, file, source_line });
    }

    /// Index the finished merged text's line starts. Called once, by
    /// `resolve_imports`/`resolve_entry_files`, after the last byte is
    /// spliced -- until then `source_location` has no lines to count and
    /// answers `None`.
    fn index_lines(&mut self, merged_text: &str) {
        self.line_starts.clear();
        self.text_len = merged_text.len();
        self.line_starts.push(0);
        for (i, b) in merged_text.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                self.line_starts.push(i + 1);
            }
        }
    }
}

/// The merged, `#import`-resolved source (`resolve_imports`'s result),
/// plus provenance: `origins` is an ordered list of `(stem, byte_range)`
/// covering every byte of `text` with no gaps -- the same `stem` can
/// appear more than once non-contiguously (e.g. the main file's own
/// lines before and after an `#import`). Consumed by OZ-096's per-origin
/// output split (`emit::emit_split`); `text` alone is exactly what
/// OZ-094 already produced, still usable directly with `transpile()`.
#[derive(Debug, Default)]
pub struct ResolvedSource {
    pub text: String,
    pub origins: Vec<(String, Range<usize>)>,
    /// Stems resolved from inside `include_dirs`/`impl_dirs` (the SDK's
    /// own Foundation headers/sources, as opposed to the caller's own
    /// project-local files) -- lets a caller mirror the Python
    /// pipeline's own `outdir/Foundation/` split (OZ-096) when writing
    /// per-origin output files.
    pub foundation_stems: HashSet<String>,
    /// Stems of spliced files that reach no Objective-C at all -- pure C
    /// pulled in by an `#import`. No output translation unit is written
    /// for these; see `reaches_objc` for why, and `main.rs` for where the
    /// pair is skipped.
    pub pure_c_stems: HashSet<String>,
    /// Byte ranges of `text` that came from a *header* rather than an
    /// implementation file.
    ///
    /// What a header holds is meant to be visible to every file that
    /// includes it, so pass-through C from one belongs in the generated
    /// header -- not in the generated `.c`, where only that one translation
    /// unit can see it. A bare top-level macro invocation is the case that
    /// forced this: `samples/zbus_service`'s header has
    /// `ZBUS_CHAN_DECLARE(chan_temperature_service_invoke, ...)`, which
    /// landed in the generated `.c` and left `main` with
    /// "'chan_temperature_service_report' undeclared". It is a common Zephyr
    /// shape (`LOG_MODULE_DECLARE`, `DEVICE_DT_DECLARE`).
    pub header_ranges: Vec<Range<usize>>,
    /// Merged offset -> (file on disk, line of that file), for #305's
    /// `#line` directives. See `SourceMap`.
    pub source_map: SourceMap,
    /// The file each `origins` stem was read from.
    ///
    /// `origins` is keyed on a stem, and so is everything downstream of it
    /// (the emit buckets, the output paths), because a stem is what names
    /// an output pair. A stem cannot name a *source file* though, which is
    /// what a `#line` directive needs, so the path is kept beside it
    /// rather than reconstructed later -- by then the include dirs it was
    /// found in are gone. One entry per stem; a stem reached twice (a
    /// header and its sibling `.m` share one, by design) keeps the first
    /// path recorded for it, which is the header. Per-*byte* provenance is
    /// `source_map`'s job and is exact.
    pub stem_paths: HashMap<String, PathBuf>,
}

impl ResolvedSource {
    /// The `.m`/`.h` on disk, and the 1-based line of it, that the byte at
    /// `merged_offset` of `text` was spliced from (#305).
    ///
    /// This is the seam a `#line`-emitting pass consumes: hand it a
    /// `start_byte()` and it hands back what a debugger has to be told.
    /// `None` means the offset is not covered by the map -- past the end
    /// of `text`, or from a resolution that recorded nothing.
    pub fn source_location(&self, merged_offset: usize) -> Option<(&Path, usize)> {
        self.source_map.source_location(merged_offset)
    }
}

/// Resolve every `#import` in `source` (as if read from a file in
/// `source_dir`, identified as `main_stem` in the returned provenance),
/// splicing each resolved header's content -- and, if one exists, its
/// sibling `.m` implementation -- in place of the `#import` line,
/// recursively (a resolved header may itself `#import` further headers,
/// resolved relative to *its own* directory). A header (or
/// implementation) already resolved earlier in the same run is elided
/// to a comment instead of spliced again -- the same effect as its own
/// `#pragma once`, since being pulled in by two different import paths
/// must not double-define its class. `#pragma once` itself is dropped
/// (meaningless once inlined). Plain `#include` lines are left
/// completely untouched.
///
/// `include_dirs` are searched, in order, for `#import <Framework/X.h>`
/// (mirroring `-I`); `impl_dirs` for a same-basename `.m` sibling of
/// any resolved header. Fails on the first `#import` that can't be
/// resolved or read -- there's no meaningful partial result once one
/// piece of the program is missing.
///
/// The caller passes text rather than a path, so the entry's own lines
/// have no real file for `source_map` to name: they are recorded against
/// `source_dir/<main_stem>.m`, the name that text would have had. Every
/// *spliced* file is a real path, read from disk, and
/// `resolve_entry_files` -- what the CLI drives -- has real paths
/// throughout.
pub fn resolve_imports(
    source: &str,
    source_dir: &Path,
    include_dirs: &[PathBuf],
    impl_dirs: &[PathBuf],
    main_stem: &str,
) -> Result<ResolvedSource, String> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut objc_memo = HashMap::new();
    let mut acc = ResolvedSource::default();
    let entry_path = source_dir.join(format!("{}.m", main_stem));
    resolve_into(
        &SpliceInput {
            text: source,
            dir: source_dir,
            stem: main_stem,
            path: &entry_path,
            dropped_lines: &[],
            // The entry text is the caller's own source, an implementation.
            is_header: false,
        },
        include_dirs,
        impl_dirs,
        &mut seen,
        &mut objc_memo,
        &mut acc,
    )?;
    acc.source_map.index_lines(&acc.text);
    Ok(acc)
}

/// `resolve_imports` for several entry `.m` files at once, merged into
/// one translation unit -- what a build system hands over, since a
/// sample's CMakeLists.txt lists every `.m` it owns (see
/// `cmake/oz_static.cmake`).
///
/// One translation unit rather than one run per file, because the whole
/// design is whole-program: `collect` rejects a class whose superclass it
/// cannot see, a category's methods merge into the class it extends, and
/// exactly one companion file carries the shared dispatch tables. Running
/// per file would break all three.
///
/// The `seen` set is shared across entries, so a file already pulled in
/// transitively by an earlier entry (`main.m` importing `App.h`, whose
/// sibling `App.m` gets spliced with it) is not spliced again when its own
/// turn comes -- the same `#pragma once` effect `resolve_imports` already
/// applies within a single run. That makes the result independent of the
/// order the build system happens to list files in, and never silently
/// drops one it did list: an entry reachable transitively contributes
/// once, and one that isn't reachable at all still contributes.
///
/// An entry file's own stem is never recorded in `foundation_stems`:
/// these are the caller's project-local files by definition, even though
/// their directory is typically also an `impl_dir` (which is how sibling
/// `.m` lookup finds them). Only files reached *through* an `#import` get
/// classified, inside `resolve_into`.
pub fn resolve_entry_files(
    entry_paths: &[PathBuf],
    include_dirs: &[PathBuf],
    impl_dirs: &[PathBuf],
) -> Result<ResolvedSource, String> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut objc_memo = HashMap::new();
    let mut acc = ResolvedSource::default();

    for path in entry_paths {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(canonical) {
            continue;
        }
        let source = fs::read_to_string(path)
            .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("out").to_string();
        resolve_into(
            &SpliceInput {
                text: &source,
                dir: &dir,
                stem: &stem,
                // #305's first step: the path is kept, not reduced to
                // `stem` and dropped. A `#line` directive needs a file
                // name, and this is the only place one exists.
                path,
                dropped_lines: &[],
                // Entry files are the `.m` sources a build system lists.
                is_header: false,
            },
            include_dirs,
            impl_dirs,
            &mut seen,
            &mut objc_memo,
            &mut acc,
        )?;
    }
    acc.source_map.index_lines(&acc.text);
    Ok(acc)
}

/// One file's contribution to the merge, as `resolve_into` reads it.
struct SpliceInput<'a> {
    /// The file's text -- already `unwrap_clang_guard`ed if it is a
    /// spliced header, which is why `dropped_lines` exists.
    text: &'a str,
    /// The directory a quoted `#import` in `text` resolves against.
    dir: &'a Path,
    /// Stem this file's bytes are attributed to in `origins`.
    stem: &'a str,
    /// The file on disk, for `source_map` and `stem_paths`.
    path: &'a Path,
    /// 1-based lines of `path` that `text` no longer has, ascending --
    /// `unwrap_clang_guard_tracked`'s second return value. Empty when the
    /// text was not rewritten before splicing.
    dropped_lines: &'a [usize],
    is_header: bool,
}

/// Where in one file's lines the merge currently is (#305).
struct LineCursor {
    file_idx: usize,
    /// Source line of the last line written from this file, if any.
    prev_source_line: Option<usize>,
}

impl LineCursor {
    /// Open a `source_map` segment for the line about to be written, if
    /// the mapping shifted since the last one.
    ///
    /// It shifted whenever this line is not one past the last line written
    /// from this file: a dropped `#pragma once`, a `#ifdef __clang__`
    /// guard the unwrap removed, an `#import` line a splice consumed. The
    /// splice case is anchored by `push_separator` rather than detected
    /// here, because a splice moves the *merged* line on without moving
    /// the source line at all.
    fn begin_line(&mut self, acc: &mut ResolvedSource, source_line: usize) {
        if self.prev_source_line != source_line.checked_sub(1) {
            let at = acc.text.len();
            acc.source_map.push_segment(at, self.file_idx, source_line);
        }
        self.prev_source_line = Some(source_line);
    }

    /// The newline that keeps a spliced file from running into whatever
    /// follows it. No source file has that line, so it is attributed to
    /// the `#import` line that caused the splice -- which also leaves this
    /// file's *next* line falling out of the same segment at the right
    /// number, since it really is one line further on.
    fn push_separator(&mut self, acc: &mut ResolvedSource, import_line: usize) {
        let at = acc.text.len();
        acc.source_map.push_segment(at, self.file_idx, import_line);
        acc.text.push('\n');
        self.prev_source_line = Some(import_line);
    }
}

/// Writes into the single, shared `acc.text` buffer (rather than building
/// and returning its own local `String`, the way an earlier version of
/// this function did) specifically so that `acc.text.len()` at any point
/// during the whole recursion is a true global byte offset into the final
/// merged text -- the only way to record `origins` ranges that are
/// still valid once every recursive call has finished contributing its
/// own piece. `acc.source_map`'s segment offsets are global for the same
/// reason.
///
/// The whole result is the accumulator, rather than one `&mut` per field,
/// because #305 gave it two more fields to carry and thirteen parameters
/// was already over the line.
fn resolve_into(
    input: &SpliceInput,
    include_dirs: &[PathBuf],
    impl_dirs: &[PathBuf],
    seen: &mut HashSet<PathBuf>,
    objc_memo: &mut HashMap<PathBuf, bool>,
    acc: &mut ResolvedSource,
) -> Result<(), String> {
    let SpliceInput { text: source, dir: current_dir, stem, path, dropped_lines, is_header } =
        *input;
    acc.stem_paths.entry(stem.to_string()).or_insert_with(|| path.to_path_buf());
    let mut cursor =
        LineCursor { file_idx: acc.source_map.file_index(path), prev_source_line: None };
    let mut run_start = acc.text.len();
    for (idx, line) in source.lines().enumerate() {
        /* This file's own line number, which is what a `#line` directive
         * has to say -- not `idx`, which counts the text as the merge
         * reads it, guard lines already removed. */
        let source_line = original_line(dropped_lines, idx + 1);
        let trimmed = line.trim_start();
        if trimmed.starts_with("#pragma once") {
            /* Dropped: a line of the file the merged buffer does not have.
             * `begin_line` sees the gap when the next line arrives. */
            continue;
        }
        let Some(target) = parse_import(trimmed) else {
            cursor.begin_line(acc, source_line);
            acc.text.push_str(line);
            acc.text.push('\n');
            continue;
        };
        // `#include "X.h"` is a resolution candidate alongside `#import`,
        // not just `#import`. `samples/zbus_objc`'s `Producer.m` opens with
        // `#include "Producer.h"` -- an ordinary C spelling for a header
        // holding an `@interface`, and until this existed the `@property`
        // in it was never seen, so its own `@synthesize` failed with
        // "'@synthesize count' but no '@property count' is declared".
        // Objective-C draws no semantic line between the two directives
        // here; only `#import`'s once-only behaviour differs, and the
        // seen-set below gives that to both.
        //
        // A `#include` that cannot be resolved at all stays exactly as
        // written, rather than failing the build the way an unresolvable
        // `#import` does -- a `#include` may legitimately name something
        // only the target's own toolchain provides.
        let is_include = trimmed.starts_with("#include");
        let resolved_path = match resolve_import_path(&target, current_dir, include_dirs, impl_dirs)
        {
            Ok(resolved) => resolved,
            Err(why) => {
                if is_include {
                    cursor.begin_line(acc, source_line);
                    acc.text.push_str(line);
                    acc.text.push('\n');
                    continue;
                }
                return Err(why);
            }
        };
        let carries_objc = reaches_objc(&resolved_path, include_dirs, impl_dirs, objc_memo);
        // A quoted `#include` that reaches no Objective-C is left exactly
        // as written: it is pure C, the C compiler resolves it the same
        // way it always did, and taking it over would only move work that
        // was never oz_static's. An `#import` is always spliced, whatever
        // it reaches -- it is Objective-C's own directive, and its target
        // may still be needed for the chain (`oz_sdk/objc/objc.h` declares
        // nothing itself; its whole body is `#import
        // <Foundation/OZObject.h>`).
        if is_include && !carries_objc {
            cursor.begin_line(acc, source_line);
            acc.text.push_str(line);
            acc.text.push('\n');
            continue;
        }
        let canonical = resolved_path.canonicalize().unwrap_or_else(|_| resolved_path.clone());
        if !seen.insert(canonical) {
            /* One line in for one line out, so the mapping stays in step:
             * this comment stands where the `#import` stood. */
            cursor.begin_line(acc, source_line);
            acc.text.push_str(&format!("/* already resolved: #import {} */\n", target.spelled()));
            continue;
        }

        // Flush `stem`'s own run so far -- everything from here until
        // the resolved file's own recursive call returns belongs to
        // *its* stem, not this one.
        if acc.text.len() > run_start {
            acc.origins.push((stem.to_string(), run_start..acc.text.len()));
            if is_header {
                acc.header_ranges.push(run_start..acc.text.len());
            }
        }

        let header_text = fs::read_to_string(&resolved_path)
            .map_err(|e| format!("cannot read '{}': {}", resolved_path.display(), e))?;
        let header_dir = resolved_path.parent().unwrap_or(current_dir).to_path_buf();
        let header_stem =
            resolved_path.file_stem().and_then(|s| s.to_str()).unwrap_or("import").to_string();
        if include_dirs.iter().chain(impl_dirs.iter()).any(|d| resolved_path.starts_with(d)) {
            acc.foundation_stems.insert(header_stem.clone());
        }
        if !carries_objc {
            acc.pure_c_stems.insert(header_stem.clone());
        }
        /* Tracked, not plain: the guard's own two lines are gone from the
         * text the recursion iterates, so without the dropped list every
         * line after a `#ifdef __clang__` would be attributed one or two
         * lines short of where it really is. */
        let (guarded_text, dropped) = unwrap_clang_guard_tracked(&header_text);
        resolve_into(
            &SpliceInput {
                text: &guarded_text,
                dir: &header_dir,
                stem: &header_stem,
                path: &resolved_path,
                dropped_lines: &dropped,
                // A `.m` reached through an `#import` is an implementation,
                // not a header -- the behaviour corpus's base header does
                // `#import "OZObject.m"` precisely to pull one in.
                is_header: resolved_path.extension().and_then(|e| e.to_str()) != Some("m"),
            },
            include_dirs,
            impl_dirs,
            seen,
            objc_memo,
            acc,
        )?;
        cursor.push_separator(acc, source_line);

        if let Some(impl_path) = find_sibling_impl(&resolved_path, impl_dirs) {
            let impl_canonical = impl_path.canonicalize().unwrap_or_else(|_| impl_path.clone());
            if seen.insert(impl_canonical) {
                let impl_text = fs::read_to_string(&impl_path)
                    .map_err(|e| format!("cannot read '{}': {}", impl_path.display(), e))?;
                let impl_dir = impl_path.parent().unwrap_or(current_dir).to_path_buf();
                resolve_into(
                    &SpliceInput {
                        text: &impl_text,
                        dir: &impl_dir,
                        // Same stem as its header -- one file pair, one
                        // origin. Its own path, though: `source_map` is
                        // per byte, and these bytes are the `.m`'s.
                        stem: &header_stem,
                        path: &impl_path,
                        dropped_lines: &[],
                        is_header: false,
                    },
                    include_dirs,
                    impl_dirs,
                    seen,
                    objc_memo,
                    acc,
                )?;
                cursor.push_separator(acc, source_line);
            }
        }

        run_start = acc.text.len();
    }
    if acc.text.len() > run_start {
        acc.origins.push((stem.to_string(), run_start..acc.text.len()));
        if is_header {
            acc.header_ranges.push(run_start..acc.text.len());
        }
    }
    Ok(())
}
