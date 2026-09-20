// SPDX-License-Identifier: Apache-2.0
//
// split_output.rs - OZ-096: tests for emit::emit_split / lib::transpile_split,
// which produce one .h/.c pair per origin file (see imports::ResolvedSource)
// instead of one inlined blob. Verifies real compilation/linking across the
// resulting *multiple* translation units -- not just that transpile_split()
// returns Ok, the same way tests/common/mod.rs's compile_and_run proves a
// single-file transpile() actually produces working C.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use oz2c::imports::resolve_imports;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oz2c_split_test_{}", name));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn cc(args: &[&str]) {
    let output = Command::new("cc").args(args).output().unwrap_or_else(|e| panic!("failed to run cc: {}", e));
    assert!(output.status.success(), "cc {:?} failed:\n{}", args, String::from_utf8_lossy(&output.stderr));
}

/// Real PAL include dir, mirroring `tests/common/mod.rs::include_dir`.
fn pal_include_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../include")
}

/// Write every `(stem, h, c)` file to `outdir`, plus the shared companion
/// pair, compile each `.c` separately (proving they're real, independent
/// translation units -- not one blob relying on define-before-use), link
/// them all together, run the result, and return captured stdout.
fn compile_link_run(
    outdir: &Path,
    files: &[(String, String, String)],
    companion_h: &str,
    companion_c: &str,
) -> String {
    fs::create_dir_all(outdir).unwrap();
    // Write every header/source first -- a per-origin `.c` `#include`s
    // both its own `.h` and the shared companion header, so all of them
    // must exist on disk before any `cc -c` invocation runs.
    for (stem, h, c) in files {
        fs::write(outdir.join(format!("{}.h", stem)), h).unwrap();
        fs::write(outdir.join(format!("{}.c", stem)), c).unwrap();
    }
    fs::write(outdir.join("oz2c_dispatch.h"), companion_h).unwrap();
    let dispatch_c = outdir.join("oz2c_dispatch.c");
    fs::write(&dispatch_c, companion_c).unwrap();

    let mut object_files = Vec::new();
    for (stem, _, _) in files {
        let c_path = outdir.join(format!("{}.c", stem));
        let o_path = outdir.join(format!("{}.o", stem));
        cc(&[
            "-DOZ_PLATFORM_HOST",
            "-I",
            pal_include_dir().to_str().unwrap(),
            "-I",
            outdir.to_str().unwrap(),
            "-c",
            c_path.to_str().unwrap(),
            "-o",
            o_path.to_str().unwrap(),
        ]);
        object_files.push(o_path);
    }
    let dispatch_o = outdir.join("oz2c_dispatch.o");
    cc(&[
        "-DOZ_PLATFORM_HOST",
        "-I",
        pal_include_dir().to_str().unwrap(),
        "-I",
        outdir.to_str().unwrap(),
        "-c",
        dispatch_c.to_str().unwrap(),
        "-o",
        dispatch_o.to_str().unwrap(),
    ]);
    object_files.push(dispatch_o);

    let bin = outdir.join("bin");
    let mut args: Vec<&str> = object_files.iter().map(|p| p.to_str().unwrap()).collect();
    args.push("-o");
    args.push(bin.to_str().unwrap());
    cc(&args);

    let run = Command::new(&bin).output().unwrap_or_else(|e| panic!("failed to run binary: {}", e));
    assert!(
        run.status.success(),
        "binary exited non-zero: {:?}\nstdout: {}\nstderr: {}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).unwrap()
}

/// The concrete case OZ-096 exists for: a class two levels removed from
/// the root, spread across three real files (Base.h/.m, Derived.h/.m,
/// main.m), each compiled as an independent translation unit. Exercises
/// the cross-file superclass dependency (`Derived.h` must `#include
/// "Base.h"` -- `struct Base base;` is a nested field, needing Base's
/// *full* struct visible, not just a forward declare) and that
/// `{name}_oz_alloc`/`_oz_free`'s prototypes (from the shared companion
/// header) are enough for a caller in yet another file to use them.
#[test]
fn cross_file_multi_level_inheritance_compiles_links_and_runs() {
    let dir = scratch_dir("cross_file_inheritance");
    fs::create_dir_all(dir.join("include")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("include/Base.h"),
        "#pragma once\n#import <Foundation/OZObject.h>\n\n\
         @interface Base : OZObject {\n\tint _value;\n}\n\
         - (int)value;\n- (void)setValue:(int)v;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Base.m"),
        "#import \"Base.h\"\n\n@implementation Base\n\
         - (int)value {\n\treturn _value;\n}\n\
         - (void)setValue:(int)v {\n\t_value = v;\n}\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("include/Derived.h"),
        "#pragma once\n#import \"Base.h\"\n\n\
         @interface Derived : Base {\n\tint _extra;\n}\n- (int)total;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Derived.m"),
        "#import \"Derived.h\"\n\n@implementation Derived\n\
         - (int)total {\n\treturn [self value] + _extra;\n}\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.m"),
        "#import \"Derived.h\"\n\n#include <stdio.h>\n\
         int main(void) {\n\tDerived *d = [Derived alloc];\n\t[d setValue:10];\n\
         \tprintf(\"total=%d\\n\", [d total]);\n\treturn 0;\n}\n",
    )
    .unwrap();

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let include_dirs = vec![repo_root.join("include/oz_sdk"), dir.join("include")];
    let impl_dirs = vec![repo_root.join("src"), dir.join("src")];
    let source = fs::read_to_string(dir.join("main.m")).unwrap();

    let resolved = resolve_imports(&source, &dir, &include_dirs, &impl_dirs, "main")
        .unwrap_or_else(|e| panic!("resolve failed: {}", e));

    let out = oz2c::transpile_split(&resolved.text, &resolved.origins).unwrap_or_else(|diags| {
        panic!("transpile_split failed:\n{}", diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
    });

    // One .h/.c pair per real origin file: OZObject, Base, Derived, main.
    let stems: Vec<&str> = out.files.iter().map(|(s, _, _)| s.as_str()).collect();
    for expected in ["OZObject", "Base", "Derived", "main"] {
        assert!(stems.contains(&expected), "stems: {:?}", stems);
    }

    let derived_h = &out.files.iter().find(|(s, _, _)| s == "Derived").unwrap().1;
    assert!(derived_h.contains("#include \"Base.h\""), "Derived.h: {}", derived_h);
    assert!(!derived_h.contains("Base_value"), "Derived.h shouldn't inline Base's methods: {}", derived_h);

    let stdout = compile_link_run(&scratch_dir("cross_file_inheritance_out"), &out.files, &out.companion_h, &out.companion_c);
    assert_eq!(stdout, "total=10\n");
}

