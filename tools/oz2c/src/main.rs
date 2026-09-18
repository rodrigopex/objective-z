// SPDX-License-Identifier: Apache-2.0
//
// main.rs - CLI entry point for the OZ-091 Track B spike.
//
// Wired into CMake by cmake/oz2c.cmake, which CMakeLists.txt includes
// under CONFIG_OBJZ. Not under CONFIG_OBJZ_BACKEND_STATIC: that symbol is
// declared `default y` in Kconfig and read by nothing -- no cmake file
// mentions it -- so turning it off would not select another backend. It is
// the last trace of the retired backend dispatcher (#420's reverse sweep).
// Run directly for manual experimentation:
//   cargo run --manifest-path tools/oz2c/Cargo.toml -- <input.m> <outdir>

mod report;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage: oz2c [-I <dir>]... [--impl-dir <dir>]... [--manifest <path>] \
         [--root-class <name>] [--pool-sizes <Class=N,...>] \
         [--item-pool-size <N>] [--ast <ast.json>]... [--allow-missing-ast] \
         [--heap-support] [--introspection] [--reflection] \
         [--line-directives] [--timings] [--quiet] \
         [--manifest-only] [--dump-cst] \
         <input.m>... <outdir>\n\
         \x20      oz2c --dump-ast-facts [--ast <ast.json>]...\n\
         \x20      oz2c --check-arc [-I <dir>]... --ast <ast.json>... <input.m>..."
    );
    ExitCode::FAILURE
}

/// Print every fact the `--ast` dumps carry, sorted, one per line.
///
/// The whole point is that two runs over the same dumps produce identical
/// bytes, so `diff` is a proof rather than an impression -- see
/// `astinfo::AstFacts::dump_lines`. Goes to **stdout**, because stderr is
/// where diagnostics live and `tests/tools/oz2c_build.py` reports its
/// first line as the reason a transpile failed.
///
/// Unlike the transpile path this does not reject dumps that describe no
/// ivars: "these dumps say nothing" is a legitimate answer to ask for, and
/// making it an error would mean the oracle could not record the
/// no-AST baseline it is most often diffed against.
/// `eprintln!` with the one prefix every oz2c error line carries.
///
/// The binary is `oz2c`; `oz2c` is the crate it is built from, and
/// naming the crate told a reader nothing they could act on. A macro
/// rather than the literal at each site, because it *was* the literal at
/// twelve of them and a thirteenth would have drifted (#456).
///
/// `scripts/objz_check_oz2c_diagnostics.py` greps for this text, so the
/// two move together or `hw-build-check` fails on both boards.
macro_rules! oz_err {
    ($($arg:tt)*) => {
        eprintln!("oz2c error: {}", format_args!($($arg)*))
    };
}

/// `oz2c --check-arc`: print the ARC audit, one section per entry source.
///
/// **Always succeeds.** #453 asks for "an audit tool whose output is a work
/// queue", not a gate, and that is a design decision rather than an
/// omission: the two models legitimately differ, because Clang retains on
/// binding and `arc.rs` elides at source level. A gate here would fail on
/// every correct program.
///
/// One report per entry file rather than one for the whole translation
/// unit, because the useful question is "what does Clang say about *this*
/// source" -- the dumps also cover every header the source imports and the
/// SDK implementations behind them, which is 29.6% of the marks in this
/// repo's own corpora.
fn run_check_arc(
    resolved: &oz2c::imports::ResolvedSource,
    ast_paths: &[PathBuf],
    entry_paths: &[PathBuf],
) -> ExitCode {
    if ast_paths.is_empty() {
        oz_err!(
            "--check-arc compares oz2c against Clang's own marks, so it needs at least \
             one --ast dump (produce one with `clang -Xclang -ast-dump=json -fsyntax-only \
             -fobjc-arc`)"
        );
        return ExitCode::FAILURE;
    }
    let mut facts = oz2c::astinfo::AstFacts::default();
    for path in ast_paths {
        match oz2c::astinfo::AstFacts::from_path(path) {
            Ok(one) => facts.merge(one),
            Err(e) => {
                oz_err!("{}", e);
                return ExitCode::FAILURE;
            }
        }
    }
    let (mut program, _) = oz2c::collect::collect(&resolved.text);
    program.ast = Some(facts);
    /* Read back out of the program rather than kept aside, so the audit
     * compares against the same facts the transpile path would attach --
     * one owner for the oracle, not two. */
    let facts = program.ast.take().expect("just set");
    for path in entry_paths {
        /* The file name as Clang would have echoed it. A suffix match is
         * what reconciles the two spellings -- see `AstFacts::marks_in`. */
        let suffix = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        for line in oz2c::checkarc::report(&program, &facts, &suffix) {
            println!("{}", line);
        }
        println!();
    }
    ExitCode::SUCCESS
}

