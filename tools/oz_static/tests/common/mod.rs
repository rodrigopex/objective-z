// SPDX-License-Identifier: Apache-2.0
//
// common/mod.rs - shared test helper: transpile, compile with the real
// PAL host headers, link, and run. Validates the static subset produces
// real, working C -- not just text that merely parses.
//
// Each test binary in tests/ compiles this module separately and uses
// only a subset of it, so per-binary "never used" warnings are expected.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// `tools/oz_static/../../include` -- the repo's real platform headers.
fn include_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../include")
}

/// Transpile `source`, compile the primary output + companion file against
/// the real PAL (host backend), link, run, and return captured stdout.
/// Panics with full diagnostics/compiler output on any failure.
pub fn compile_and_run(source: &str, stem: &str) -> String {
    let out = oz_static::transpile(source).unwrap_or_else(|diags| {
        panic!(
            "transpile('{}') was expected to succeed but produced diagnostics:\n{}",
            stem,
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });

    let dir = std::env::temp_dir().join(format!("oz_static_test_{}", stem));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let main_c = dir.join(format!("{}.c", stem));
    fs::write(&main_c, &out.source_c).unwrap();
    fs::write(dir.join("oz_static_dispatch.h"), &out.companion_h).unwrap();
    let dispatch_c = dir.join("oz_static_dispatch.c");
    fs::write(&dispatch_c, &out.companion_c).unwrap();

    let main_o = dir.join("main.o");
    let dispatch_o = dir.join("dispatch.o");
    let bin = dir.join("bin");

    cc(&["-DOZ_PLATFORM_HOST", "-I", include_dir().to_str().unwrap(), "-I",
         dir.to_str().unwrap(), "-c", main_c.to_str().unwrap(), "-o", main_o.to_str().unwrap()]);
    cc(&["-DOZ_PLATFORM_HOST", "-I", include_dir().to_str().unwrap(), "-I",
         dir.to_str().unwrap(), "-c", dispatch_c.to_str().unwrap(), "-o", dispatch_o.to_str().unwrap()]);
    cc(&[main_o.to_str().unwrap(), dispatch_o.to_str().unwrap(), "-o", bin.to_str().unwrap()]);

    let run = Command::new(&bin).output().unwrap_or_else(|e| panic!("failed to run binary: {}", e));
    assert!(run.status.success(), "binary exited non-zero: {:?}\nstdout: {}\nstderr: {}",
            run.status, String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));

    String::from_utf8(run.stdout).unwrap()
}

