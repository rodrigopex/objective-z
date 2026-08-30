// SPDX-License-Identifier: Apache-2.0
//
// imports.rs - OZ-094: resolves '#import' directives before the source
// ever reaches the core pipeline (parse -> collect -> emit). Kept
// deliberately separate from `transpile()`, which stays a pure,
// filesystem-free function every existing test relies on calling
// directly with a pre-assembled string -- only `main.rs` (and any
// future caller that actually has a real file on disk) needs this.

use std::collections::HashSet;
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

/// `#import "X.h"` or `#import <Framework/X.h>` -- `None` for anything
/// else (including a plain `#include`, which is never a resolution
/// candidate: only `#import` is, matching this codebase's own
/// convention of what needs inlining vs. what's a real system/RTOS
/// header left untouched).
fn parse_import(trimmed_line: &str) -> Option<ImportTarget> {
    let rest = trimmed_line.strip_prefix("#import")?.trim_start();
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

fn resolve_import_path(target: &ImportTarget, current_dir: &Path, include_dirs: &[PathBuf]) -> Result<PathBuf, String> {
    match target {
        ImportTarget::Quoted(p) => {
            let local = current_dir.join(p);
            if local.is_file() {
                return Ok(local);
            }
            for dir in include_dirs {
                let candidate = dir.join(p);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
            Err(format!(
                "cannot resolve #import \"{}\" -- not found in '{}' or any of {} include dir(s)",
                p,
                current_dir.display(),
                include_dirs.len()
            ))
        }
        ImportTarget::Angled(p) => {
            for dir in include_dirs {
                let candidate = dir.join(p);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
            Err(format!("cannot resolve #import <{}> in any of {} include dir(s)", p, include_dirs.len()))
        }
    }
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
    let mut out = String::new();
    let mut skip_next_endif = false;
    for line in src.lines() {
        let t = line.trim();
        if t == "#ifdef __clang__" {
            skip_next_endif = true;
            continue;
        }
        if skip_next_endif && t == "#endif" {
            skip_next_endif = false;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The merged, `#import`-resolved source (`resolve_imports`'s result),
/// plus provenance: `origins` is an ordered list of `(stem, byte_range)`
/// covering every byte of `text` with no gaps -- the same `stem` can
/// appear more than once non-contiguously (e.g. the main file's own
/// lines before and after an `#import`). Consumed by OZ-096's per-origin
/// output split (`emit::emit_split`); `text` alone is exactly what
/// OZ-094 already produced, still usable directly with `transpile()`.
#[derive(Debug)]
pub struct ResolvedSource {
    pub text: String,
    pub origins: Vec<(String, Range<usize>)>,
    /// Stems resolved from inside `include_dirs`/`impl_dirs` (the SDK's
    /// own Foundation headers/sources, as opposed to the caller's own
    /// project-local files) -- lets a caller mirror the Python
    /// pipeline's own `outdir/Foundation/` split (OZ-096) when writing
    /// per-origin output files.
    pub foundation_stems: HashSet<String>,
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
pub fn resolve_imports(
    source: &str,
    source_dir: &Path,
    include_dirs: &[PathBuf],
    impl_dirs: &[PathBuf],
    main_stem: &str,
) -> Result<ResolvedSource, String> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut text = String::new();
    let mut origins = Vec::new();
    let mut foundation_stems = HashSet::new();
    resolve_into(
        source,
        source_dir,
        include_dirs,
        impl_dirs,
        &mut seen,
        main_stem,
        &mut text,
        &mut origins,
        &mut foundation_stems,
    )?;
    Ok(ResolvedSource { text, origins, foundation_stems })
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
    let mut text = String::new();
    let mut origins = Vec::new();
    let mut foundation_stems = HashSet::new();

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
            &source,
            &dir,
            include_dirs,
            impl_dirs,
            &mut seen,
            &stem,
            &mut text,
            &mut origins,
            &mut foundation_stems,
        )?;
    }
    Ok(ResolvedSource { text, origins, foundation_stems })
}

/// Writes into the single, shared `out` buffer (rather than building and
/// returning its own local `String`, the way an earlier version of this
/// function did) specifically so that `out.len()` at any point during
/// the whole recursion is a true global byte offset into the final
/// merged text -- the only way to record `origins` ranges that are
/// still valid once every recursive call has finished contributing its
/// own piece.
#[allow(clippy::too_many_arguments)]
fn resolve_into(
    source: &str,
    current_dir: &Path,
    include_dirs: &[PathBuf],
    impl_dirs: &[PathBuf],
    seen: &mut HashSet<PathBuf>,
    stem: &str,
    out: &mut String,
    origins: &mut Vec<(String, Range<usize>)>,
    foundation_stems: &mut HashSet<String>,
) -> Result<(), String> {
    let mut run_start = out.len();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#pragma once") {
            continue;
        }
        let Some(target) = parse_import(trimmed) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let resolved_path = resolve_import_path(&target, current_dir, include_dirs)?;
        let canonical = resolved_path.canonicalize().unwrap_or_else(|_| resolved_path.clone());
        if !seen.insert(canonical) {
            out.push_str(&format!("/* already resolved: #import {} */\n", target.spelled()));
            continue;
        }

        // Flush `stem`'s own run so far -- everything from here until
        // the resolved file's own recursive call returns belongs to
        // *its* stem, not this one.
        if out.len() > run_start {
            origins.push((stem.to_string(), run_start..out.len()));
        }

        let header_text = fs::read_to_string(&resolved_path)
            .map_err(|e| format!("cannot read '{}': {}", resolved_path.display(), e))?;
        let header_dir = resolved_path.parent().unwrap_or(current_dir).to_path_buf();
        let header_stem =
            resolved_path.file_stem().and_then(|s| s.to_str()).unwrap_or("import").to_string();
        if include_dirs.iter().chain(impl_dirs.iter()).any(|d| resolved_path.starts_with(d)) {
            foundation_stems.insert(header_stem.clone());
        }
        resolve_into(
            &unwrap_clang_guard(&header_text),
            &header_dir,
            include_dirs,
            impl_dirs,
            seen,
            &header_stem,
            out,
            origins,
            foundation_stems,
        )?;
        out.push('\n');

        if let Some(impl_path) = find_sibling_impl(&resolved_path, impl_dirs) {
            let impl_canonical = impl_path.canonicalize().unwrap_or_else(|_| impl_path.clone());
            if seen.insert(impl_canonical) {
                let impl_text = fs::read_to_string(&impl_path)
                    .map_err(|e| format!("cannot read '{}': {}", impl_path.display(), e))?;
                let impl_dir = impl_path.parent().unwrap_or(current_dir).to_path_buf();
                // Same stem as its header -- one file pair, one origin.
                resolve_into(
                    &impl_text,
                    &impl_dir,
                    include_dirs,
                    impl_dirs,
                    seen,
                    &header_stem,
                    out,
                    origins,
                    foundation_stems,
                )?;
                out.push('\n');
            }
        }

        run_start = out.len();
    }
    if out.len() > run_start {
        origins.push((stem.to_string(), run_start..out.len()));
    }
    Ok(())
}