fn dump_merged_ast_facts(ast_paths: &[PathBuf]) -> ExitCode {
    let mut facts = oz2c::astinfo::AstFacts::default();
    for path in ast_paths {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                oz_err!("cannot read --ast '{}': {}", path.display(), e);
                return ExitCode::FAILURE;
            }
        };
        match oz2c::astinfo::AstFacts::from_json(&text) {
            Ok(one) => facts.merge(one),
            Err(e) => {
                oz_err!("--ast '{}': {}", path.display(), e);
                return ExitCode::FAILURE;
            }
        }
    }
    for line in facts.dump_lines() {
        println!("{}", line);
    }
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut extra_include_dirs: Vec<PathBuf> = Vec::new();
    let mut extra_impl_dirs: Vec<PathBuf> = Vec::new();
    let mut manifest_path: Option<PathBuf> = None;
    let mut expected_root: Option<String> = None;
    let mut pool_overrides = oz2c::PoolOverrides::new();
    let mut ast_paths: Vec<PathBuf> = Vec::new();
    let mut heap_support = false;
    let mut introspection = false;
    let mut reflection = false;
    let mut item_pool_size: Option<usize> = None;
    let mut line_directives = false;
    let mut dump_cst = false;
    let mut dump_ast_facts = false;
    let mut check_arc = false;
    let mut timings = false;
    let mut quiet = false;
    let mut manifest_only = false;
    let mut allow_missing_ast = false;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-I" => {
                let Some(dir) = args.get(i + 1) else { return usage() };
                extra_include_dirs.push(PathBuf::from(dir));
                i += 2;
            }
            "--impl-dir" => {
                let Some(dir) = args.get(i + 1) else { return usage() };
                extra_impl_dirs.push(PathBuf::from(dir));
                i += 2;
            }
            "--manifest" => {
                let Some(path) = args.get(i + 1) else { return usage() };
                manifest_path = Some(PathBuf::from(path));
                i += 2;
            }
            "--root-class" => {
                let Some(name) = args.get(i + 1) else { return usage() };
                expected_root = Some(name.clone());
                i += 2;
            }
            // Same spelling and meaning as the Python backend's flag.
            // Enables `+dynamicAllocWithHeap:` and the heap-aware free path; the
            // generated code is additionally guarded by `OZ_HEAP_SUPPORT`,
            // which is what makes the PAL expose the heap it needs.
            "--heap-support" => {
                heap_support = true;
                i += 1;
            }
            // `CONFIG_OBJZ_INTROSPECTION`. Enables `-isKindOfClass:` and
            // `-conformsToProtocol:`, the two introspection selectors that
            // generate a table (a superclass chain and a per-protocol
            // conformance bitmap). Without it both are located errors that
            // name the option. Class identity -- `[Foo class]`,
            // `[obj class]`, `-isMemberOfClass:` -- is always available and
            // unaffected: it costs no table.
            "--introspection" => {
                introspection = true;
                i += 1;
            }
            // `CONFIG_OBJZ_REFLECTION`. Enables `@selector`, `SEL`,
            // `-respondsToSelector:` and the `-performSelector:` family.
            // Costs more than introspection when used: a const record per
            // reflectively-named selector, a uniform-shape wrapper for
            // each, and an `OZ_PROTOCOL_SEND_*` forced into existence for
            // any that had only one implementor. All of it flash, none of
            // it emitted for a selector no `@selector(...)` names.
            "--reflection" => {
                reflection = true;
                i += 1;
            }
            // `CONFIG_OBJZ_DEBUG_LINES`. Put `#line` directives on the code
            // the author wrote -- method bodies, plain C function bodies and
            // hoisted blocks -- so a `gdb` breakpoint, a fatal-error
            // backtrace, `addr2line` and a coverage report all name the
            // `.m` instead of `oz2c_generated/<Class>.c` (#305).
            // Synthesized code keeps pointing at the generated file, which
            // is where it genuinely lives.
            //
            // Costs nothing at runtime -- it changes only what the compiler
            // writes into DWARF -- but it roughly doubles the generated C,
            // since every directive carries an absolute path. So the Kconfig
            // option depends on `CONFIG_DEBUG` and is `y` within it (#395),
            // and this flag is what supplies it. Off by default
            // *here*, like `--introspection` and `--reflection`: the flag's
            // absence is what the option's `n` means, and it keeps the committed
            // `tests/zephyr/generated/` C (regenerated through this CLI)
            // free of absolute paths from whoever's machine ran it.
            "--line-directives" => {
                line_directives = true;
                i += 1;
            }
            // Clang resolves types; tree-sitter does not. Supplying the AST
            // is what lets oz2c know which ivars are objects the class
            // owns -- including `id`-typed ones, which it otherwise has to
            // skip rather than risk releasing a non-object. Produce it with
            // `-fobjc-arc`, or the dump carries no ownership at all.
            "--ast" => {
                let Some(path) = args.get(i + 1) else { return usage() };
                ast_paths.push(PathBuf::from(path));
                i += 2;
            }
            // The escape hatch for `--ast` now being *required* of any
            // source that declares a class. Transpile anyway, with `arc`
            // falling back to the syntactic rule that skips every
            // `id`-typed ivar -- correct, and a leak.
            //
            // It exists because one caller genuinely cannot produce a
            // dump, and only one: a hand transpile of a source whose
            // header closure will not parse where it is being run. The
            // configure-time `--manifest-only` run is the in-tree example
            // and needs no flag, because it implies this (see below):
            // Zephyr's generated headers do not exist yet at that point,
            // so `zephyr/kernel.h` dies on
            // `fatal error: 'zephyr/syscall_list.h' file not found`.
            //
            // Named for what it permits rather than what it disables, so
            // it cannot be mistaken for "skip the AST to go faster".
            "--allow-missing-ast" => {
                allow_missing_ast = true;
                i += 1;
            }
            // Same spelling and meaning as the Python backend's flag, so a
            // sample's CMakeLists.txt needs no per-backend variant. Also
            // accepted as an `/* oz-pool: ... */` comment in the source
            // itself, which is what the oracle's own behavior cases use;
            // this flag wins for the classes it names (see
            // `pools::PoolSizes::set_overrides`).
            "--pool-sizes" => {
                let Some(spec) = args.get(i + 1) else { return usage() };
                match oz2c::pools::parse_pool_sizes(spec) {
                    Ok(sizes) => pool_overrides.extend(sizes),
                    Err(why) => {
                        oz_err!("--pool-sizes: {}", why);
                        return ExitCode::FAILURE;
                    }
                }
                i += 2;
            }
            // Slots for the shared '@[...]'/'@{...}' element pool, the
            // oracle's identically-spelled flag. Also accepted as an
            // `/* oz-item-pool: N */` comment; this flag wins (see
            // `pools::PoolSizes::item_slots`).
            "--item-pool-size" => {
                let Some(spec) = args.get(i + 1) else { return usage() };
                match spec.parse::<usize>() {
                    Ok(slots) => item_pool_size = Some(slots),
                    Err(_) => {
                        oz_err!(
                            "--item-pool-size: '{}' is not a number",
                            spec
                        );
                        return ExitCode::FAILURE;
                    }
                }
                i += 2;
            }
            // Print the top-level shape of the *import-resolved* tree and
            // stop. The resolved text is what every later pass sees, and it
            // is not the file the user wrote: `#import` splices each header
            // and its sibling implementation inline, so a construct's
            // grouping -- and its byte offsets, which the whole emitter is
            // keyed on -- can differ from the raw `.m`. Reaching for the
            // raw file instead is a real way to misdiagnose: it is how
            // #288's cause was first missed.
            //
            // Only the translation unit's own children are printed, with
            // their origin and any ERROR/MISSING marker. That is the view
            // that shows a construct absorbed into its neighbour, which is
            // the failure this exists for; the full tree over a resolved
            // source runs to tens of thousands of nodes and buries it.
            // Add the per-phase table to the progress output. Off by
            // default because most of what it explains is one number --
            // the AST ingest -- which the default output already names.
            // Write only the manifest -- the list of files a full run
            // would generate -- and none of the files themselves.
            //
            // `cmake/oz2c.cmake` has to know that list before it can
            // declare it as an `add_custom_command` OUTPUT, and the only
            // thing that knows it is oz2c. That is why a second, full
            // transpile ran at configure time, dumping and parsing every
            // Clang AST to produce output it then threw away.
            //
            // No AST is needed for it: the file *names* come from
            // `stem_order`, which is built from top-level node origins, so
            // the AST changes what is in a file and never which files
            // exist. Deliberately the same code path as a real run, with
            // only the `fs::write` calls skipped -- so the list cannot
            // drift from the real one by construction, which reimplementing
            // the layout rules in CMake would not guarantee (#299).
            "--manifest-only" => {
                manifest_only = true;
                i += 1;
            }
            "--timings" => {
                timings = true;
                i += 1;
            }
            // Print nothing on stdout, and the summary on stderr exactly
            // as oz2c did before there was any progress output. For
            // anything that parses what we print.
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            "--dump-cst" => {
                dump_cst = true;
                i += 1;
            }
            // Print the merged `--ast` facts and exit -- the equivalence
            // oracle for any change to the AST path.
            //
            // Ingesting these dumps is 97.5% of oz2c's wall clock on
            // px-keyboard (11.05s of 11.32s), so that is where the
            // optimisation pressure is, and a mistake there does not fail
            // the build: it makes the oracle answer one question fewer,
            // which silently leaks an ivar. Diffing generated C only
            // catches facts one program happens to use today, so this
            // dumps the fact set itself (#299).
            //
            // Takes no source and no outdir -- the facts are a property of
            // the dumps alone, and requiring an unrelated `.m` would make
            // the baseline harder to capture than the thing it guards.
            "--dump-ast-facts" => {
                dump_ast_facts = true;
                i += 1;
            }
            // An audit, not a gate: it always succeeds, and its output is a
            // work queue. Takes the sources and the dumps but no outdir --
            // it emits nothing, and demanding a directory it would not
            // write to would make the tool harder to run than to read.
            "--check-arc" => {
                check_arc = true;
                i += 1;
            }
            arg => {
                positional.push(arg.to_string());
                i += 1;
            }
        }
    }

    if dump_ast_facts {
        return dump_merged_ast_facts(&ast_paths);
    }

    /* `quiet` is checked after `timings` so the two are not
     * order-dependent on the command line, and `--dump-cst` forces quiet:
     * it prints a dump on stdout and returns before the pipeline, so a
     * progress header above it would be noise in front of the thing the
     * flag exists to show. */
    let level = if quiet || dump_cst || check_arc {
        report::Level::Quiet
    } else if timings {
        report::Level::Timings
    } else {
        report::Level::Normal
    };
    let mut rep = report::Reporter::new(level);
    // Every positional but the last is an entry `.m`; the last is the
    // output directory. A build system lists every `.m` a target owns
    // (see `cmake/oz2c.cmake`), and all of them become one
    // translation unit -- see `imports::resolve_entry_files` for why one
    // unit rather than one run per file.
    /* `--check-arc` writes nothing, so it takes sources and no outdir:
     * demanding a directory it would not touch would make the tool harder
     * to run than to read. Every other invocation keeps the
     * `<input.m>... <outdir>` shape. */
    let (outdir, entry_paths): (Option<&Path>, Vec<PathBuf>) = if check_arc {
        if positional.is_empty() {
            return usage();
        }
        (None, positional.iter().map(PathBuf::from).collect())
    } else {
        if positional.len() < 2 {
            return usage();
        }
        (
            Some(Path::new(positional.last().unwrap())),
            positional[..positional.len() - 1].iter().map(PathBuf::from).collect(),
        )
    };

    for path in &entry_paths {
        if !path.is_file() {
            oz_err!("no such input file: '{}'", path.display());
            return ExitCode::FAILURE;
        }
    }

    // `#import` resolution needs a real filesystem, so it lives outside
    // the core (pure, filesystem-free) `transpile()`/`transpile_split()`
    // pipeline -- see `imports.rs`. Default search paths: this repo's
    // own `include/oz_sdk` (headers) and `src` (their sibling `.m`
    // implementations), the same layout every Foundation class lives in.
    // `-I`/`--impl-dir` extend these for a caller's own project-local
    // headers -- mirroring gcc's `-I`, plus a second flag because
    // `find_sibling_impl` (imports.rs) only searches `impl_dirs`, never
    // a header's own directory.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut include_dirs = vec![repo_root.join("include/oz_sdk")];
    include_dirs.extend(extra_include_dirs);
    let mut impl_dirs = vec![repo_root.join("src")];
    impl_dirs.extend(extra_impl_dirs);
    oz2c::progress::Observer::enter(&mut rep, oz2c::progress::Phase::ImportResolve);
    let resolved =
        match oz2c::imports::resolve_entry_files(&entry_paths, &include_dirs, &impl_dirs) {
            Ok(r) => r,
            Err(e) => {
                /* Rendered, not printed as a bare line (#550). This was
                 * the only unlocated diagnostic the front end could
                 * produce: it named the path and the search dirs and not
                 * the file that asked for it. `render` draws the same
                 * `--> file:line:col` frame, snippet and caret every other
                 * refusal gets -- against the *failing file's own* text,
                 * which the error carries, because this is raised during
                 * the splice when no merged buffer exists yet.
                 *
                 * `render` emits the `oz2c error:` prefix and its own
                 * trailing newline, so `eprint!` rather than `oz_err!` --
                 * the same reason the diagnostic loop at the end of this
                 * file does (#457). An I/O read failure carries no span
                 * and falls back to the bare line, which is right: there
                 * is no source position for "permission denied". */
                eprint!("{}", oz2c::render::render(&e.diagnostic, &e.source));
                return ExitCode::FAILURE;
            }
        };

    if dump_cst {
        dump_resolved_cst(&resolved);
        return ExitCode::SUCCESS;
    }

    if check_arc {
        return run_check_arc(&resolved, &ast_paths, &entry_paths);
    }

    /* Never `None` here: `--check-arc` is the only shape without an
     * outdir, and it returned above. */
    let outdir = outdir.expect("an outdir, since --check-arc returned above");

    // oz2c infers the root class (the one class with no superclass)
    // rather than being told it, so `--root-class` is a cross-check on the
    // build system's expectation, not an input to codegen: it catches a
    // target configured for a root that isn't actually the root, which
    // would otherwise produce a working-but-differently-rooted program.
    // Only paid for when the flag is passed, since it needs its own
    // `collect` pass.
    oz2c::progress::Observer::enter(&mut rep, oz2c::progress::Phase::RootClassCheck);
    if let Some(expected) = &expected_root {
        let (program, _) = oz2c::collect::collect(&resolved.text);
        match program.root_class() {
            Some(actual) if actual == expected => {}
            Some(actual) => {
                oz_err!(
                    "--root-class '{}' does not match this program's root class \
                     '{}' (the root is inferred as the class with no superclass, not configured)",
                    expected, actual
                );
                return ExitCode::FAILURE;
            }
            None => {
                oz_err!(
                    "--root-class '{}' was requested but this program declares \
                     no root class (every class has a superclass)",
                    expected
                );
                return ExitCode::FAILURE;
            }
        }
    }

    /* Sizes by `stat`, not by reading: the dumps are read one at a time
     * inside the pipeline now (`Options::ast_paths`), so reading them here
     * purely to measure them would reinstate the 1.30 GB peak this
     * avoids. */
    oz2c::progress::Observer::enter(&mut rep, oz2c::progress::Phase::AstRead);
    let mut ast_sizes: Vec<usize> = Vec::new();
    for path in &ast_paths {
        match fs::metadata(path) {
            Ok(meta) => ast_sizes.push(meta.len() as usize),
            Err(e) => {
                oz_err!("cannot read --ast '{}': {}", path.display(), e);
                return ExitCode::FAILURE;
            }
        }
    }
    /* Both known only here: the labels come from the paths this loop just
     * read, and the resolved size from `#import` resolution above. So this
     * is the earliest point the header can state the scale, and it has to
     * precede the ingest it is describing. */
    rep.note_ast_inputs(&ast_paths, &ast_sizes);
    rep.header(entry_paths.len(), resolved.origins.len(), resolved.text.len());

    /* Where each stem's generated pair will be written, for the `#line`
     * directives that hand attribution back to the generated file itself
     * (#305). The `Foundation/` split is decided here as well as in the
     * write loop below, because `emit` needs the answer while it is still
     * assembling the text -- and absolute, so a debugger resolves both
     * halves from any working directory. Built only when the flag asked
     * for directives; without it nothing reads this. */
    let mut generated_dirs: std::collections::HashMap<String, PathBuf> =
        std::collections::HashMap::new();
    if line_directives {
        let outdir_abs = if outdir.is_absolute() {
            outdir.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(outdir)
        };
        let foundation = outdir_abs.join("Foundation");
        for (stem, _) in &resolved.origins {
            let dir = if resolved.foundation_stems.contains(stem) {
                foundation.clone()
            } else {
                outdir_abs.clone()
            };
            generated_dirs.insert(stem.clone(), dir);
        }
    }

    match oz2c::transpile_split_observed(
        &resolved.text,
        &resolved.origins,
        &oz2c::Options {
            pool_sizes: pool_overrides,
            ast_json: Vec::new(),
            ast_paths: ast_paths.clone(),
            heap_support,
            introspection,
            reflection,
            item_pool_size,
            source_map: line_directives.then(|| resolved.source_map.clone()),
            generated_dirs,
            header_ranges: resolved.header_ranges.clone(),
            // The build contract: a source declaring a class is not
            // transpiled without Clang's answer about its ivars.
            //
            // `--manifest-only` is exempt by construction rather than by
            // permission. That run exists to discover the *file list*, and
            // no AST fact affects which files exist -- the names come from
            // top-level node origins (#299). It is also the one run that
            // could not have a dump if it wanted one: CMake calls it
            // before Zephyr's generated headers exist. Requiring `--ast`
            // there would mean producing 742 MB of JSON to answer a
            // question about filenames, which is precisely the cost #299
            // removed.
            require_ast: !allow_missing_ast && !manifest_only,
        },
        &mut rep,
    ) {
        Ok(out) => {
            oz2c::progress::Observer::enter(
                &mut rep,
                oz2c::progress::Phase::Write,
            );
            // Foundation/SDK-origin files land in their own subdirectory,
            // matching the Python pipeline's own `outdir/Foundation/`
            // layout -- the caller's own project-local files stay at
            // `outdir/` directly. Every generated `#include` is still
            // just a bare filename (see `emit::emit_split`), so whatever
            // compiles this needs both `outdir` and `outdir/Foundation`
            // on its include search path -- see cmake/oz2c.cmake.
            let foundation_dir = outdir.join("Foundation");
            if !manifest_only {
                if let Err(e) = fs::create_dir_all(&foundation_dir) {
                    oz_err!(
                        "cannot create '{}': {}",
                        foundation_dir.display(),
                        e
                    );
                    return ExitCode::FAILURE;
                }
            }
            let mut written: Vec<PathBuf> = Vec::new();
            for (file_stem, header_h, source_c) in &out.files {
                // A spliced pure-C header gets no output pair. Its text was
                // needed so the parse saw the whole program, but there is
                // nothing in it to transpile, and copying it back out is at
                // best duplication of a header the C compiler already has:
                // `cmake/oz2c.cmake` puts the module's own `include/`
                // on the path and links `src/OZLog.c` itself. At worst it
                // is a redefinition -- `include/oz_sdk/assert.h` is an
                // AST-analysis shim (its own comment: "The generated C
                // includes platform/oz_assert.h which provides the real
                // macros"), so its `static inline oz_assert_msg` came out
                // as a generated `assert.c` the PAL had already turned into
                // a function-like macro: "expected identifier or '('".
                if resolved.pure_c_stems.contains(file_stem) {
                    continue;
                }
                let target_dir =
                    if resolved.foundation_stems.contains(file_stem) { &foundation_dir } else { outdir };
                let h_path = target_dir.join(format!("{}.h", file_stem));
                let c_path = target_dir.join(format!("{}.c", file_stem));
                if !manifest_only {
                    let _ = fs::write(&h_path, header_h);
                    let _ = fs::write(&c_path, source_c);
                }
                written.push(h_path);
                written.push(c_path);
            }
            let dispatch_h = foundation_dir.join("oz2c_dispatch.h");
            let dispatch_c = foundation_dir.join("oz2c_dispatch.c");
            if !manifest_only {
                let _ = fs::write(&dispatch_h, out.companion_h);
                let _ = fs::write(&dispatch_c, out.companion_c);
            }
            written.push(dispatch_h);
            written.push(dispatch_c);
            if let Some(path) = &manifest_path {
                let manifest_text: String =
                    written.iter().map(|p| format!("{}\n", p.display())).collect();
                if let Err(e) = fs::write(path, manifest_text) {
                    oz_err!("cannot write manifest '{}': {}", path.display(), e);
                    return ExitCode::FAILURE;
                }
            }
            rep.finish(written.len(), outdir, manifest_only);
            ExitCode::SUCCESS
        }
        Err(diags) => {
            /* Resolved here rather than inside the library because this is
             * where the map lives: `resolved.source_map` covers the very
             * buffer that was just transpiled, while `Options::source_map`
             * carries one only to switch `#line` directives on -- reusing
             * that field would turn directives on for every build (#456).
             *
             * Each diagnostic carries the byte offset it was raised at, and
             * byte offsets survive `repair_bare_macro_statements` (which
             * overwrites a whitespace byte in place), so nothing has to be
             * re-derived here. A diagnostic with no offset, or one whose
             * offset falls outside the map, keeps the merged position it
             * arrived with. */
            for mut d in diags {
                d.resolve_in(&resolved.source_map);
                /* `render` carries the `oz2c error:` prefix itself, and
                 * emits its own trailing newline, so this goes out with
                 * `eprint!` rather than through `oz_err!` (#457). The
                 * first line is still a self-contained summary, which is
                 * what `oz2c_build.py` reads. */
                eprint!("{}", oz2c::render::render(&d, &resolved.text));
            }
            ExitCode::FAILURE
        }
    }
}