fn cc(args: &[&str]) {
    let output = Command::new("cc").args(args).output().unwrap_or_else(|e| panic!("failed to run cc: {}", e));
    assert!(
        output.status.success(),
        "cc {:?} failed:\n{}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Transpile `source`, expecting it to be rejected. Returns the joined
/// diagnostic messages for substring assertions.
pub fn expect_reject(source: &str) -> String {
    match oz_static::transpile(source) {
        Ok(_) => panic!("expected transpile to be rejected by the static bar, but it succeeded"),
        Err(diags) => diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"),
    }
}

// ---------------------------------------------------------------------------
// Foundation class fixtures -- derived from the real source, not hand-copied
// ---------------------------------------------------------------------------
//
// Every fixture below is assembled at test-run time from the actual
// `src/*.m` / `include/oz_sdk/Foundation/*.h` files via `include_str!` --
// the same files clangd and the Python pipeline use -- instead of being
// retyped into a Rust string literal. That guarantees these fixtures can
// never silently drift from the real Foundation classes: if a class's
// real source changes, its test fixture picks up the change automatically
// (or the corresponding cut-list marker below stops matching and panics
// loudly, telling us exactly what needs updating).
//
// oz_static has no `#import`/`#include` resolution -- it parses one
// file's text as-is -- so a class's header and implementation still need
// to be combined into a single translation unit. `assemble` below does
// only that mechanical join, plus stripping the two lines that only make
// sense across multiple files (`#pragma once`, `#import ...`) and
// unwrapping the `#ifdef __clang__ / @compatibility_alias .../ #endif`
// guard some headers use (oz_static's top-level emit pass elides a bare
// `compatibility_alias_declaration` to a comment, but doesn't recurse
// into `#ifdef`/`#endif` conditionals to find one nested inside, so left
// wrapped it would pass through as invalid raw ObjC text -- and the
// `#ifdef` was only ever a compiler-portability guard in the original
// anyway, since this harness's `cc` always defines `__clang__`).
//
// A few classes (OZArray; OZQ31 for one method) need real content
// removed or added on top of that, because they use something oz_static
// can't yet resolve or a cross-file dependency this host harness can't
// pull in. Those cuts are done as small, named, marker-anchored
// transforms (`remove_line_containing`/`remove_line_range`/
// `remove_method_body`) over the real included text -- so they still
// track the real file for everything else, and panic loudly (rather than
// silently no-op) if a marker stops matching a future edit to that file.

/// Drop `#pragma once` and `#import ...` lines -- meaningless once this
/// is inlined into a single generated translation unit rather than
/// `#import`ed (oz_static has no import/include resolution at all).
fn strip_import_and_pragma_lines(src: &str) -> String {
    src.lines()
        .filter(|line| {
            let t = line.trim_start();
            !(t.starts_with("#pragma once") || t.starts_with("#import "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Join a class's real header + implementation into one translation
/// unit, applying only the two generic, meaning-preserving adaptations
/// every class needs (see module doc comment). The `#ifdef __clang__`
/// guard unwrap is shared with `oz_static::imports` (OZ-094's real
/// `#import` resolver hits the exact same headers) rather than
/// duplicated here.
fn assemble(header: &str, implementation: &str) -> String {
    format!(
        "{}\n{}\n",
        strip_import_and_pragma_lines(&oz_static::imports::unwrap_clang_guard(header)),
        strip_import_and_pragma_lines(implementation)
    )
}

/// Remove every line containing `marker`. Panics if none matched, so a
/// cut list that stops applying (the real file changed) fails loudly
/// instead of silently leaving stale content in place.
fn remove_line_containing(src: &str, marker: &str) -> String {
    let mut found = false;
    let kept: Vec<&str> = src
        .lines()
        .filter(|l| {
            let matches = l.contains(marker);
            found |= matches;
            !matches
        })
        .collect();
    assert!(found, "remove_line_containing: marker {:?} not found", marker);
    kept.join("\n") + "\n"
}

/// Remove every line from the first line containing `start_marker`
/// through the next line containing `end_marker` (inclusive of both).
/// For a multi-line declaration/signature with no braces to balance.
fn remove_line_range(src: &str, start_marker: &str, end_marker: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains(start_marker))
        .unwrap_or_else(|| panic!("remove_line_range: start marker {:?} not found", start_marker));
    let end = lines[start..]
        .iter()
        .position(|l| l.contains(end_marker))
        .map(|i| start + i)
        .unwrap_or_else(|| {
            panic!("remove_line_range: end marker {:?} not found after start", end_marker)
        });
    let mut out: Vec<&str> = lines[..start].to_vec();
    out.extend_from_slice(&lines[end + 1..]);
    out.join("\n") + "\n"
}

/// Remove a whole method (signature through its matching closing brace)
/// starting from the line containing `signature_marker`, by counting
/// brace depth -- robust to the signature's `{` being on the same line
/// or its own line.
fn remove_method_body(src: &str, signature_marker: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let start = lines.iter().position(|l| l.contains(signature_marker)).unwrap_or_else(|| {
        panic!("remove_method_body: signature marker {:?} not found", signature_marker)
    });
    let mut depth: i32 = 0;
    let mut seen_open = false;
    let mut end = start;
    for (i, line) in lines[start..].iter().enumerate() {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    seen_open = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if seen_open && depth == 0 {
            end = start + i;
            break;
        }
    }
    assert!(seen_open && depth == 0, "remove_method_body: no matching closing brace for {:?}", signature_marker);
    let mut out: Vec<&str> = lines[..start].to_vec();
    out.extend_from_slice(&lines[end + 1..]);
    out.join("\n") + "\n"
}

/// OZObject, the real Foundation root class -- assembled from
/// `include/oz_sdk/Foundation/OZObject.h` / `src/OZObject.m` verbatim
/// (only the two generic adaptations from the module doc comment).
pub fn ozobject_src() -> String {
    assemble(
        include_str!("../../../../include/oz_sdk/Foundation/OZObject.h"),
        include_str!("../../../../src/OZObject.m"),
    )
}

/// OZQ31, the fixed-point Foundation class -- assembled from
/// `include/oz_sdk/Foundation/OZQ31.h` / `src/OZQ31.m` verbatim (same
/// helper function bodies and method bodies, including `other->_raw`
/// cross-instance ivar access and `[[OZQ31 alloc] init]` chaining), with
/// one addition: the real `-cDescription:maxLength:` calls
/// `_oz_get_log_precision()`, defined in `src/OZLog.c` -- which needs
/// Zephyr's `printk` and the Python pipeline's own generated dispatch
/// headers, neither available on host. A small stub supplies it (always
/// -1, i.e. `-cDescription:maxLength:`'s default 14-digit precision), so
/// log-precision-specific behavior (`%.N@` format specifiers) isn't
/// exercised by any test here. Requires `OZObject` (`common::ozobject_src`)
/// in scope as the root class.
pub fn ozq31_src() -> String {
    let assembled = assemble(
        include_str!("../../../../include/oz_sdk/Foundation/OZQ31.h"),
        include_str!("../../../../src/OZQ31.m"),
    );
    format!(
        "/* synthesized stub (not from source): the real _oz_get_log_precision\n * \
lives in src/OZLog.c, which needs Zephyr's printk plus the Python\n * \
pipeline's own generated dispatch headers -- neither available on host.\n * \
Plain (not static/inline): oz_static's own companion header now declares\n * \
this symbol too (see companion.rs), and a static definition can't follow\n * \
a non-static declaration. */\n\
int _oz_get_log_precision(void) {{ return -1; }}\n\n{}",
        assembled
    )
}

/// OZString, the immutable-string Foundation class -- assembled from
/// `include/oz_sdk/Foundation/OZString.h` / `src/OZString.m` verbatim
/// (only the two generic adaptations). Requires `OZObject`
/// (`common::ozobject_src`) in scope as the root class.
pub fn ozstring_src() -> String {
    assemble(
        include_str!("../../../../include/oz_sdk/Foundation/OZString.h"),
        include_str!("../../../../src/OZString.m"),
    )
}

/// OZDefer, the deferred-cleanup Foundation class -- assembled from
/// `include/oz_sdk/Foundation/OZDefer.h` / `src/OZDefer.m` verbatim (the
/// two generic adaptations), plus lowering the real header's ivars
/// (`__unsafe_unretained id _owner; void (^_block)(id);`) to their
/// already-valid plain-C form (`id _owner; void (*_block)(id);`) --
/// ivar declarations are copied verbatim by oz_static (see
/// `emit::render_interface`), with no `^`-to-`*` block-declarator
/// rewrite applied the way a local variable's declarator gets (see
/// `emit::render_expr`'s `block_pointer_declarator` arm), so the ivar
/// must already be spelled in valid plain C. Requires `OZObject`
/// (`common::ozobject_src`) in scope as the root class; oz_static has no
/// ARC (tracked separately as #189), so releasing an object holding an
/// OZDefer ivar must call `[_cleanup release]` explicitly in that
/// object's own `-dealloc` -- there's no automatic ivar release to rely
/// on.
pub fn ozdefer_src() -> String {
    assemble(
        include_str!("../../../../include/oz_sdk/Foundation/OZDefer.h"),
        include_str!("../../../../src/OZDefer.m"),
    )
    .replace("__unsafe_unretained id _owner;", "id _owner;")
    .replace("void (^_block)(id);", "void (*_block)(id);")
}

/// OZArray, the immutable-array Foundation class -- assembled from
/// `include/oz_sdk/Foundation/OZArray.h` / `src/OZArray.m`. `-iter`/
/// `-next` (and `<IteratorProtocol>` conformance, and the `_iterIdx`
/// ivar they need) are kept now -- they used to be cut here too, until
/// for-in support needed them for real (see `emit::render_forin_statement`)
/// and the dynamic-dispatch generalization (ported from the Python
/// pipeline's `_classify_dispatch` while adding OZDictionary) made
/// `OZ_PROTOCOL_SEND_iter`/`OZ_PROTOCOL_SEND_next` possible. The
/// `iterIdx` *property* (`@property (readonly) uint16_t iterIdx;` +
/// `@synthesize iterIdx = _iterIdx;` -- distinct from the `_iterIdx`
/// ivar itself, which `-iter`/`-next` use directly) is kept too, now
/// that OZ-095 added `@property`/`@synthesize` support. Still cut, same
/// reasons as before: the real header's generic type param
/// (`<__covariant ObjectType>` -- untested territory, not worth the
/// risk when dropping it changes nothing observable) and
/// `enumerateObjectsUsingBlock:`/`countByEnumeratingWithState:` (only
/// back Foundation's own `NSFastEnumeration`-style for-in, which this
/// port doesn't use -- the Python oracle's own for-in desugar is
/// `-iter`/`-next`-based too, see `_emit_forin_stmt`), and
/// `cDescription:maxLength:` (still recurses `[elem cDescription:...]`
/// on a bare `id` -- now dynamically dispatchable in principle, same
/// fix as OZDictionary's, just not restored here since nothing in this
/// port's scope needs it).
///
/// `+arrayWithObjects:count:` isn't declared/implemented in the real
/// `OZArray.m` either -- the real pipeline synthesizes it at emit-time as
/// `{Name}_initWithItems` (a template-generated, item-pool-backed
/// `static inline`, see `tools/oz_transpile/templates/class_header.h.j2`).
/// oz_static's equivalent, `OZArray_oz_initWithItems`, is generated by
/// `companion::render_array_support` (malloc-based instead of
/// pool-based) and never written to ObjC source at all -- it backs the
/// `@[...]` boxed array literal desugar in `emit.rs`, the same way
/// `OZQ31`'s class methods back `@42`.
///
/// Requires `OZObject` (`common::ozobject_src`) in scope as the root
/// class; a boxed array literal's elements typically also need `OZQ31`
/// (`common::ozq31_src`) in scope, since `@(42)` desugars to it. For-in
/// over an `OZArray` also needs `IteratorProtocol`
/// (`common::iterator_protocol_src`) declared somewhere in scope.
pub fn ozarray_src() -> String {
    let mut header = include_str!("../../../../include/oz_sdk/Foundation/OZArray.h").to_string();
    header = header.replace("__unsafe_unretained id *_items;", "id *_items;");
    header = header.replace(
        "@interface OZArray<__covariant ObjectType> : OZObject <IteratorProtocol> {",
        "@interface OZArray : OZObject <IteratorProtocol> {",
    );
    header = remove_line_containing(&header, "arrayWithObjects:");
    header = remove_line_containing(&header, "struct NSFastEnumerationState;");
    header = remove_line_containing(&header, "enumerateObjectsUsingBlock:");
    header = remove_line_range(&header, "countByEnumeratingWithState:", "count:(unsigned long)len;");
    header = remove_line_containing(&header, "cDescription:(char *)buf maxLength:(int)maxLen;");

    let mut implementation = include_str!("../../../../src/OZArray.m").to_string();
    implementation = remove_method_body(&implementation, "- (void)enumerateObjectsUsingBlock:");
    implementation = remove_method_body(&implementation, "cDescription:(char *)buf maxLength:(int)maxLen");

    assemble(&header, &implementation)
}

/// OZMutableString, the growable-string Foundation class -- assembled
/// from `include/oz_sdk/Foundation/OZMutableString.h` / `src/OZMutableString.m`
/// verbatim (only the two generic adaptations), including its own
/// `malloc`/`free` for the growable `_data` buffer (real string-growth
/// logic already present in the source, unrelated to the object's own
/// alloc/free machinery oz_static synthesizes -- see #199 for that
/// separate, Zephyr-only concern). Subclasses `OZString`
/// (`common::ozstring_src`), inheriting `_data`/`_length`/`_hash` and
/// overriding `-dealloc` to free `_data` (correct without an explicit
/// `[super dealloc]`: `OZString` has none of its own to chain to -- only
/// boxed literals ever produce an `OZString` instance in this port, and
/// those are static, not heap-allocated). Requires `OZObject`
/// (`common::ozobject_src`) and `OZString` (`common::ozstring_src`) in
/// scope.
pub fn ozmutablestring_src() -> String {
    assemble(
        include_str!("../../../../include/oz_sdk/Foundation/OZMutableString.h"),
        include_str!("../../../../src/OZMutableString.m"),
    )
}

/// OZDictionary, the immutable-dictionary Foundation class -- assembled
/// from `include/oz_sdk/Foundation/OZDictionary.h` / `src/OZDictionary.m`.
/// `-iter`/`-next`/`<IteratorProtocol>`/`_iterIdx` are kept (see
/// `ozarray_src`'s doc comment -- same reasoning, now that for-in
/// support exists), and so is the `iterIdx` *property* (distinct from
/// the ivar), now that OZ-095 added `@property`/`@synthesize` support.
/// Still cut, same reasons as `ozarray_src`: the generic type params
/// (`<__covariant KeyType, __covariant ObjectType>`) and
/// `countByEnumeratingWithState:` (for-in here is `-iter`/`-next`-based,
/// matching the Python oracle's own desugar). `cDescription:maxLength:`
/// is kept, unlike `ozarray_src`: its body message-sends
/// `cDescription:maxLength:` back onto bare `id`-typed locals (each
/// key/value), which is exactly the dynamic-dispatch case
/// `model.rs`/`companion.rs`/`emit.rs` were generalized for while
/// building this class in the first place (`-objectForKey:`'s
/// `[k isEqual:key]` is what forced that fix).
///
/// `+dictionaryWithObjects:forKeys:count:` isn't declared/implemented
/// in the real `OZDictionary.m` either -- same reason as OZArray's
/// `+arrayWithObjects:count:` (synthesized at emit-time instead, see
/// `companion::render_dict_support`'s `OZDictionary_oz_initWithKeysValues`).
///
/// Requires `OZObject` (`common::ozobject_src`) in scope as the root
/// class; a boxed dictionary literal's keys/values typically also need
/// `OZString` (`common::ozstring_src`) and `OZQ31` (`common::ozq31_src`)
/// in scope, since `@"..."` and `@(42)` desugar to them. For-in over an
/// `OZDictionary` also needs `IteratorProtocol`
/// (`common::iterator_protocol_src`) declared somewhere in scope.
pub fn ozdictionary_src() -> String {
    let mut header = include_str!("../../../../include/oz_sdk/Foundation/OZDictionary.h").to_string();
    header = header.replace("__unsafe_unretained id *_keys;", "id *_keys;");
    header = header.replace("__unsafe_unretained id *_values;", "id *_values;");
    header = header.replace(
        "@interface OZDictionary<__covariant KeyType, __covariant ObjectType> : OZObject <IteratorProtocol> {",
        "@interface OZDictionary : OZObject <IteratorProtocol> {",
    );
    header = remove_line_range(&header, "dictionaryWithObjects:", "count:(unsigned int)count;");
    header = remove_line_containing(&header, "struct NSFastEnumerationState;");
    header = remove_line_range(&header, "countByEnumeratingWithState:", "count:(unsigned long)len;");

    let implementation = include_str!("../../../../src/OZDictionary.m").to_string();

    assemble(&header, &implementation)
}

/// `IteratorProtocol`, the for-in protocol -- assembled from the real
/// `include/oz_sdk/Foundation/Iterator+Protocol.h` verbatim, no cuts at
/// all needed: its `@property (nonatomic, readonly) uint16_t iterIdx;`
/// (a *protocol* requirement, not a class one) never reaches the static
/// bar or a collision with the class-level `@property` ban -- protocol
/// parsing (`collect::extract_protocol`) only ever extracts
/// `method_declaration` children, silently skipping everything else,
/// and the whole `@protocol ... @end` block is elided to a comment in
/// the generated output regardless (see `emit.rs`'s top-level
/// `protocol_declaration` handling) -- so its real text never has to
/// compile as anything.
///
/// Declaring this protocol (with `-iter`/`-next` inside) is what makes
/// `Program::is_dynamically_dispatched("iter"/"next", false)` true --
/// conformance to it isn't even checked for that (see
/// `Program::all_protocol_methods`'s doc comment: dispatch generation
/// only cares "who implements this selector," not who formally
/// conforms) -- so a class doesn't strictly need `<IteratorProtocol>`
/// in its own `@interface` line for for-in to work against it, though
/// `ozarray_src`/`ozdictionary_src` still declare it for the free
/// conformance validation (#192) and because the real headers do too.
pub fn iterator_protocol_src() -> String {
    strip_import_and_pragma_lines(include_str!("../../../../include/oz_sdk/Foundation/Iterator+Protocol.h"))
}
