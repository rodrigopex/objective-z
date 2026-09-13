// SPDX-License-Identifier: Apache-2.0
//
// source_map.rs -- #305: every byte of the merged buffer has to be
// traceable back to the `.m`/`.h` line it was spliced from, so a later
// pass can put a `#line` directive on it.
//
// These tests assert the *mapping*, never any generated text: for a
// snippet whose line in its own file is read back from that file on disk,
// `ResolvedSource::source_location(<offset of the snippet in the merged
// text>)` must name that file and that line.
//
// Splicing is not a copy, and the cases worth testing are exactly where
// it stops being one -- a dropped `#pragma once`, an `#import` line
// replaced by a whole file, a deleted `#ifdef __clang__` guard, the
// separator newlines pushed after a splice. Each shifts the merged line
// number away from the source line number, by a delta that varies *within*
// one file, so a test covering only the first file in the buffer would
// prove nothing.

use std::fs;
use std::path::{Path, PathBuf};

use oz_static::imports::{resolve_entry_files, resolve_imports, ResolvedSource};

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oz_static_source_map_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("inc")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    dir
}

/// 1-based line of `text` holding `needle` -- the harness shape
/// `tests/dispatch_signature_agreement.rs` uses against `Diagnostic.line`.
fn line_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line contains {:?}", needle))
        + 1
}

/// Offset of `snippet` in the merged text, insisting it appears exactly
/// once so the offset is unambiguous.
fn only_offset_of(resolved: &ResolvedSource, snippet: &str) -> usize {
    let count = resolved.text.match_indices(snippet).count();
    assert_eq!(count, 1, "{:?} appears {} times in the merged text, need 1", snippet, count);
    resolved.text.find(snippet).unwrap()
}

/// `snippet` must resolve to the line of `file` that spells it -- with the
/// expected line read back from the file on disk rather than written out
/// here, so the fixture stays the single source of truth.
fn assert_maps_to(resolved: &ResolvedSource, snippet: &str, file: &Path) {
    let offset = only_offset_of(resolved, snippet);
    let on_disk = fs::read_to_string(file).unwrap();
    let want = line_of(&on_disk, snippet);
    let (got_file, got_line) = resolved
        .source_location(offset)
        .unwrap_or_else(|| panic!("no source location for {:?} at offset {}", snippet, offset));
    assert_eq!(
        (got_file, got_line),
        (file, want),
        "{:?} is {}:{}, mapped to {}:{}",
        snippet,
        file.display(),
        want,
        got_file.display(),
        got_line
    );
}

/// The fixture every shifting case is read off: two entry `.m` files, a
/// header importing another header, a sibling `.m` pulled in with its
/// header, a `#pragma once` in each header, a `#ifdef __clang__` guard, an
/// unresolvable `#include` left as written, and a second import of an
/// already-resolved header.
fn shifting_fixture(name: &str) -> (PathBuf, ResolvedSource) {
    let dir = scratch_dir(name);
    fs::write(
        dir.join("inc/Base.h"),
        "\
#pragma once
/* BASE_HEADER_TOP */
#ifdef __clang__
@compatibility_alias Base BaseImpl;
#endif
@interface Base
- (int)baseValue;
@end
/* BASE_HEADER_TAIL */
",
    )
    .unwrap();
    fs::write(
        dir.join("src/Base.m"),
        "\
#import \"Base.h\"
/* BASE_IMPL_TOP */
@implementation Base
- (int)baseValue { return 7; }
@end
/* BASE_IMPL_TAIL */
",
    )
    .unwrap();
    fs::write(
        dir.join("inc/Mid.h"),
        "\
#pragma once
/* MID_TOP */
#import \"Base.h\"
/* MID_AFTER_IMPORT */
@interface Mid : Base
@end
/* MID_TAIL */
",
    )
    .unwrap();
    fs::write(
        dir.join("src/main.m"),
        "\
/* MAIN_TOP */
#import \"Mid.h\"
/* MAIN_AFTER_IMPORT */
#include <nowhere/absent.h>
/* MAIN_TAIL */
int main(void) { return 0; }
",
    )
    .unwrap();
    fs::write(
        dir.join("src/other.m"),
        "\
/* OTHER_TOP */
#import \"Base.h\"
/* OTHER_AFTER_IMPORT */
",
    )
    .unwrap();

    let entries = vec![dir.join("src/main.m"), dir.join("src/other.m")];
    let resolved = resolve_entry_files(&entries, &[dir.join("inc")], &[dir.join("src")])
        .unwrap_or_else(|e| panic!("resolution failed: {}", e));
    (dir, resolved)
}

