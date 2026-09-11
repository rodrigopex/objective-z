// SPDX-License-Identifier: Apache-2.0
//
// sdk_spliced_file_scope.rs -- what an SDK `.m` may put at file scope (#422).
//
// File-scope text ahead of an `@implementation` is spliced into the
// *generated header* for that origin, and every Foundation translation unit
// includes those headers. So file scope in `src/*.m` is not private: it is
// the Foundation's shared namespace. `src/OZObject.m` says the same thing
// from the other side, in the comment on `_oz_write_default_description`,
// which is `static inline` rather than `static` for exactly this reason --
// a plain `static` definition is unused in all but one of the translation
// units it lands in, and Zephyr builds with `-Werror`.
//
// `src/OZMutableString.m` carried `#define NULL ((void *)0)` there, which
// therefore redefined a standard library macro for the whole Foundation
// (#422). It was `#ifndef`-guarded and its replacement text was
// token-identical to libc's, so it never expanded and never warned -- which
// is why reading the generated header, compiling it, and running it all
// reported nothing. Only the text says it.
//
// Hence a text guard rather than a behavioural one. Two rules, and the
// first is the general one:
//
//   1. A `#define` in the spliced prelude must be project-namespaced
//      (`OZ`/`_OZ`). That admits the idempotency guard `OZNumber.m` wraps
//      its `static inline` q31 helpers in (`_OZ_Q31_HELPERS`), which is the
//      pattern this file endorses, and rejects anything owned by someone
//      else -- libc today, Zephyr tomorrow.
//   2. No standard library macro is defined anywhere in an SDK `.m`, prelude
//      or not. After the `@implementation` the text lands in the generated
//      `.c` instead of the header, which is narrower blast radius and still
//      not this SDK's macro to define.
//
// Both enumerate `src/*.m` rather than naming one file, because the defect
// was one instance of a class of defect. The count assertion is deliberate:
// a glob that matches nothing reports clean, which is how #400's PR came to
// quote a passing number for a corpus it never looked at.

use std::fs;
use std::path::PathBuf;

/// Every Objective-C source under `src/` -- the SDK implementations whose
/// file scope is shared, paired with its text.
///
/// `src/runtime_legacy/` is not compiled and never transpiled, so it is out
/// of scope; `read_dir` is not recursive, which keeps it out by itself.
fn sdk_sources() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src");
    let mut out: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", dir.display(), e))
        .map(|e| e.expect("cannot read a directory entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "m"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let text = fs::read_to_string(&p)
                .unwrap_or_else(|e| panic!("cannot read {}: {}", p.display(), e));
            (name, text)
        })
        .collect();
    out.sort();

    /* A glob that matches nothing passes every assertion below. */
    assert!(
        out.len() >= 9,
        "expected the SDK's Objective-C sources under src/, found {}: {:?}",
        out.len(),
        out.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    out
}

/// The part of a `.m` that oz_static splices into the generated header for
/// that origin: everything ahead of the first `@implementation`.
///
/// A file with no `@implementation` at all -- `src/OZLog.m` is one, a
/// reference stub -- is prelude from end to end.
///
/// The boundary is a *line* beginning with `@implementation`, not the first
/// occurrence of the word. A plain `src.find("@implementation")` matched the
/// word inside the very comment that explains this splicing, put one line
/// above the `#define` this file exists to catch, and truncated the prelude
/// short of it -- so the guard reported clean on the exact input it was
/// written against.
fn spliced_prelude(src: &str) -> String {
    src.lines()
        .take_while(|line| !line.trim_start().starts_with("@implementation"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The macro name a `#define` line introduces, if the line is one.
fn defined_macro(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("#define")?;
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let name = rest.trim_start();
    let end = name
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(name.len());
    if end == 0 {
        return None;
    }
    Some(&name[..end])
}

/// Rule 1: a macro defined in the spliced prelude belongs to this project.
#[test]
fn a_spliced_prelude_defines_only_project_namespaced_macros() {
    let mut offenders: Vec<String> = Vec::new();

    for (name, text) in sdk_sources() {
        for line in spliced_prelude(&text).lines() {
            let Some(macro_name) = defined_macro(line) else {
                continue;
            };
            if macro_name.starts_with("OZ") || macro_name.starts_with("_OZ") {
                continue;
            }
            offenders.push(format!("{}: {}", name, line.trim()));
        }
    }

    assert!(
        offenders.is_empty(),
        "file scope ahead of an `@implementation` is spliced into the generated \
         header every Foundation translation unit includes, so a macro defined \
         there is defined for all of them. Namespace it `OZ`/`_OZ`, or move it \
         inside the `@implementation`:\n  {}",
        offenders.join("\n  ")
    );
}

/// Rule 2: no standard library macro is redefined in an SDK `.m` at all.
///
/// The list is the ones a `.m` here has any plausible reason to reach for.
/// `NULL` is the one that was actually redefined; the rest are on it so this
/// test is about the class of defect rather than about one name.
#[test]
fn no_sdk_source_redefines_a_standard_library_macro() {
    const STANDARD: [&str; 10] = [
        "NULL",
        "offsetof",
        "EXIT_SUCCESS",
        "EXIT_FAILURE",
        "errno",
        "assert",
        "NDEBUG",
        "bool",
        "true",
        "false",
    ];

    let mut offenders: Vec<String> = Vec::new();

    for (name, text) in sdk_sources() {
        for (n, line) in text.lines().enumerate() {
            let Some(macro_name) = defined_macro(line) else {
                continue;
            };
            if STANDARD.contains(&macro_name) {
                offenders.push(format!("{}:{}: {}", name, n + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "an SDK `.m` redefines a standard library macro. `<stddef.h>` and \
         friends supply these, and `OZObject.h` includes them; a redefinition \
         is silent only for as long as its replacement text happens to match \
         the toolchain's:\n  {}",
        offenders.join("\n  ")
    );
}
