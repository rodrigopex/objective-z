// no_dead_ivars.rs -- every ivar the Foundation SDK declares must be
// touched by the SDK's own implementation.
//
// Written because two were not, and neither was found by anyone reading
// the headers: `OZString._hash` sat between `_length` and `_data` looking
// exactly like them, and `OZObject._refcount` looks like the refcount --
// the real one is `oz_refcount`, synthesized by `companion.rs` into the
// same struct (#371). A declared ivar becomes a field in the generated
// struct whether or not anything reads it, so a dead one is pure
// footprint: `_refcount` cost 4 bytes in *every object of every class*,
// `OZObject` being the root.
//
// Two traps this test exists to avoid repeating:
//
//   - **Substring matching lies.** A plain `grep _refcount` matches
//     `oz_refcount`, which made the dead ivar look used. Matching here is
//     on whole identifier tokens.
//   - **A comment is not a use.** `emit.rs` carries "`_refcount` stays a
//     sibling", which is about the synthesized field; that is why only
//     `src/` and `include/` are searched, and why the search is for the
//     token rather than the prose.
//
// Scope is deliberately the SDK's own sources. An ivar a Foundation class
// declares is private to that class, so the methods that use it live in
// `src/*.m`; if nothing there and nothing in another SDK header touches
// it, no amount of user code can have made it live. Widening the search
// to `tests/` would also have to exclude `tests/zephyr/generated/`, whose
// committed output *contains* the very field being asked about -- a dead
// ivar would then prove itself alive.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn repo(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

/// Every file under `dir`, recursively, skipping the trees that are kept
/// for reference rather than compiled.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for e in entries.flatten() {
                let p = e.path();
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name == "runtime_legacy" || name == "objc-reference" {
                        continue;
                }
                if p.is_dir() {
                        sources(&p, out);
                } else if matches!(
                        p.extension().and_then(|s| s.to_str()),
                        Some("m") | Some("h") | Some("c")
                ) {
                        out.push(p);
                }
        }
}

/// The ivars declared in one header, as (class, ivar) pairs.
///
/// An ivar block is the `{ ... }` between `@interface` and the first
/// member declaration. Parsed by hand rather than with a regex crate:
/// oz2c is a build tool and this is not worth a dependency.
fn ivars_in(text: &str) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(at) = rest.find("@interface") {
                rest = &rest[at + "@interface".len()..];
                let head_end = rest.find('{');
                let end = rest.find("@end").unwrap_or(rest.len());
                let Some(open) = head_end else { continue };
                if open > end {
                        /* no ivar block before this interface ends */
                        continue;
                }
                let class = rest[..open]
                        .split_whitespace()
                        .next()
                        .unwrap_or("?")
                        .trim_start_matches('(')
                        .to_string();
                let Some(close) = rest[open..].find('}') else { continue };
                let body = &rest[open + 1..open + close];
                for line in body.lines() {
                        let line = line.split("/*").next().unwrap_or("").trim();
                        let Some(decl) = line.strip_suffix(';') else { continue };
                        let Some(name) = decl.split_whitespace().last() else { continue };
                        let name = name.trim_start_matches('*');
                        if name.starts_with('_') {
                                found.push((class.clone(), name.to_string()));
                        }
                }
                rest = &rest[open + close..];
        }
        found
}

/// Occurrences of `ident` as a whole identifier token, so `_refcount`
/// does not match inside `oz_refcount`.
fn token_count(text: &str, ident: &str) -> usize {
        let bytes = text.as_bytes();
        let mut n = 0;
        let mut from = 0;
        while let Some(rel) = text[from..].find(ident) {
                let start = from + rel;
                let end = start + ident.len();
                let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
                let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
                if before_ok && after_ok {
                        n += 1;
                }
                from = end;
        }
        n
}

fn is_ident_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
}

#[test]
fn every_declared_foundation_ivar_is_used() {
        let foundation = repo("include/oz_sdk/Foundation");
        let mut headers: Vec<PathBuf> = fs::read_dir(&foundation)
                .expect("Foundation headers must be readable")
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("h"))
                .collect();
        headers.sort();
        assert!(!headers.is_empty(), "no Foundation headers found at {:?}", foundation);

        let mut declared: Vec<(String, String, PathBuf)> = Vec::new();
        for h in &headers {
                let text = fs::read_to_string(h).expect("header must be readable");
                for (class, ivar) in ivars_in(&text) {
                        declared.push((class, ivar, h.clone()));
                }
        }
        assert!(
                declared.len() >= 15,
                "parsed only {} ivars, so the parser is probably broken rather than \
                 the headers being empty",
                declared.len()
        );

        let mut files = Vec::new();
        sources(&repo("src"), &mut files);
        sources(&repo("include"), &mut files);
        let corpus: BTreeMap<PathBuf, String> = files
                .into_iter()
                .filter_map(|p| fs::read_to_string(&p).ok().map(|t| (p, t)))
                .collect();

        let mut dead = Vec::new();
        for (class, ivar, header) in &declared {
                /* The declaration itself is one occurrence, so a live ivar
                 * has at least two. */
                let total: usize = corpus.values().map(|t| token_count(t, ivar)).sum();
                if total < 2 {
                        dead.push(format!(
                                "  {}.{} -- declared in {} and referenced nowhere else",
                                class,
                                ivar,
                                header.file_name().unwrap().to_string_lossy()
                        ));
                }
        }

        assert!(
                dead.is_empty(),
                "these ivars are declared but never used, and each one is a field in \
                 every instance of its class:\n{}\n\nEither use it or delete it. A \
                 declared ivar is not free: it is emitted into the generated struct \
                 either way (#371).",
                dead.join("\n")
        );
}