/// An entry file's own lines, on both sides of the `#import` that splices
/// three files in between them -- the lines after it are where the merged
/// line number has run far ahead of the source line number.
#[test]
fn entry_file_lines_survive_the_splice_between_them() {
    let (dir, resolved) = shifting_fixture("entry");
    let main = dir.join("src/main.m");
    assert_maps_to(&resolved, "/* MAIN_TOP */", &main);
    assert_maps_to(&resolved, "/* MAIN_AFTER_IMPORT */", &main);
    assert_maps_to(&resolved, "/* MAIN_TAIL */", &main);
    assert_maps_to(&resolved, "int main(void) { return 0; }", &main);
    /* The delta, spelled out: line 3 of a file, well past line 3 of the
     * buffer. A mapping that just returned the merged line would pass
     * every "right file" assertion and fail this one. */
    let offset = only_offset_of(&resolved, "/* MAIN_AFTER_IMPORT */");
    assert_eq!(resolved.source_location(offset).map(|(_, line)| line), Some(3));
    assert!(
        resolved.text[..offset].lines().count() > 10,
        "the fixture must splice enough to make merged and source lines disagree"
    );
}

/// The `#include` that resolves to nothing is copied through as written --
/// one line in, one line out, so it maps to its own line and does not
/// disturb what follows it.
#[test]
fn an_unresolvable_include_is_mapped_where_it_stands() {
    let (dir, resolved) = shifting_fixture("include");
    assert_maps_to(&resolved, "#include <nowhere/absent.h>", &dir.join("src/main.m"));
}

/// A second entry file, spliced after everything the first one pulled in.
/// Nothing about its mapping may depend on being first in the buffer.
#[test]
fn a_second_entry_file_maps_to_itself() {
    let (dir, resolved) = shifting_fixture("second_entry");
    let other = dir.join("src/other.m");
    assert_maps_to(&resolved, "/* OTHER_TOP */", &other);
    assert_maps_to(&resolved, "/* OTHER_AFTER_IMPORT */", &other);
}

/// `#pragma once` is dropped, so every line of that header after it sits
/// one merged line earlier than its own file line says.
#[test]
fn a_dropped_pragma_once_shifts_the_header_by_one() {
    let (dir, resolved) = shifting_fixture("pragma");
    let base = dir.join("inc/Base.h");
    assert_maps_to(&resolved, "/* BASE_HEADER_TOP */", &base);
    assert_maps_to(&resolved, "/* BASE_HEADER_TAIL */", &base);
    /* Line 2 of the file, first line of its contribution -- named here
     * explicitly, since this is the delta the drop introduces. */
    let offset = only_offset_of(&resolved, "/* BASE_HEADER_TOP */");
    assert_eq!(resolved.source_location(offset).map(|(_, line)| line), Some(2));
    assert!(
        !resolved.text.contains("#pragma once"),
        "the pragma must really be dropped, or this proves nothing"
    );
}

/// `#ifdef __clang__` and its `#endif` are deleted by
/// `unwrap_clang_guard`, so the guarded line and everything after it in
/// that header shift again -- by one, then by two.
#[test]
fn a_deleted_clang_guard_shifts_the_rest_of_the_header() {
    let (dir, resolved) = shifting_fixture("clang_guard");
    let base = dir.join("inc/Base.h");
    /* Inside the guard: one line deleted ahead of it. */
    assert_maps_to(&resolved, "@compatibility_alias Base BaseImpl;", &base);
    /* After the guard: two. */
    assert_maps_to(&resolved, "- (int)baseValue;", &base);
    /* The absolute lines, so a fixture edit cannot quietly make the
     * assertions above tautological. */
    let on_disk = fs::read_to_string(&base).unwrap();
    assert_eq!(line_of(&on_disk, "@compatibility_alias Base BaseImpl;"), 4);
    assert_eq!(line_of(&on_disk, "- (int)baseValue;"), 7);
    assert!(
        !resolved.text.contains("#ifdef __clang__"),
        "the guard must really be gone, or this proves nothing:\n{}",
        resolved.text
    );
}

/// A header that itself `#import`s another: its own lines after that
/// import are the case where a *spliced* file's mapping shifts mid-file.
#[test]
fn a_header_that_imports_another_keeps_its_own_lines() {
    let (dir, resolved) = shifting_fixture("nested");
    let mid = dir.join("inc/Mid.h");
    assert_maps_to(&resolved, "/* MID_TOP */", &mid);
    assert_maps_to(&resolved, "/* MID_AFTER_IMPORT */", &mid);
    assert_maps_to(&resolved, "@interface Mid : Base", &mid);
    assert_maps_to(&resolved, "/* MID_TAIL */", &mid);
}

