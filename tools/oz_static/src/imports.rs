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
/// parameter type that came from a system or RTOS header -- OZTimer's
/// `struct k_timer`, from `#include <zephyr/kernel.h>` -- is otherwise
/// named there before anything has declared it. C then invents a
/// prototype-scoped tag, and the class's own header, which does carry the
/// include, declares the same function with the real type: `error:
/// conflicting types for 'OZTimer_initWithUserData_expiry_stop_'`. The
/// oracle propagates the include for the same reason -- see its committed
/// `tests/zephyr/generated/OZTimer_ozh.h`, whose `#include
/// <zephyr/kernel.h>` sits ahead of the struct and prototypes.
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
    /// Stems of spliced files that reach no Objective-C at all -- pure C
    /// pulled in by an `#import`. No output translation unit is written
    /// for these; see `reaches_objc` for why, and `main.rs` for where the
    /// pair is skipped.
    pub pure_c_stems: HashSet<String>,
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
    let mut pure_c_stems = HashSet::new();
    let mut objc_memo = HashMap::new();
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
        &mut pure_c_stems,
        &mut objc_memo,
    )?;
    Ok(ResolvedSource { text, origins, foundation_stems, pure_c_stems })
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
    let mut pure_c_stems = HashSet::new();
    let mut objc_memo = HashMap::new();

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
            &mut pure_c_stems,
            &mut objc_memo,
        )?;
    }
    Ok(ResolvedSource { text, origins, foundation_stems, pure_c_stems })
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
    pure_c_stems: &mut HashSet<String>,
    objc_memo: &mut HashMap<PathBuf, bool>,
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
            Ok(path) => path,
            Err(why) => {
                if is_include {
                    out.push_str(line);
                    out.push('\n');
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
            out.push_str(line);
            out.push('\n');
            continue;
        }
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
        if !carries_objc {
            pure_c_stems.insert(header_stem.clone());
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
            pure_c_stems,
            objc_memo,
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
                    pure_c_stems,
                    objc_memo,
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