/// Print the translation unit's own children, for `--dump-cst`.
///
/// One line per top-level node: kind, byte range, 1-based line in the
/// resolved text, originating stem, and the node's first line of text.
/// A node tree-sitter could not parse is marked, and so is one whose span
/// reaches past its own construct into the next -- which is what a
/// semicolon-less function-like macro invocation does to its neighbour
/// (#288, #289).
fn dump_resolved_cst(resolved: &oz2c::imports::ResolvedSource) {
    let src = &resolved.text;
    let tree = oz2c::parse::parse(src);
    let root = tree.root_node();

    println!("resolved text: {} bytes, {} top-level nodes", src.len(), root.child_count());
    println!(
        "{:<28} {:>18}  {:>6}  {:<16}  {}",
        "kind", "bytes", "line", "origin", "first line"
    );

    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        let (line, _) = oz2c::parse::line_col(src, node.start_byte());
        let origin = resolved
            .origins
            .iter()
            .find(|(_, r)| r.contains(&node.start_byte()))
            .map(|(s, _)| s.as_str())
            .unwrap_or("main");
        let text = &src[node.byte_range()];
        let first = text.lines().next().unwrap_or("").trim();
        let first: String = first.chars().take(64).collect();

        let mut marks = String::new();
        if node.is_error() {
            marks.push_str(" <ERROR>");
        }
        if node.is_missing() {
            marks.push_str(" <MISSING>");
        }
        /* A construct absorbed into this one still shows up as a nested
         * node of its own kind, which is the tell worth flagging. */
        if let Some(kind) = absorbed_construct(node) {
            marks.push_str(&format!(" <ABSORBED {}>", kind));
        }

        println!(
            "{:<28} {:>8}..{:<8}  {:>6}  {:<16}  {}{}",
            node.kind(),
            node.start_byte(),
            node.end_byte(),
            line,
            origin,
            first,
            marks
        );
    }
}

/// The kind of a top-level construct nested inside `node` that should have
/// been a sibling of it, if there is one.
fn absorbed_construct(node: tree_sitter::Node) -> Option<&'static str> {
    const TOP_LEVEL: &[&str] = &[
        "class_interface",
        "class_implementation",
        "category_interface",
        "category_implementation",
        "protocol_declaration",
    ];
    fn walk(node: tree_sitter::Node, depth: usize) -> Option<&'static str> {
        if depth > 0 {
            if let Some(found) = TOP_LEVEL.iter().find(|k| **k == node.kind()) {
                return Some(found);
            }
        }
        let mut cursor = node.walk();
        let children: Vec<tree_sitter::Node> = node.children(&mut cursor).collect();
        children.into_iter().find_map(|c| walk(c, depth + 1))
    }
    walk(node, 0)
}