/// The sibling `.m` pulled in with its header shares the header's
/// `origins` stem by design -- but its bytes are the `.m`'s, and
/// `source_location` has to say so. It also arrives after a pushed
/// separator newline, which is the inserted-line case.
#[test]
fn a_sibling_implementation_maps_to_the_m_not_its_header() {
    let (dir, resolved) = shifting_fixture("sibling");
    let base_m = dir.join("src/Base.m");
    assert_maps_to(&resolved, "/* BASE_IMPL_TOP */", &base_m);
    assert_maps_to(&resolved, "- (int)baseValue { return 7; }", &base_m);
    assert_maps_to(&resolved, "/* BASE_IMPL_TAIL */", &base_m);
    /* The header shares the stem, so a stem-keyed answer would have named
     * the `.h` for these bytes. */
    let offset = only_offset_of(&resolved, "/* BASE_IMPL_TOP */");
    assert_eq!(
        resolved.source_location(offset).map(|(f, _)| f.to_path_buf()),
        Some(base_m),
        "stem-keyed provenance is not enough; the map is per byte"
    );
}

/// An `#import` of an already-resolved header becomes a one-line comment
/// -- one line in, one line out, so the mapping must not move.
#[test]
fn an_already_resolved_import_maps_to_the_import_line() {
    let (dir, resolved) = shifting_fixture("already");
    let base_m = dir.join("src/Base.m");
    /* Two files import the already-resolved `Base.h`; the first such
     * comment in the buffer is `Base.m`'s, on its line 1. */
    let offset = resolved.text.find("/* already resolved: #import \"Base.h\" */").unwrap();
    assert_eq!(resolved.source_location(offset), Some((base_m.as_path(), 1)));
}

/// The whole buffer, line by line: no offset may be left unmapped, and no
/// mapped line may point past the end of the file it names -- including
/// the separator newlines splicing invents, which belong to no file's
/// lines at all. A per-snippet test can pass while a boundary elsewhere
/// is silently wrong, so this one walks every line there is.
#[test]
fn every_merged_line_maps_inside_the_file_it_names() {
    let (_dir, resolved) = shifting_fixture("exhaustive");
    let mut offset = 0usize;
    for line in resolved.text.lines() {
        let (file, mapped) = resolved
            .source_location(offset)
            .unwrap_or_else(|| panic!("offset {} ({:?}) is unmapped", offset, line));
        let on_disk = fs::read_to_string(file).unwrap();
        let lines = on_disk.lines().count();
        assert!(
            mapped >= 1 && mapped <= lines,
            "offset {} ({:?}) mapped to {}:{}, which has {} lines",
            offset,
            line,
            file.display(),
            mapped,
            lines
        );
        offset += line.len() + 1;
    }
}

/// The strong form of the whole feature: every merged line that came from
/// a file must map to the line of that file which *is* that text -- not
/// just to somewhere in the right file. Skipped: blank lines (the
/// separators splicing inserts) and the comment it writes in place of an
/// already-resolved `#import`, neither of which is a copy of a source
/// line.
#[test]
fn every_merged_line_matches_the_source_line_it_names() {
    let (_dir, resolved) = shifting_fixture("exact");
    let mut offset = 0usize;
    let mut checked = 0usize;
    for merged_line in resolved.text.lines() {
        if merged_line.trim().is_empty() || merged_line.starts_with("/* already resolved:") {
            offset += merged_line.len() + 1;
            continue;
        }
        let (file, mapped) = resolved.source_location(offset).unwrap();
        let on_disk = fs::read_to_string(file).unwrap();
        let want = on_disk.lines().nth(mapped - 1).unwrap();
        assert_eq!(
            merged_line,
            want,
            "merged line at offset {} reads {:?} but {}:{} reads {:?}",
            offset,
            merged_line,
            file.display(),
            mapped,
            want
        );
        checked += 1;
        offset += merged_line.len() + 1;
    }
    assert!(checked >= 20, "only {} lines checked -- the fixture shrank", checked);
}

/// The map is segment-shaped, not a per-line table: a run of plain lines
/// costs one segment however long it is.
#[test]
fn the_map_stays_segment_shaped() {
    let dir = scratch_dir("segments");
    let mut body = String::from("#pragma once\n");
    for i in 1..=200 {
        body.push_str(&format!("/* filler {} */\n", i));
    }
    fs::write(dir.join("inc/Long.h"), &body).unwrap();
    fs::write(dir.join("src/main.m"), "#import \"Long.h\"\n/* TAIL */\n").unwrap();

    let resolved =
        resolve_entry_files(&[dir.join("src/main.m")], &[dir.join("inc")], &[dir.join("src")])
            .unwrap();
    let merged_lines = resolved.text.lines().count();
    assert!(merged_lines > 200, "merged lines: {}", merged_lines);
    assert!(
        resolved.source_map.segment_count() <= 4,
        "{} lines mapped by {} segments -- that is a per-line table, not a segment map",
        merged_lines,
        resolved.source_map.segment_count()
    );
    assert_maps_to(&resolved, "/* filler 200 */", &dir.join("inc/Long.h"));
    assert_maps_to(&resolved, "/* TAIL */", &dir.join("src/main.m"));
}