/// `OZArray`'s extra boxed-literal builder (`OZArray_oz_initWithItems`)
/// has no prototype anywhere in the shared companion header -- only a
/// full definition, generated in-place next to `OZArray`'s own struct
/// (see `emit::render_interface`'s `extra_proto`). A caller in a
/// *different* file (main.c, via a `@[...]` literal) needs that
/// prototype declared in `OZArray.h`, not just defined in `OZArray.c` --
/// this only surfaces once alloc/free-style helpers live in a separate
/// translation unit from their caller, which is exactly what OZ-096
/// introduces.
#[test]
fn boxed_array_literal_helper_prototype_is_visible_across_files() {
    // Minimal stand-ins for the real OZNumber/OZArray -- just enough to
    // trigger `emit::render_interface`'s `name == "OZNumber"`/`"OZArray"`
    // special cases (the boxed-literal desugar and its helper are
    // hardcoded to those exact class names), without real OZArray.m's
    // `countByEnumeratingWithState:`/`enumerateObjectsUsingBlock:` (a
    // separate, pre-existing, unrelated gap -- `NSFastEnumerationState`
    // is never made visible to the shared companion header regardless
    // of single- or multi-file output).
    let dir = scratch_dir("boxed_array_literal");
    fs::write(
        dir.join("OZNumber.h"),
        "#pragma once\n#import <Foundation/OZObject.h>\n\n\
         @interface OZNumber : OZObject {\n\tint32_t _raw;\n}\n+ (id)numberWithInt32:(int32_t)v;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("OZNumber.m"),
        "#import \"OZNumber.h\"\n\n@implementation OZNumber\n\
         + (id)numberWithInt32:(int32_t)v {\n\tOZNumber *q = [OZNumber alloc];\n\tq->_raw = v;\n\treturn q;\n}\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("OZArray.h"),
        "#pragma once\n#import <Foundation/OZObject.h>\n\n\
         @interface OZArray : OZObject {\n\tid *_items;\n\tsize_t _count;\n}\n- (size_t)count;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("OZArray.m"),
        "#import \"OZArray.h\"\n\n@implementation OZArray\n- (size_t)count {\n\treturn _count;\n}\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.m"),
        "#import <Foundation/OZObject.h>\n#import \"OZNumber.h\"\n#import \"OZArray.h\"\n\n\
         #include <stdio.h>\nint main(void) {\n\tOZArray *arr = @[@(1), @(2), @(3)];\n\tprintf(\"count=%zu\\n\", [arr count]);\n\treturn 0;\n}\n",
    )
    .unwrap();

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let include_dirs = vec![repo_root.join("include/oz_sdk")];
    // `dir` first: these scratch OZNumber/OZArray stand-ins must win over
    // the real `src/OZNumber.m`/`src/OZArray.m` sibling impls, which would
    // otherwise be found first (same stem) and, via their own `#import
    // <Foundation/...>`, pull in the *real* header too under a
    // different canonical path -- merging both into one conflicting
    // "OZNumber" class instead of using only the scratch stand-in.
    let impl_dirs = vec![dir.clone(), repo_root.join("src")];
    let source = fs::read_to_string(dir.join("main.m")).unwrap();

    let resolved = resolve_imports(&source, &dir, &include_dirs, &impl_dirs, "main")
        .unwrap_or_else(|e| panic!("resolve failed: {}", e));

    let out = oz2c::transpile_split(&resolved.text, &resolved.origins).unwrap_or_else(|diags| {
        panic!("transpile_split failed:\n{}", diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n"))
    });

    let array_h = &out.files.iter().find(|(s, _, _)| s == "OZArray").unwrap().1;
    assert!(array_h.contains("OZArray_oz_initWithItems"), "OZArray.h: {}", array_h);

    let stdout = compile_link_run(&scratch_dir("boxed_array_literal_out"), &out.files, &out.companion_h, &out.companion_c);
    assert_eq!(stdout, "count=3\n");
}

