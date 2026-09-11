// kconfig_options_declared.rs -- every `CONFIG_OBJZ_*` a compiled source or
// the build system names must be declared in the module's `Kconfig`.
//
// Written because one was not, and the `#ifndef` beside it is what made that
// invisible: `src/OZLog.c` carried
// `#ifndef CONFIG_OBJZ_LOG_BUFFER_SIZE / #define ... 128`, sized its stack
// buffer from the symbol, and `OZLog.h` documented the option to users --
// while `Kconfig` declared it nowhere (#420). The fallback makes the code
// *read* as configurable, so a maintainer greps the source, finds the symbol
// used and defaulted, and never asks the separate question of whether
// anything declares it.
//
// What the user gets from an undeclared option is not the ignored setting the
// phrase suggests. Zephyr's `kconfig.cmake` turns Kconfig warnings into
// `error: Aborting due to Kconfig warnings`, so a `prj.conf` that believed the
// documentation failed the *configure* step with "attempt to assign the value
// '256' to the undefined symbol OBJZ_LOG_BUFFER_SIZE" -- naming the user's
// spelling, not the documentation that was wrong.
//
// Three choices in here are load-bearing:
//
//   - **Comments in `src/` and `include/` are not stripped.** A header comment
//     promising an option to users is a promise, and half of #420 was
//     exactly that: the option was documented in `OZLog.h`'s Doxygen block.
//     A prose mention of an option that does not exist is the bug, so it has
//     to count.
//   - **Comments in the CMake files *are* stripped.** Those name symbols
//     deliberately to record that they are retired -- `oz_static.cmake` and
//     `CMakeLists.txt` both explain that `CONFIG_OBJZ_BACKEND` dispatched
//     between two backends until the Python one went. Requiring those to exist
//     would be requiring the retirement to be undone.
//   - **`runtime_legacy` and `objc-reference` are skipped**, the same
//     exclusion `no_dead_ivars.rs` makes and for the same reason: no
//     `CMakeLists.txt`, cmake module or justfile recipe reaches either tree,
//     so the ~29 undeclared spellings in them are references in dead code
//     rather than reads. Widening to them would make this test permanently red
//     and therefore useless.
//
// The reverse direction is deliberately not asserted here. It has exactly one
// hit -- `CONFIG_OBJZ_BACKEND_STATIC` is declared, `default y`, and read by
// nothing at all, with `main.rs:5` claiming it is "wired into CMake by
// cmake/oz_static.cmake" where that file never tests it. Deleting a Kconfig
// symbol is its own decision, so it is recorded in `docs/STATUS.md` under
// "Standing design rules" rather than turned into a failing test.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn repo(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

/// Every C/ObjC source and header under `dir`, recursively, skipping the
/// trees that are kept for reference rather than compiled.
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

/// The symbols `Kconfig` declares, without the `CONFIG_` prefix.
///
/// `menuconfig` as well as `config`: the master enable is
/// `menuconfig OBJZ`, and a `CONFIG_OBJZ` read is as real as any other.
fn declared_in(kconfig: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for line in kconfig.lines() {
                let line = line.trim();
                for kw in ["config ", "menuconfig "] {
                        if let Some(rest) = line.strip_prefix(kw) {
                                let name = rest.trim();
                                if !name.is_empty() && !name.contains(char::is_whitespace) {
                                        out.insert(name.to_string());
                                }
                        }
                }
        }
        out
}

/// Every `CONFIG_OBJZ...` spelling in `text`, as whole identifier tokens.
///
/// A trailing `_` is dropped, so the `f"CONFIG_OBJZ_{name}"` shape a
/// generator writes does not turn into a symbol nobody could declare.
fn config_tokens(text: &str) -> BTreeSet<String> {
        let bytes = text.as_bytes();
        let mut out = BTreeSet::new();
        let needle = "CONFIG_OBJZ";
        let mut from = 0;
        while let Some(rel) = text[from..].find(needle) {
                let start = from + rel;
                if start > 0 && is_ident_byte(bytes[start - 1]) {
                        from = start + needle.len();
                        continue;
                }
                let mut end = start + needle.len();
                while end < bytes.len() && is_ident_byte(bytes[end]) {
                        end += 1;
                }
                let tok = text[start..end].trim_end_matches('_');
                if tok.len() > "CONFIG_".len() {
                        out.insert(tok.to_string());
                }
                from = end;
        }
        out
}

fn is_ident_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
}

/// `text` with `#` comments removed, line by line. Good enough for CMake,
/// which has no block comment in use here and no `#` inside a string in any
/// line that names a config symbol.
fn strip_hash_comments(text: &str) -> String {
        text.lines()
                .map(|l| l.split('#').next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n")
}

#[test]
fn every_config_objz_a_compiled_source_names_is_declared() {
        let kconfig_path = repo("Kconfig");
        let kconfig = fs::read_to_string(&kconfig_path).expect("Kconfig must be readable");
        let declared = declared_in(&kconfig);
        assert!(
                declared.len() >= 5,
                "parsed only {} symbols out of {:?}, so the parser is probably broken \
                 rather than the file being empty",
                declared.len(),
                kconfig_path
        );
        assert!(
                declared.contains("OBJZ"),
                "the master enable `menuconfig OBJZ` was not parsed, so this test is \
                 not reading Kconfig the way it thinks it is"
        );

        /* Comments are kept in the SDK's own sources: a header comment
         * promising an option to users is a promise (#420). */
        let mut files = Vec::new();
        sources(&repo("src"), &mut files);
        sources(&repo("include"), &mut files);
        files.sort();
        assert!(
                files.len() >= 10,
                "found only {} compiled sources, so the walk is broken",
                files.len()
        );

        let mut corpus: BTreeMap<PathBuf, String> = files
                .into_iter()
                .filter_map(|p| fs::read_to_string(&p).ok().map(|t| (p, t)))
                .collect();

        /* Comments are stripped from the build files: those name retired
         * symbols on purpose, to record that they are retired. */
        for rel in ["CMakeLists.txt", "cmake/oz_static.cmake", "cmake/ObjcClang.cmake"] {
                let p = repo(rel);
                let text = fs::read_to_string(&p)
                        .unwrap_or_else(|e| panic!("{rel} must be readable: {e}"));
                corpus.insert(p, strip_hash_comments(&text));
        }

        let mut undeclared = Vec::new();
        for (path, text) in &corpus {
                for tok in config_tokens(text) {
                        let name = tok.strip_prefix("CONFIG_").unwrap_or(&tok);
                        if !declared.contains(name) {
                                undeclared.push(format!(
                                        "  {} names {} -- no `config {}` in Kconfig",
                                        path.file_name().unwrap().to_string_lossy(),
                                        tok,
                                        name
                                ));
                        }
                }
        }
        undeclared.sort();
        undeclared.dedup();

        assert!(
                undeclared.is_empty(),
                "these CONFIG_OBJZ_* symbols are read or documented by compiled sources \
                 and declared nowhere:\n{}\n\nDeclare each one in Kconfig under `if OBJZ`, \
                 or stop naming it. An undeclared option is not a setting that gets \
                 ignored: Zephyr aborts on Kconfig warnings, so a prj.conf that sets it \
                 fails the configure step naming the *user's* spelling rather than the \
                 documentation that was wrong (#420).",
                undeclared.join("\n")
        );
}