/// Step 1's own deliverable: the path is kept beside the stem, so a
/// consumer holding only a stem (the emit buckets, the output paths) can
/// still name a file. A stem shared by a header and its sibling `.m`
/// keeps the header, which is why per-byte provenance exists alongside it.
#[test]
fn every_stem_keeps_the_path_it_was_read_from() {
    let (dir, resolved) = shifting_fixture("stems");
    let path_of = |stem: &str| resolved.stem_paths.get(stem).cloned();
    assert_eq!(path_of("main"), Some(dir.join("src/main.m")));
    assert_eq!(path_of("other"), Some(dir.join("src/other.m")));
    assert_eq!(path_of("Mid"), Some(dir.join("inc/Mid.h")));
    assert_eq!(path_of("Base"), Some(dir.join("inc/Base.h")));
    /* No stem left with only a name -- that was the state #305 filed. */
    for (stem, _) in &resolved.origins {
        assert!(resolved.stem_paths.contains_key(stem), "stem {:?} has no path", stem);
    }
}

/// `parse::repair_bare_macro_statements` is documented offset-preserving,
/// and is -- but it is **not** line-preserving: the whitespace byte it
/// overwrites with `;` is normally the macro line's own newline (its
/// candidate test requires a newline in the gap). So the repaired text
/// `emit` works on has fewer lines than the merged text, while every byte
/// offset still agrees.
///
/// That is why `source_location` resolves lines against the buffer as
/// spliced and takes no text argument: an offset taken from the repaired
/// text still answers correctly, and there is no wrong text to pass.
#[test]
fn the_repair_preserves_offsets_but_eats_a_newline() {
    let dir = scratch_dir("repair");
    fs::write(
        dir.join("src/main.m"),
        "\
#define OBS_DECLARE(n) extern const int n;
#define ADD_OBS(c, n, prio) extern const int n;
OBS_DECLARE(marker)
ADD_OBS(some_chan, marker, 4);
/* AFTER_REPAIR */
",
    )
    .unwrap();
    let resolved = resolve_entry_files(&[dir.join("src/main.m")], &[], &[dir.join("src")]).unwrap();

    let (repaired, slots) = oz_static::parse::repair_bare_macro_statements(&resolved.text);
    assert!(!slots.is_empty(), "the fixture must actually trigger a repair");
    assert_eq!(repaired.len(), resolved.text.len(), "the repair must preserve every offset");
    assert!(
        repaired.lines().count() < resolved.text.lines().count(),
        "this test exists because the repair eats a newline; if it stops doing so, say so \
         here rather than deleting the guard"
    );

    /* An offset located in the repaired text, resolved against the map. */
    let offset = repaired.find("/* AFTER_REPAIR */").unwrap();
    let on_disk = fs::read_to_string(dir.join("src/main.m")).unwrap();
    assert_eq!(
        resolved.source_location(offset),
        Some((dir.join("src/main.m").as_path(), line_of(&on_disk, "AFTER_REPAIR")))
    );
}

/// `resolve_imports` takes text rather than a path, so its entry lines
/// have no real file to name: they get the name that text would have had,
/// and every spliced file is still a real path.
#[test]
fn text_entry_resolution_names_the_file_the_text_would_have_been() {
    let dir = scratch_dir("text_entry");
    fs::write(dir.join("inc/Helper.h"), "#pragma once\n/* HELPER */\n").unwrap();
    let src = "#import \"Helper.h\"\n/* CALLER */\n";
    let resolved = resolve_imports(src, &dir, &[dir.join("inc")], &[], "main").unwrap();
    let caller = resolved.text.find("/* CALLER */").unwrap();
    assert_eq!(
        resolved.source_location(caller),
        Some((dir.join("main.m").as_path(), 2)),
        "merged:\n{}",
        resolved.text
    );
    let helper = resolved.text.find("/* HELPER */").unwrap();
    assert_eq!(resolved.source_location(helper), Some((dir.join("inc/Helper.h").as_path(), 2)));
}

/// An offset past the end of the buffer has no location, rather than the
/// last one the map happened to see.
#[test]
fn an_offset_past_the_end_has_no_location() {
    let (_dir, resolved) = shifting_fixture("past_end");
    assert_eq!(resolved.source_location(resolved.text.len()), None);
    assert_eq!(resolved.source_location(resolved.text.len() + 1), None);
}