/// Everything a header holds besides its `@interface` has to survive into
/// the generated output, and has to be reachable from a *different* origin
/// file than the one it was written in.
///
/// This is the shape of `tests/behavior/cases/regression/
/// issue_090_header_preservation.m`, the Python pipeline's own regression
/// test for the same bug ("transpiler drops struct/union/enum/macro
/// definitions from companion headers when they are not referenced by ObjC
/// interface members"). oz2c dropped three of the five kinds:
///
/// - a `struct`/`union` definition with a body matched no arm in
///   `emit_split`, which builds each file only from what its arms push, so
///   the definition vanished and left just its trailing `;` -- every use of
///   it was then "variable has incomplete type". `emit()` never showed this,
///   because that path patched the original text and anything unpatched
///   survived, so a single-file test could not catch it. Since #254 the two
///   share one walk and so share this arm; the case stays here because the
///   *placement* it asserts -- reachable from another translation unit -- is
///   still something only a split build can demonstrate.
/// - a `static inline` helper went to the body, so no other file could call
///   it.
///
/// Enums and macros already worked; they are asserted here too, so a future
/// change cannot quietly lose them either.
#[test]
fn non_objc_header_content_survives_into_other_translation_units() {
    let dir = scratch_dir("header_content_preservation");
    fs::create_dir_all(dir.join("include")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("include/Sensor.h"),
        "#pragma once\n#import <Foundation/OZObject.h>\n\n\
         enum sensor_state {\n\tSENSOR_IDLE = 0,\n\tSENSOR_SAMPLING,\n\tSENSOR_ERROR,\n};\n\n\
         union sensor_data {\n\tint raw;\n\tfloat calibrated;\n};\n\n\
         struct sensor_msg {\n\tenum sensor_state state;\n\tunion sensor_data data;\n};\n\n\
         #define SENSOR_MAX_CHANNELS 8\n\
         #define SENSOR_DOUBLE(v) ((v) * 2)\n\n\
         static inline int sensor_scale(int raw, int factor)\n{\n\treturn raw * factor;\n}\n\n\
         @interface Sensor : OZObject {\n\tint _reading;\n}\n\
         - (int)reading;\n- (void)setReading:(int)v;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Sensor.m"),
        "#import \"Sensor.h\"\n\n@implementation Sensor\n\
         - (int)reading {\n\treturn _reading;\n}\n\
         - (void)setReading:(int)v {\n\t_reading = v;\n}\n@end\n",
    )
    .unwrap();
    // Every one of the five kinds is used here, in an origin that is not
    // the header they were written in -- a struct with a union field by
    // value (needing both complete, in the right order), the enum, both
    // macros, and the `static inline`.
    fs::write(
        dir.join("main.m"),
        "#import \"Sensor.h\"\n\n#include <stdio.h>\n\
         int main(void) {\n\
         \tstruct sensor_msg msg;\n\
         \tmsg.state = SENSOR_ERROR;\n\
         \tmsg.data.raw = sensor_scale(SENSOR_DOUBLE(3), SENSOR_MAX_CHANNELS);\n\
         \tSensor *s = [Sensor alloc];\n\t[s setReading:msg.data.raw];\n\
         \tprintf(\"state=%d reading=%d\\n\", (int)msg.state, [s reading]);\n\treturn 0;\n}\n",
    )
    .unwrap();

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let include_dirs = vec![repo_root.join("include/oz_sdk"), dir.join("include")];
    let impl_dirs = vec![repo_root.join("src"), dir.join("src")];
    let source = fs::read_to_string(dir.join("main.m")).unwrap();

    let resolved = resolve_imports(&source, &dir, &include_dirs, &impl_dirs, "main")
        .unwrap_or_else(|e| panic!("resolve failed: {}", e));
    let out = oz2c::transpile_split(&resolved.text, &resolved.origins).unwrap_or_else(|diags| {
        panic!(
            "transpile_split failed:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });

    // The types go to the companion header: it is the one header every
    // generated file includes, and its own prototypes can name them.
    for expected in ["union sensor_data", "struct sensor_msg", "enum sensor_state"] {
        assert!(
            out.companion_h.contains(expected),
            "companion header is missing `{}`:\n{}",
            expected,
            out.companion_h
        );
    }
    // The union must come before the struct that has one by value, and both
    // after the enum -- source order, which the source itself had to get
    // right for C.
    let enum_at = out.companion_h.find("enum sensor_state {").unwrap();
    let union_at = out.companion_h.find("union sensor_data {").unwrap();
    let struct_at = out.companion_h.find("struct sensor_msg {").unwrap();
    assert!(enum_at < union_at && union_at < struct_at, "hoisted types are out of source order");

    // The `static inline` goes to its own origin's header, where another
    // file including that header can call it.
    let sensor_h = &out.files.iter().find(|(s, _, _)| s == "Sensor").unwrap().1;
    assert!(
        sensor_h.contains("int sensor_scale(int raw, int factor)"),
        "Sensor.h is missing the static inline helper:\n{}",
        sensor_h
    );

    // Compiling each origin as its own translation unit is the real check:
    // "reachable from another file" is not something inspecting one string
    // can establish.
    let stdout = compile_link_run(
        &scratch_dir("header_content_preservation_out"),
        &out.files,
        &out.companion_h,
        &out.companion_c,
    );
    assert_eq!(stdout, "state=2 reading=48\n");
}

/// A bare top-level macro *invocation* in a header has to reach every origin
/// that includes it, not just the one file it was written in.
///
/// This is the shape Zephyr is full of -- `ZBUS_CHAN_DECLARE`,
/// `LOG_MODULE_DECLARE`, `DEVICE_DT_DECLARE` -- and it is neither a
/// `preproc` node (so the passthrough arm's macro rule missed it) nor a
/// declaration, so it fell to the generated `.c`. `samples/zbus_service`
/// could not be built for ARM at all because of it: its header declares the
/// channels with `ZBUS_CHAN_DECLARE(...)` and `main` then failed with
/// "'chan_temperature_service_report' undeclared".
///
/// Routing is by *provenance* now: whatever a header contributed goes into
/// the generated header, which is what a header is for. Checked by compiling
/// each origin as its own translation unit, since "visible from another
/// file" is not something inspecting one string can establish.
#[test]
fn header_macro_invocation_reaches_other_origins() {
    let dir = scratch_dir("header_macro_invocation");
    fs::create_dir_all(dir.join("include")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    // A stand-in for the Zephyr macro pair: one declares, one defines. Only
    // the declaration is in the header, as in the real thing.
    fs::write(
        dir.join("include/Chan.h"),
        "#pragma once\n#import <Foundation/OZObject.h>\n\n\
         #define FAKE_CHAN_DECLARE(name) extern int name\n\
         #define FAKE_CHAN_DEFINE(name)  int name = 7\n\n\
         FAKE_CHAN_DECLARE(g_fake_chan);\n\n\
         @interface Chan : OZObject\n- (int)value;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Chan.m"),
        "#import \"Chan.h\"\n\nFAKE_CHAN_DEFINE(g_fake_chan);\n\n\
         @implementation Chan\n- (int)value {\n\treturn g_fake_chan;\n}\n@end\n",
    )
    .unwrap();
    // main.m reaches the channel only through the header's declaration.
    fs::write(
        dir.join("main.m"),
        "#import \"Chan.h\"\n\n#include <stdio.h>\n\
         int main(void) {\n\tChan *c = [Chan alloc];\n\
         \tprintf(\"direct=%d method=%d\\n\", g_fake_chan, [c value]);\n\treturn 0;\n}\n",
    )
    .unwrap();

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let include_dirs = vec![repo_root.join("include/oz_sdk"), dir.join("include")];
    let impl_dirs = vec![repo_root.join("src"), dir.join("src")];
    let source = fs::read_to_string(dir.join("main.m")).unwrap();

    let resolved = resolve_imports(&source, &dir, &include_dirs, &impl_dirs, "main")
        .unwrap_or_else(|e| panic!("resolve failed: {}", e));
    let out = oz2c::transpile_split_with_options(
        &resolved.text,
        &resolved.origins,
        &oz2c::Options {
            header_ranges: resolved.header_ranges.clone(),
            ..Default::default()
        },
    )
    .unwrap_or_else(|diags| {
        panic!(
            "transpile_split failed:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });

    let chan = out.files.iter().find(|(s, _, _)| s == "Chan").unwrap();
    assert!(
        chan.1.contains("FAKE_CHAN_DECLARE(g_fake_chan)"),
        "the header's macro invocation should be in Chan.h:\n{}",
        chan.1
    );
    assert!(
        chan.2.contains("FAKE_CHAN_DEFINE(g_fake_chan)"),
        "the .m's macro invocation should stay in Chan.c:\n{}",
        chan.2
    );

    let stdout = compile_link_run(
        &scratch_dir("header_macro_invocation_out"),
        &out.files,
        &out.companion_h,
        &out.companion_c,
    );
    assert_eq!(stdout, "direct=7 method=7\n");
}

/// A `typedef` in a header, named by a method signature, reaches the
/// companion header where that signature's prototype lands (#533).
///
/// `oz2c_dispatch.h` declares a prototype for every method of every class
/// and includes no user header. A `struct` or a bare `enum` written in a
/// class's own header is hoisted into it for exactly that reason; a
/// `typedef` was not, so the prototype named a type the file never
/// defined and GCC answered `unknown type name 'PXSensorFlags'` on a
/// generated line, with oz2c exiting 0.
///
/// It was **every** typedef and not only the enum the issue reported.
/// Measured across four shapes before the fix: four errors, one each for
/// `typedef enum`, `typedef struct`, `typedef int` and
/// `typedef unsigned char`. A *bare* `enum E { ... }` was already hoisted
/// and compiled clean, which is what made the report look enum-specific.
#[test]
fn a_typedef_named_by_a_method_signature_reaches_the_companion_header() {
    let dir = scratch_dir("typedef_in_signature");
    fs::create_dir_all(dir.join("include")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("include/Kinds.h"),
        "#pragma once\n#import <Foundation/OZObject.h>\n\n\
         struct bare_struct { int a; };\n\
         enum bare_enum { BareOne = 1 };\n\
         typedef enum { TdEnumOne = 1 } TdEnum;\n\
         typedef struct { int b; } TdStruct;\n\
         typedef int TdInt;\n\
         typedef unsigned char TdByte;\n\n\
         @interface Kinds : OZObject\n\
         - (int)useBareStruct:(struct bare_struct)v;\n\
         - (int)useBareEnum:(enum bare_enum)v;\n\
         - (int)useTdEnum:(TdEnum)v;\n\
         - (int)useTdStruct:(TdStruct)v;\n\
         - (int)useTdInt:(TdInt)v;\n\
         - (int)useTdByte:(TdByte)v;\n\
         @end\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Kinds.m"),
        "#import \"Kinds.h\"\n\n@implementation Kinds\n\
         - (int)useBareStruct:(struct bare_struct)v { return v.a; }\n\
         - (int)useBareEnum:(enum bare_enum)v { return (int)v; }\n\
         - (int)useTdEnum:(TdEnum)v { return (int)v; }\n\
         - (int)useTdStruct:(TdStruct)v { return v.b; }\n\
         - (int)useTdInt:(TdInt)v { return v; }\n\
         - (int)useTdByte:(TdByte)v { return (int)v; }\n\
         @end\n",
    )
    .unwrap();
    /* `main` is a second origin on purpose: the point is that the *shared*
     * header carries the type, not that the declaring origin's own `.h`
     * happens to. */
    fs::write(
        dir.join("main.m"),
        "#import \"Kinds.h\"\n\n#include <stdio.h>\n\
         int main(void) {\n\tKinds *k = [Kinds alloc];\n\
         \tstruct bare_struct bs = { 1 };\n\
         \tTdStruct ts = { 4 };\n\
         \tprintf(\"sum=%d\\n\",\n\
         \t       [k useBareStruct:bs] + [k useBareEnum:BareOne]\n\
         \t       + [k useTdEnum:TdEnumOne] + [k useTdStruct:ts]\n\
         \t       + [k useTdInt:8] + [k useTdByte:16]);\n\
         \treturn 0;\n}\n",
    )
    .unwrap();

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let include_dirs = vec![repo_root.join("include/oz_sdk"), dir.join("include")];
    let impl_dirs = vec![repo_root.join("src"), dir.join("src")];
    let source = fs::read_to_string(dir.join("main.m")).unwrap();
    let resolved = resolve_imports(&source, &dir, &include_dirs, &impl_dirs, "main")
        .unwrap_or_else(|e| panic!("resolve failed: {}", e));
    let out = oz2c::transpile_split_with_options(
        &resolved.text,
        &resolved.origins,
        &oz2c::Options { header_ranges: resolved.header_ranges.clone(), ..Default::default() },
    )
    .unwrap_or_else(|diags| {
        panic!(
            "transpile_split failed:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });

    /* Every one of the six, in the file the prototypes live in. The two
     * bare shapes were already there; the four typedefs are #533. */
    for needle in [
        "struct bare_struct",
        "enum bare_enum",
        "} TdEnum",
        "} TdStruct",
        "typedef int TdInt",
        "typedef unsigned char TdByte",
    ] {
        assert!(
            out.companion_h.contains(needle),
            "`{}` should be hoisted into the companion header:\n{}",
            needle,
            out.companion_h
        );
    }
    /* And no `;;`: a `type_definition` carries its own semicolon where a
     * specifier node does not, so pushing its text verbatim emitted
     * `typedef int TdInt;;` -- an empty declaration at file scope, which
     * is not valid ISO C and which `just test-pedantic` gates on. */
    assert!(!out.companion_h.contains(";;"), "no doubled semicolon:\n{}", out.companion_h);

    /* Compiled and linked across both origins, which is the claim: reading
     * the header would not have caught the ordering case below. */
    let ran = compile_link_run(
        &dir.join("out"),
        &out.files,
        &out.companion_h,
        &out.companion_c,
    );
    assert_eq!(ran.trim(), "sum=31");
}

/// Source order is preserved across every hoisted kind, because a
/// `typedef` can name a struct *or* be named by one (#533).
///
/// This was broken before the fix and independently of it: the hoisted
/// declarations lived in three lists keyed on kind -- forward declares,
/// then enums, then structs and unions -- so a `typedef` the struct
/// depended on could not be placed. `typedef int Celsius;` followed by
/// `struct reading { Celsius temp; };` emitted the struct with `Celsius`
/// nowhere above it: two errors, one of them *inside* the hoisted struct.
///
/// The old ordering rested on a sound argument for two kinds -- "a hoisted
/// struct can have an enum field by value ... nothing runs the other way:
/// an enum cannot contain a struct" -- and a typedef is the counterexample
/// that made a third list unorderable. One list in source order needs no
/// analysis: C required the author to write a working order already.
#[test]
fn hoisted_c_types_keep_source_order_so_a_typedef_can_precede_its_user() {
    let dir = scratch_dir("hoist_source_order");
    fs::create_dir_all(dir.join("include")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("include/Ord.h"),
        "#pragma once\n#import <Foundation/OZObject.h>\n\n\
         typedef int Celsius;\n\n\
         struct reading { Celsius temp; };\n\n\
         typedef struct reading Reading;\n\n\
         @interface Ord : OZObject\n\
         - (struct reading)read;\n\
         - (Celsius)temp;\n\
         - (Reading)again;\n\
         @end\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/Ord.m"),
        "#import \"Ord.h\"\n\n@implementation Ord\n\
         - (struct reading)read { struct reading r = { 21 }; return r; }\n\
         - (Celsius)temp { return 21; }\n\
         - (Reading)again { struct reading r = { 21 }; return r; }\n\
         @end\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.m"),
        "#import \"Ord.h\"\n\n#include <stdio.h>\n\
         int main(void) {\n\tOrd *o = [Ord alloc];\n\
         \tprintf(\"t=%d\\n\", (int)[o temp] + [o read].temp + [o again].temp);\n\
         \treturn 0;\n}\n",
    )
    .unwrap();

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let include_dirs = vec![repo_root.join("include/oz_sdk"), dir.join("include")];
    let impl_dirs = vec![repo_root.join("src"), dir.join("src")];
    let source = fs::read_to_string(dir.join("main.m")).unwrap();
    let resolved = resolve_imports(&source, &dir, &include_dirs, &impl_dirs, "main")
        .unwrap_or_else(|e| panic!("resolve failed: {}", e));
    let out = oz2c::transpile_split_with_options(
        &resolved.text,
        &resolved.origins,
        &oz2c::Options { header_ranges: resolved.header_ranges.clone(), ..Default::default() },
    )
    .unwrap_or_else(|diags| {
        panic!(
            "transpile_split failed:\n{}",
            diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
        )
    });

    /* The order, asserted directly: a dependency must precede its user,
     * and `Reading` must follow the struct it names. Compiling alone would
     * catch this, but the positions say *why* it compiles. */
    let celsius = out.companion_h.find("typedef int Celsius").expect("Celsius hoisted");
    let reading = out.companion_h.find("struct reading {").expect("struct reading hoisted");
    let alias = out.companion_h.find("typedef struct reading Reading").expect("Reading hoisted");
    assert!(celsius < reading, "the typedef must precede the struct using it:\n{}", out.companion_h);
    assert!(reading < alias, "the struct must precede the typedef naming it:\n{}", out.companion_h);

    let ran = compile_link_run(&dir.join("out"), &out.files, &out.companion_h, &out.companion_c);
    assert_eq!(ran.trim(), "t=63");
}
