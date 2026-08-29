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

/// Resolve every `#import` in `source` (as if read from a file in
/// `source_dir`), splicing each resolved header's content -- and, if
/// one exists, its sibling `.m` implementation -- in place of the
/// `#import` line, recursively (a resolved header may itself `#import`
/// further headers, resolved relative to *its own* directory). A
/// header (or implementation) already resolved earlier in the same run
/// is elided to a comment instead of spliced again -- the same effect
/// as its own `#pragma once`, since being pulled in by two different
/// import paths must not double-define its class. `#pragma once`
/// itself is dropped (meaningless once inlined). Plain `#include`
/// lines are left completely untouched.
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
) -> Result<String, String> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    resolve(source, source_dir, include_dirs, impl_dirs, &mut seen)
}

fn resolve(
    source: &str,
    current_dir: &Path,
    include_dirs: &[PathBuf],
    impl_dirs: &[PathBuf],
    seen: &mut HashSet<PathBuf>,
) -> Result<String, String> {
    let mut out = String::new();
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

        let header_text = fs::read_to_string(&resolved_path)
            .map_err(|e| format!("cannot read '{}': {}", resolved_path.display(), e))?;
        let header_dir = resolved_path.parent().unwrap_or(current_dir).to_path_buf();
        let expanded_header =
            resolve(&unwrap_clang_guard(&header_text), &header_dir, include_dirs, impl_dirs, seen)?;
        out.push_str(&expanded_header);
        out.push('\n');

        if let Some(impl_path) = find_sibling_impl(&resolved_path, impl_dirs) {
            let impl_canonical = impl_path.canonicalize().unwrap_or_else(|_| impl_path.clone());
            if seen.insert(impl_canonical) {
                let impl_text = fs::read_to_string(&impl_path)
                    .map_err(|e| format!("cannot read '{}': {}", impl_path.display(), e))?;
                let impl_dir = impl_path.parent().unwrap_or(current_dir).to_path_buf();
                let expanded_impl = resolve(&impl_text, &impl_dir, include_dirs, impl_dirs, seen)?;
                out.push_str(&expanded_impl);
                out.push('\n');
            }
        }
    }
    Ok(out)
}
