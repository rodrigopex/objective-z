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

use oz_static::imports::resolve_imports;

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oz_static_split_test_{}", name));
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
    fs::write(outdir.join("oz_static_dispatch.h"), companion_h).unwrap();
    let dispatch_c = outdir.join("oz_static_dispatch.c");
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
    let dispatch_o = outdir.join("oz_static_dispatch.o");
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

    let out = oz_static::transpile_split(&resolved.text, &resolved.origins).unwrap_or_else(|diags| {
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
    // Minimal stand-ins for the real OZQ31/OZArray -- just enough to
    // trigger `emit::render_interface`'s `name == "OZQ31"`/`"OZArray"`
    // special cases (the boxed-literal desugar and its helper are
    // hardcoded to those exact class names), without real OZArray.m's
    // `countByEnumeratingWithState:`/`enumerateObjectsUsingBlock:` (a
    // separate, pre-existing, unrelated gap -- `NSFastEnumerationState`
    // is never made visible to the shared companion header regardless
    // of single- or multi-file output).
    let dir = scratch_dir("boxed_array_literal");
    fs::write(
        dir.join("OZQ31.h"),
        "#pragma once\n#import <Foundation/OZObject.h>\n\n\
         @interface OZQ31 : OZObject {\n\tint32_t _raw;\n}\n+ (id)fixedWithInt32:(int32_t)v;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("OZQ31.m"),
        "#import \"OZQ31.h\"\n\n@implementation OZQ31\n\
         + (id)fixedWithInt32:(int32_t)v {\n\tOZQ31 *q = [OZQ31 alloc];\n\tq->_raw = v;\n\treturn q;\n}\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("OZArray.h"),
        "#pragma once\n#import <Foundation/OZObject.h>\n\n\
         @interface OZArray : OZObject {\n\tid *_items;\n\tunsigned int _count;\n}\n- (unsigned int)count;\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("OZArray.m"),
        "#import \"OZArray.h\"\n\n@implementation OZArray\n- (unsigned int)count {\n\treturn _count;\n}\n@end\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.m"),
        "#import <Foundation/OZObject.h>\n#import \"OZQ31.h\"\n#import \"OZArray.h\"\n\n\
         #include <stdio.h>\nint main(void) {\n\tOZArray *arr = @[@(1), @(2), @(3)];\n\tprintf(\"count=%u\\n\", [arr count]);\n\treturn 0;\n}\n",
    )
    .unwrap();

    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let include_dirs = vec![repo_root.join("include/oz_sdk")];
    // `dir` first: these scratch OZQ31/OZArray stand-ins must win over
    // the real `src/OZQ31.m`/`src/OZArray.m` sibling impls, which would
    // otherwise be found first (same stem) and, via their own `#import
    // <Foundation/...>`, pull in the *real* header too under a
    // different canonical path -- merging both into one conflicting
    // "OZQ31" class instead of using only the scratch stand-in.
    let impl_dirs = vec![dir.clone(), repo_root.join("src")];
    let source = fs::read_to_string(dir.join("main.m")).unwrap();

    let resolved = resolve_imports(&source, &dir, &include_dirs, &impl_dirs, "main")
        .unwrap_or_else(|e| panic!("resolve failed: {}", e));

    let out = oz_static::transpile_split(&resolved.text, &resolved.origins).unwrap_or_else(|diags| {
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
/// interface members"). oz_static dropped three of the five kinds:
///
/// - a `struct`/`union` definition with a body matched no arm in
///   `emit_split`, which builds each file only from what its arms push, so
///   the definition vanished and left just its trailing `;` -- every use of
///   it was then "variable has incomplete type". `emit()` never showed this,
///   because that path patches the original text and anything unpatched
///   survives, which is why a single-file test cannot catch it.
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
    let out = oz_static::transpile_split(&resolved.text, &resolved.origins).unwrap_or_else(|diags| {
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
    let out = oz_static::transpile_split_with_options(
        &resolved.text,
        &resolved.origins,
        &oz_static::Options {
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
