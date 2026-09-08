// SPDX-License-Identifier: Apache-2.0
//
// line_directives.rs -- #305 step 3: the generated C carries `#line`
// directives, so the toolchain resolves a transpiled program's addresses
// to the Objective-C it was written in.
//
// These tests assert the **mapping**, never the text: for a snippet whose
// line in its own file is read back from that file on disk, the position
// the generated C attributes to the line spelling that snippet must be
// that file and that line. `attributed_position` computes it the way the
// C preprocessor does -- walking the emitted lines and following every
// directive -- so what is asserted is what `gdb`, `addr2line` and DWARF
// will say, not merely that some plausible directive is present.
//
// The cases worth covering are the ones a naive implementation gets
// wrong, and both are about *not* counting lines locally:
//
//   - a statement in a file reached through an `#import`, where splicing
//     has moved the merged line far away from the source line;
//   - a file whose bare macro invocation `parse::repair_bare_macro_
//     statements` repairs, which is offset-preserving but **not**
//     line-preserving: the repaired text `emit` walks has one line fewer
//     per repair while every byte offset still agrees, so anything that
//     derived a line from that text would be quietly wrong.
//
// `tests/source_map.rs` covers the map underneath this (#305 steps 1-2).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use oz_static::imports::{resolve_entry_files, ResolvedSource};
use oz_static::Options;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oz_static_line_directives_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("inc")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

/// 1-based line of `text` holding `needle` -- the harness shape
/// `tests/dispatch_signature_agreement.rs` uses against `Diagnostic.line`,
/// and `tests/source_map.rs` against the map. Expected lines are read off
/// disk with this and never hand-counted.
fn line_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line contains {:?}", needle))
        + 1
}

/// The file and line the generated text attributes to the line spelling
/// `snippet`, computed exactly as the preprocessor does: a
/// `#line N "file"` puts the *following* line at line N of `file`, and
/// every line after it counts on from there.
///
/// `generated_name` stands in for the name the compiler would use before
/// any directive -- the generated file's own.
fn attributed_position(generated: &str, snippet: &str, generated_name: &str) -> (String, usize) {
    attributed_where(generated, generated_name, &|line| line.contains(snippet))
        .unwrap_or_else(|| panic!("no generated line contains {:?} in:\n{}", snippet, generated))
}

