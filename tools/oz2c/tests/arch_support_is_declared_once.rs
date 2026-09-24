// arch_support_is_declared_once.rs -- the architectures `Kconfig` lets
// `CONFIG_OBJZ` be enabled for are exactly the ones
// `_objz_get_clang_target_triple()` can name a triple for.
//
// Two files state the same fact and neither derives it from the other.
// `Kconfig`'s `depends on` decides whether the option can be set at all;
// `cmake/ObjcClang.cmake`'s table decides whether a triple exists for the
// Clang AST dump oz2c reads as an ownership oracle. Drift either way is a
// bad failure:
//
//   - an architecture in Kconfig with no triple reaches
//     `message(FATAL_ERROR ...)` from inside the *application's* configure,
//     after the option was accepted -- so Kconfig promised support the
//     build cannot deliver.
//   - a triple with no Kconfig entry is unreachable, and reads as support
//     that was added and then forgotten.
//
// Why the two lists exist at all, rather than one deriving the other: the
// dump has to be parsed for the target, because Zephyr's arch headers carry
// inline asm whose register names and operand constraints Clang validates
// against the triple. A mismatch fails the dump outright (16 diagnostics for
// an ARM triple over x86 headers, 15 over RISC-V), oz2c refuses a dump whose
// clang run errored, and the build stops. The facts oz2c extracts are in
// fact triple-independent -- byte-identical across triples for all 13 `.m`
// files of `samples/hello_category` -- which makes a single fixed triple
// look viable and it is not, because facts only matter once the dump is
// accepted (#612).
//
// The check is at *architecture family* granularity, not per-CPU. Kconfig
// names families (`CPU_CORTEX_M`), the table names individual cores
// (`CONFIG_CPU_CORTEX_M33`), and requiring agreement below the family would
// mean re-listing every core here -- a third copy, and the one most likely
// to rot. Adding a core to an already-supported family needs no Kconfig
// change, which is exactly the case this deliberately does not flag.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

/// The architecture families this project knows how to talk about.
///
/// Explicit rather than inferred: a new family is a deliberate act, and
/// this list is where a reader finds out which ones exist. `needle` is what
/// identifies the family in either file -- Kconfig writes `CPU_CORTEX_M`,
/// the cmake table writes `CONFIG_CPU_CORTEX_M33`, and both contain the
/// family's name.
const FAMILIES: &[(&str, &str)] = &[
        ("ARM Cortex-M", "CPU_CORTEX_M"),
        ("ARM Cortex-A", "CPU_CORTEX_A"),
        ("RISC-V", "RISCV"),
        ("x86", "X86"),
];

/// `menuconfig OBJZ`'s `depends on` line, as written.
fn kconfig_depends_line() -> String {
        let path = repo("Kconfig");
        let text = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));

        /* The first `depends on` after `menuconfig OBJZ`, so a `depends on`
         * belonging to one of the options under `if OBJZ` cannot be read as
         * the master enable's. */
        let mut seen_menuconfig = false;
        for line in text.lines() {
                let trimmed = line.trim();
                if trimmed == "menuconfig OBJZ" {
                        seen_menuconfig = true;
                        continue;
                }
                if seen_menuconfig {
                        if let Some(rest) = trimmed.strip_prefix("depends on ") {
                                return rest.to_string();
                        }
                        /* The help block ends the property list. */
                        if trimmed == "help" {
                                break;
                        }
                }
        }
        panic!(
                "no `depends on` found for `menuconfig OBJZ` in {}. If the architecture \
                 dependency was removed, the triple table in cmake/ObjcClang.cmake no longer \
                 has a counterpart and this test's premise is gone -- see #612.",
                path.display()
        );
}

/// The body of `_objz_get_clang_target_triple()`, comments stripped.
///
/// Comments have to go: the function's own doc block names architectures
/// while *explaining* the constraint, including ones it does not support.
fn triple_table_body() -> String {
        let path = repo("cmake/ObjcClang.cmake");
        let text = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));

        let start = text
                .find("function(_objz_get_clang_target_triple")
                .unwrap_or_else(|| {
                        panic!(
                                "no `_objz_get_clang_target_triple()` in {}. If the per-target \
                                 triple was replaced by a fixed one, note that this was tried \
                                 and reverted: a mismatched triple fails the dump on inline \
                                 asm -- see #612.",
                                path.display()
                        )
                });
        let rest = &text[start..];
        let end = rest.find("\nendfunction()").unwrap_or_else(|| {
                panic!("`_objz_get_clang_target_triple()` has no `endfunction()`")
        });

        rest[..end].lines().map(|l| l.split('#').next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

#[test]
fn kconfig_and_the_triple_table_support_the_same_architectures() {
        let depends = kconfig_depends_line();
        let table = triple_table_body();

        /* Both parsers have to have found something real, or the sets agree
         * by both being empty. */
        assert!(
                depends.contains("CPU_CORTEX_M"),
                "the `depends on` line parsed as `{depends}`, which does not mention \
                 CPU_CORTEX_M -- the parser is reading the wrong line"
        );
        assert!(
                table.contains("set(_triple"),
                "the triple table parsed to something with no `set(_triple ...)` in it, so \
                 the extraction is broken rather than the table being empty"
        );

        let mut in_kconfig = BTreeSet::new();
        let mut in_table = BTreeSet::new();
        for (name, needle) in FAMILIES {
                /* RISCV is a substring of nothing else here, but X86 is a
                 * substring of nothing while CPU_CORTEX_M is a prefix of
                 * CPU_CORTEX_M33 -- which is the point, since the family is
                 * what both files agree on. */
                if depends.contains(needle) {
                        in_kconfig.insert(*name);
                }
                if table.contains(needle) {
                        in_table.insert(*name);
                }
        }

        assert!(
                !in_kconfig.is_empty() && !in_table.is_empty(),
                "neither list matched any known family, so FAMILIES is stale rather than the \
                 files disagreeing"
        );

        let kconfig_only: Vec<_> = in_kconfig.difference(&in_table).copied().collect();
        let table_only: Vec<_> = in_table.difference(&in_kconfig).copied().collect();

        assert!(
                kconfig_only.is_empty(),
                "Kconfig lets CONFIG_OBJZ be enabled for {kconfig_only:?}, but \
                 _objz_get_clang_target_triple() has no triple for it. The option would be \
                 accepted and then the application's configure would die in \
                 cmake/ObjcClang.cmake. Add the triple, or drop it from `depends on`.\n\
                 Kconfig: {in_kconfig:?}\ntable:   {in_table:?}"
        );
        assert!(
                table_only.is_empty(),
                "_objz_get_clang_target_triple() names a triple for {table_only:?}, which \
                 Kconfig's `depends on` does not allow -- so it is unreachable and reads as \
                 support that exists. Add it to `depends on`, or remove the triple.\n\
                 Kconfig: {in_kconfig:?}\ntable:   {in_table:?}"
        );
}