/// `attributed_position` for a snippet that also appears somewhere it
/// must not be matched -- a hoisted block's prototype as well as its
/// definition, say.
fn attributed_where(
    generated: &str,
    generated_name: &str,
    wanted: &dyn Fn(&str) -> bool,
) -> Option<(String, usize)> {
    let mut file = generated_name.to_string();
    let mut line = 1usize;
    for text in generated.lines() {
        if let Some(rest) = text.trim_start().strip_prefix("#line ") {
            let (number, path) = rest.split_once(' ').unwrap_or((rest, ""));
            line = number.trim().parse().unwrap_or_else(|_| {
                panic!("unparsable #line directive: {:?}", text);
            });
            if !path.is_empty() {
                file = path.trim().trim_matches('"').to_string();
            }
            continue;
        }
        /* A comment line is never the code being asked about: every
         * translated statement is emitted under a `/* <original> */`
         * restatement of itself, which would otherwise match the snippet
         * one line before the directive that belongs to it. Counted, since
         * it occupies a line -- just never matched. */
        let is_comment = {
            let trimmed = text.trim_start();
            trimmed.starts_with("/*") || trimmed.starts_with('*')
        };
        if !is_comment && wanted(text) {
            return Some((file, line));
        }
        line += 1;
    }
    None
}

/// One generated `.c`, by stem.
fn source_c(files: &[(String, String, String)], stem: &str) -> String {
    files
        .iter()
        .find(|(s, _, _)| s == stem)
        .map(|(_, _, c)| c.clone())
        .unwrap_or_else(|| panic!("no generated source for stem {:?}", stem))
}

/// Transpile a resolution with `#line` directives on, the way `oz2c
/// --line-directives` (`CONFIG_OBJZ_DEBUG_LINES=y`) does: the resolution's
/// own source map, and a directory per stem so a directive naming the
/// generated file back can name it absolutely.
fn transpile_with_directives(
    resolved: &ResolvedSource,
    outdir: &Path,
) -> Vec<(String, String, String)> {
    let mut generated_dirs: HashMap<String, PathBuf> = HashMap::new();
    for (stem, _) in &resolved.origins {
        generated_dirs.insert(stem.clone(), outdir.to_path_buf());
    }
    let options = Options {
        source_map: Some(resolved.source_map.clone()),
        generated_dirs,
        header_ranges: resolved.header_ranges.clone(),
        ..Default::default()
    };
    oz_static::transpile_split_with_options(&resolved.text, &resolved.origins, &options)
        .unwrap_or_else(|diags| panic!("transpile failed: {:?}", diags))
        .files
}

/// The same resolution with directives off -- `Options::default()`, which
/// is every existing caller.
fn transpile_without_directives(resolved: &ResolvedSource) -> Vec<(String, String, String)> {
    let options =
        Options { header_ranges: resolved.header_ranges.clone(), ..Default::default() };
    oz_static::transpile_split_with_options(&resolved.text, &resolved.origins, &options)
        .unwrap_or_else(|diags| panic!("transpile failed: {:?}", diags))
        .files
}

/// The generated line spelling `emitted` must be attributed to the line of
/// `file` that spells `written` -- the expected line read back off disk
/// rather than written out here.
///
/// Two snippets because a translated statement is not spelled the same in
/// both places (`_n = _n + 1;` becomes `self->_n = self->_n + 1;`); pass
/// the same string twice for one that survives verbatim.
fn assert_attributed(generated: &str, stem: &str, file: &Path, written: &str, emitted: &str) {
    let on_disk = fs::read_to_string(file).unwrap();
    let want = line_of(&on_disk, written);
    let (got_file, got_line) =
        attributed_position(generated, emitted, &format!("{}.c", stem));
    assert_eq!(
        (got_file.as_str(), got_line),
        (file.to_string_lossy().as_ref(), want),
        "{:?} is {}:{}, but the generated {:?} is attributed to {}:{}",
        written,
        file.display(),
        want,
        emitted,
        got_file,
        got_line
    );
}

/// A two-file program: a class in its own header with its implementation
/// in the sibling `.m`, and an entry `.m` that imports it and holds a
/// plain C `main`. Every emission point step 3 touches is in here -- a
/// method body, a plain C function body, a hoisted block -- and the
/// entry file's own lines sit *after* an `#import` that splices two whole
/// files in, so its merged lines are nowhere near its source lines.
fn program_fixture(name: &str) -> (PathBuf, ResolvedSource) {
    let dir = scratch_dir(name);
    fs::write(
        dir.join("inc/Counter.h"),
        "\
#pragma once
/* COUNTER_HEADER */
@interface Counter
- (int)bump;
- (int)value;
- (int)guarded;
@end
",
    )
    .unwrap();
    fs::write(
        dir.join("src/Counter.m"),
        "\
#import \"Counter.h\"

@implementation Counter {
	int _n;
}

- (int)bump
{
	_n = _n + 1;
	return _n;
}

- (int)value
{
	return _n;
}

- (int)guarded
{
	int seen = 0;
	@synchronized (self) {
		seen = 1;
	}
	return seen;
}

@end
",
    )
    .unwrap();
    fs::write(
        dir.join("src/main.m"),
        "\
#import \"Counter.h\"

static int doubled(int v)
{
	int scaled = v * 2;
	return scaled;
}

static int twiced(int v)
{
	int (^twice)(int) = ^(int v) { return v * 2; };
	return twice(v);
}

int main(void)
{
	Counter *c = [Counter alloc];
	int bumped = [c bump];
	int out = doubled(bumped) + twiced(bumped);
	return out;
}
",
    )
    .unwrap();

    let entries = vec![dir.join("src/main.m")];
    let resolved = resolve_entry_files(&entries, &[dir.join("inc")], &[dir.join("src")])
        .unwrap_or_else(|e| panic!("resolution failed: {}", e));
    (dir, resolved)
}

/// A `for-in` loop over a typed array: the one shape that splices a
/// rendered body in *after text on the same line* (`for (...) <body>`), so
/// it is the shape a leading directive on that body breaks. Needs the real
/// Foundation, because for-in is protocol dispatch -- resolved from the
/// repository's own `include/oz_sdk` and `src`, exactly as `oz2c` does by
/// default.
fn for_in_fixture(name: &str) -> (PathBuf, ResolvedSource) {
    let dir = scratch_dir(name);
    fs::write(
        dir.join("src/main.m"),
        "\
#import <Foundation/Foundation.h>

int main(void)
{
	OZArray<OZString *> *names = @[ @\"alpha\", @\"beta\" ];
	for (OZString *name in names) {
		OZLog(\"name: %@\", name);
	}
	return 0;
}
",
    )
    .unwrap();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let resolved = resolve_entry_files(
        &[dir.join("src/main.m")],
        &[repo.join("include/oz_sdk")],
        &[repo.join("src"), dir.join("src")],
    )
    .unwrap_or_else(|e| panic!("resolution failed: {}", e));
    (dir, resolved)
}

/// A method body statement, in the file reached through the `#import` --
/// the case the whole issue is about, and the one a merged line number
/// gets wrong.
#[test]
fn a_method_body_statement_is_attributed_to_its_own_file_and_line() {
    let (dir, resolved) = program_fixture("method_body");
    let files = transpile_with_directives(&resolved, &dir);
    let counter_c = source_c(&files, "Counter");
    let counter_m = dir.join("src/Counter.m");

    assert_attributed(&counter_c, "Counter", &counter_m, "_n = _n + 1;", "_n = self->_n + 1;");
    assert_attributed(&counter_c, "Counter", &counter_m, "return _n;", "return self->_n;");

    /* The delta spelled out, so this cannot pass on a merged line that
     * happens to agree: the statement is line 9 of a 20-line file, and
     * well past line 9 of the buffer emit walks. */
    let (_, line) = attributed_position(&counter_c, "_n = self->_n + 1;", "Counter.c");
    assert_eq!(line, 9, "the fixture moved -- expected `_n = _n + 1;` on line 9");
    let merged = resolved.text.find("_n = _n + 1;").unwrap();
    assert!(
        resolved.text[..merged].lines().count() > 12,
        "the fixture must splice enough for merged and source lines to disagree"
    );
}

/// The method's own definition line, which is what a backtrace naming the
/// function resolves to before it reaches any statement.
#[test]
fn a_method_definition_is_attributed_to_its_signature_line() {
    let (dir, resolved) = program_fixture("method_signature");
    let files = transpile_with_directives(&resolved, &dir);
    let counter_c = source_c(&files, "Counter");
    let counter_m = dir.join("src/Counter.m");
    let on_disk = fs::read_to_string(&counter_m).unwrap();

    let (file, line) = attributed_position(&counter_c, "int Counter_bump(", "Counter.c");
    assert_eq!(
        (file.as_str(), line),
        (counter_m.to_string_lossy().as_ref(), line_of(&on_disk, "- (int)bump")),
        "the generated definition must sit on the `- (int)bump` line"
    );
}

/// A plain C function at file scope: its body statements *and* its
/// signature line, which is the line someone types (`break main.m:73` on
/// px-keyboard's own `int main(void)`). The Proposal named only
/// `render_method_definition`; nothing about this is method-specific.
#[test]
fn a_plain_c_function_body_is_attributed_to_its_own_lines() {
    let (dir, resolved) = program_fixture("c_function");
    let files = transpile_with_directives(&resolved, &dir);
    let main_c = source_c(&files, "main");
    let main_m = dir.join("src/main.m");

    assert_attributed(&main_c, "main", &main_m, "int scaled = v * 2;", "int scaled = v * 2;");
    assert_attributed(&main_c, "main", &main_m, "return scaled;", "return scaled;");
    assert_attributed(
        &main_c,
        "main",
        &main_m,
        "int bumped = [c bump];",
        "int bumped = Counter_bump(",
    );

    /* The signature lines themselves, both functions. */
    let on_disk = fs::read_to_string(&main_m).unwrap();
    for signature in ["static int doubled(int v)", "int main(void)"] {
        let (file, line) = attributed_position(&main_c, signature, "main.c");
        assert_eq!(
            (file.as_str(), line),
            (main_m.to_string_lossy().as_ref(), line_of(&on_disk, signature)),
            "the generated {:?} must sit on its own source line",
            signature
        );
    }
}

/// A hoisted block: the directive on the synthesized function, and the
/// `.m` line in its **name**, which is what a backtrace shows first.
/// `oz_block_L3271_C38_1` shipped for a block on line 56 of a 210-line
/// file -- 3271 was its line in the spliced buffer.
#[test]
fn a_hoisted_block_carries_its_source_line_in_the_name_and_a_directive() {
    let (dir, resolved) = program_fixture("block");
    let files = transpile_with_directives(&resolved, &dir);
    let main_c = source_c(&files, "main");
    let main_m = dir.join("src/main.m");
    let on_disk = fs::read_to_string(&main_m).unwrap();

    let want = line_of(&on_disk, "^(int v) { return v * 2; }");
    /* Columns are 1-based bytes, the same convention `parse::line_col`
     * uses -- so the tab indenting the line counts as one. The block
     * literal's own `^`, not the block-pointer declarator's. */
    let column = on_disk.lines().nth(want - 1).unwrap().find("^(int v)").unwrap() + 1;
    let name = format!("oz_block_L{}_C{}_1", want, column);
    assert!(
        main_c.contains(&name),
        "no hoisted block named {:?} (the `.m` line and column) in:\n{}",
        name,
        main_c
    );

    /* And the definition sits on that line, so a backtrace through the
     * symbol lands in the `.m`. The prototype ahead of the call sites
     * spells the same signature, so the definition is the one that is
     * not a declaration. */
    let signature = format!("int {}(int v)", name);
    let (file, line) =
        attributed_where(&main_c, "main.c", &|l| l.contains(&signature) && !l.ends_with(';'))
            .unwrap_or_else(|| panic!("no definition of {} in:\n{}", name, main_c));
    assert_eq!((file.as_str(), line), (main_m.to_string_lossy().as_ref(), want));

    /* The merged position, for contrast: this is what the name used to
     * carry, and it names nowhere. */
    let merged_line = resolved.text[..resolved.text.find("^(int v)").unwrap()].lines().count() + 1;
    assert!(
        merged_line > want,
        "merged line {} vs source line {} -- the fixture stopped proving anything",
        merged_line,
        want
    );
}

/// A bare macro invocation, which `parse::repair_bare_macro_statements`
/// terminates by overwriting the newline after its `)` with a `;`. Every
/// byte offset survives; one *line* does not. So everything after the
/// repair is where an implementation that counted newlines in the text
/// `emit` walks would be exactly one line short -- per repair.
#[test]
fn a_repaired_bare_macro_does_not_shift_what_follows_it() {
    let dir = scratch_dir("repair");
    fs::write(
        dir.join("src/main.m"),
        "\
#define OBS_DECLARE(n) extern const int n;
#define ADD_OBS(c, n, prio) extern const int n;
@interface Gadget {
	int _n;
}
- (int)run;
@end
OBS_DECLARE(first_marker)
ADD_OBS(chan, second_marker, 3)
@implementation Gadget
- (int)run
{
	int local = 41;
	return local + _n;
}
@end
",
    )
    .unwrap();
    let resolved =
        resolve_entry_files(&[dir.join("src/main.m")], &[dir.join("inc")], &[dir.join("src")])
            .unwrap();
    /* The repair has to really happen, or this proves nothing. */
    let repaired = oz_static::parse::repair_bare_macro_statements(&resolved.text).0;
    assert!(
        repaired.lines().count() < resolved.text.lines().count(),
        "no line was eaten -- the fixture no longer triggers the repair"
    );

    let files = transpile_with_directives(&resolved, &dir);
    let main_c = source_c(&files, "main");
    let main_m = dir.join("src/main.m");
    /* Both statements sit after the repair, and one of them is
     * translated, so the body takes the per-statement path rather than
     * the verbatim one -- which is where a locally counted line would
     * show up as an off-by-one-per-repair. */
    assert_attributed(&main_c, "main", &main_m, "int local = 41;", "int local = 41;");
    assert_attributed(
        &main_c,
        "main",
        &main_m,
        "return local + _n;",
        "return local + self->_n;",
    );
}

/// Code oz_static synthesized -- a hoisted prototype, a slab definition,
/// a dispatch thunk -- stays attributed to the generated file, which is
/// where it genuinely lives. Every such directive has to name the line it
/// is really on, or `list` would show the wrong generated code; and at
/// least one of them has to follow a `.m` directive, or the resets are
/// decorative and a section ending inside a method body would leak its
/// position into the synthesized code after it.
#[test]
fn synthesized_code_stays_attributed_to_the_generated_file() {
    let (dir, resolved) = program_fixture("synthesized");
    let files = transpile_with_directives(&resolved, &dir);

    let mut resets_after_source = 0usize;
    for (stem, h, c) in &files {
        for (text, extension) in [(h, "h"), (c, "c")] {
            let generated = dir.join(format!("{}.{}", stem, extension));
            let generated = generated.to_string_lossy().to_string();
            let mut in_source_file = false;
            for (index, line) in text.lines().enumerate() {
                let Some(rest) = line.trim_start().strip_prefix("#line ") else {
                    continue;
                };
                let (number, path) = rest.split_once(' ').unwrap();
                let path = path.trim().trim_matches('"');
                let claimed: usize = number.trim().parse().unwrap();
                if path != generated {
                    /* A `.m`/`.h` the author wrote -- covered by the
                     * mapping tests above. */
                    in_source_file = true;
                    continue;
                }
                assert_eq!(
                    claimed,
                    index + 2,
                    "a reset in {:?} claims line {} while sitting on line {}",
                    generated,
                    claimed,
                    index + 1
                );
                if in_source_file {
                    resets_after_source += 1;
                    in_source_file = false;
                }
            }
        }
    }
    assert!(
        resets_after_source > 0,
        "no section was handed back to a generated file after a .m one -- \
         synthesized code is inheriting a source position"
    );
}

/// The off switch, which is the absence of a map: `Options::default()` --
/// what every other caller in the tree passes, and what `oz2c` without
/// `--line-directives` passes -- emits not one directive, and the same
/// bytes it emitted before any of this existed.
#[test]
fn no_map_means_no_directives_and_identical_bytes() {
    let (dir, resolved) = program_fixture("off");
    let off = transpile_without_directives(&resolved);
    for (stem, h, c) in &off {
        assert!(!c.contains("#line"), "stem {:?} .c carries a directive:\n{}", stem, c);
        assert!(!h.contains("#line"), "stem {:?} .h carries a directive:\n{}", stem, h);
    }

    /* And the directives are the only difference *to the code*: strip them
     * from the on-version, normalize the one identifier that deliberately
     * differs -- a hoisted block is named after where it was written, so
     * with a map that is the `.m` line and without one the merged line --
     * and the two agree line for line. */
    let on = transpile_with_directives(&resolved, &dir);
    assert_eq!(on.len(), off.len());
    for ((stem, on_h, on_c), (_, off_h, off_c)) in on.iter().zip(off.iter()) {
        for (with, without, which) in [(on_h, off_h, "header"), (on_c, off_c, "source")] {
            let code_only = |text: &String| -> Vec<String> {
                text.lines()
                    .filter(|l| {
                        let t = l.trim_start();
                        !t.starts_with("#line ") && !t.starts_with("/* block at ")
                    })
                    .map(anonymize_block_positions)
                    .collect()
            };
            assert_eq!(
                code_only(with),
                code_only(without),
                "stem {:?} {} differs by more than its directives",
                stem,
                which
            );
        }
    }
}

/// Replace every `L<digits>_C<digits>` in a line with `L_C`, so two
/// renderings can be compared without the position a hoisted block is
/// named after -- the `.m`'s line with a source map, the merged buffer's
/// without one.
fn anonymize_block_positions(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("_L") {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        let digits = tail[2..].chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            out.push_str("_L");
            rest = &tail[2..];
            continue;
        }
        out.push_str("_L");
        rest = &tail[2 + digits..];
        if let Some(after_c) = rest.strip_prefix("_C") {
            let digits = after_c.chars().take_while(char::is_ascii_digit).count();
            out.push_str("_C");
            rest = &after_c[digits..];
        }
    }
    out.push_str(rest);
    out
}

/// A preprocessing directive may be indented but may not share its line
/// with anything else. Nothing that assembles generated text may splice a
/// directive into the middle of a line, and the shape that proved it is a
/// **nested** body: `@synchronized (self) <body>` and
/// `for (OZString *name in names) <body>` both render a nested body, and
/// the for-in puts it after text on the same line -- so a leading
/// directive on that body arrived as `for (...) #line 29 "main.m"`:
/// `error: stray '#' in program`, an ARM build failure
/// (`samples/transpiled_generics`) that no host test noticed.
#[test]
fn no_directive_shares_a_line_with_code() {
    let (plain_dir, plain) = program_fixture("own_line");
    let (loop_dir, looping) = for_in_fixture("own_line_forin");
    let mut files = transpile_with_directives(&plain, &plain_dir);
    files.extend(transpile_with_directives(&looping, &loop_dir));
    let mut checked = 0usize;
    for (stem, h, c) in &files {
        for text in [h, c] {
            for (index, line) in text.lines().enumerate() {
                if !line.contains("#line ") {
                    continue;
                }
                assert!(
                    line.trim_start().starts_with("#line "),
                    "{:?} line {}: a directive shares its line with code -- {:?}",
                    stem,
                    index + 1,
                    line
                );
                /* And nothing after it either: everything a directive says
                 * is over at the end of its own line. */
                assert!(
                    line.matches("#line ").count() == 1 && line.ends_with('"'),
                    "{:?} line {}: something follows the directive -- {:?}",
                    stem,
                    index + 1,
                    line
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 10, "only {} directives seen -- the fixture shrank", checked);
}

/// Every directive names an absolute path, so a debugger resolves it from
/// whatever directory it happens to be run in.
#[test]
fn every_directive_names_an_absolute_path() {
    let (dir, resolved) = program_fixture("absolute");
    let files = transpile_with_directives(&resolved, &dir);
    let mut seen = 0usize;
    for (stem, h, c) in &files {
        for text in [h, c] {
            for line in text.lines() {
                let Some(rest) = line.trim_start().strip_prefix("#line ") else {
                    continue;
                };
                let path = rest.split_once(' ').map(|(_, p)| p.trim().trim_matches('"'));
                let path = path.unwrap_or_else(|| panic!("{:?}: bare directive {:?}", stem, line));
                assert!(
                    Path::new(path).is_absolute(),
                    "{:?}: directive names a relative path: {:?}",
                    stem,
                    line
                );
                seen += 1;
            }
        }
    }
    assert!(seen > 10, "only {} directives emitted -- the fixture shrank", seen);
}
