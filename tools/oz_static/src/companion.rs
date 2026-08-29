// SPDX-License-Identifier: Apache-2.0
//
// companion.rs - the one small generated file for multi-implementor
// dispatch (dealloc's const-vtable, computed entirely at compile time and
// never mutated at runtime) and pool/init registration, mirroring the
// existing oz_dispatch.h/.c pattern used by the Python pipeline.
//
// Only the root class's full struct lives here, because oz_static_retain/
// oz_static_release/the dealloc switch are generic (shared by every
// class) and need its tracking fields (oz_class_id, oz_refcount,
// oz_deallocating) directly -- mirroring how the Python pipeline already
// treats OZObject as the Foundation root's own generated pair. Every
// other class's full struct (and alloc/free, which need it for sizeof)
// lives in-place at its own @interface/@implementation; this file only
// forward-declares them, which is all its dealloc-dispatch switch needs
// (it only casts pointers and calls functions through them, never
// dereferences).
//
// Everything here is grouped and labeled per originating class, so a
// reader sees "here's what each class contributed to shared infra," not
// an undifferentiated dump.

use crate::model::Program;

/// Walks `start`'s own superclass chain looking for whichever class
/// actually implements `selector` -- the same single-inheritance method
/// lookup real Objective-C dispatch does, just resolved here at
/// generation time instead of at runtime. `None` only if nothing in
/// `start`'s chain implements it at all (dead code at every call site,
/// which only calls this for a class already known to conform/inherit).
fn find_defining_method(program: &Program, start: &str, selector: &str, is_class_method: bool) -> Option<String> {
    let mut cur = Some(start.to_string());
    while let Some(name) = cur {
        let info = program.classes.get(&name)?;
        if info.methods.iter().any(|m| m.is_class_method == is_class_method && m.selector == selector) {
            return Some(name);
        }
        cur = info.superclass.clone();
    }
    None
}

fn find_defining_dealloc(program: &Program, start: &str) -> Option<String> {
    find_defining_method(program, start, "dealloc", false)
}

/// Order classes so a superclass's struct always precedes its subclasses'
/// (C requires a struct's members to be complete types when embedded by
/// value, e.g. `struct Base base;`), regardless of the order they appeared
/// in the source file.
fn topological_order(program: &Program) -> Vec<String> {
    let mut order = Vec::new();
    let mut visited = std::collections::HashSet::new();
    fn visit(
        program: &Program,
        name: &str,
        visited: &mut std::collections::HashSet<String>,
        order: &mut Vec<String>,
    ) {
        if !visited.insert(name.to_string()) {
            return;
        }
        if let Some(sup) = program.classes.get(name).and_then(|c| c.superclass.clone()) {
            visit(program, &sup, visited, order);
        }
        order.push(name.to_string());
    }
    for name in &program.class_order {
        visit(program, name, &mut visited, &mut order);
    }
    order
}

/// `{name}_oz_alloc`/`{name}_oz_free`: malloc-based (host-testable; not
/// slab-integrated -- this crate isn't wired into the embedded build
/// yet). `root` is whichever class has no superclass; alloc always needs
/// it to set the tracking fields regardless of which class is being
/// allocated.
pub(crate) fn render_alloc_free(name: &str, root: &str) -> String {
    let mut c = String::new();
    c.push_str(&format!(
        "/* synthesized: allocates and zero-initializes a new {name} (not from source) */\n",
        name = name
    ));
    c.push_str(&format!("struct {name} *{name}_oz_alloc(void)\n{{\n", name = name));
    c.push_str(&format!(
        "\tstruct {name} *obj = malloc(sizeof(struct {name}));\n\
         \tif (!obj) {{\n\t\treturn (struct {name} *)0;\n\t}}\n\
         \tmemset(obj, 0, sizeof(struct {name}));\n",
        name = name
    ));
    c.push_str(&format!(
        "\t((struct {root} *)obj)->oz_class_id = OZ_STATIC_CLASS_{name};\n\
         \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n",
        root = root,
        name = name
    ));
    c.push_str("\treturn obj;\n}\n\n");
    c.push_str(&format!(
        "/* synthesized: releases {name}'s storage -- called only from\n * \
oz_static_release, once the refcount reaches zero (not from source) */\n",
        name = name
    ));
    c.push_str(&format!(
        "void {name}_oz_free(struct {name} *obj)\n{{\n\tfree(obj);\n}}\n\n",
        name = name
    ));
    c
}

/// OZArray-specific replacement for `render_alloc_free`: alloc is
/// identical, but free must also release every held item and free the
/// items buffer -- OZArray.m (transplanted verbatim from `src/OZArray.m`)
/// has no `-dealloc` of its own (the real Python pipeline synthesizes this
/// at emit-time too, via a static item-pool allocator this malloc-based
/// spike doesn't have), so the generic dealloc dispatch would otherwise
/// fall through to OZObject's no-op `-dealloc` and leak both. Also emits
/// `OZArray_oz_initWithItems`, the malloc-based builder backing the
/// `@[...]` boxed array literal desugar in `emit.rs`.
pub(crate) fn render_array_support(name: &str, root: &str) -> String {
    let mut c = String::new();
    c.push_str(&format!(
        "/* synthesized: allocates and zero-initializes a new {name} (not from source) */\n",
        name = name
    ));
    c.push_str(&format!("struct {name} *{name}_oz_alloc(void)\n{{\n", name = name));
    c.push_str(&format!(
        "\tstruct {name} *obj = malloc(sizeof(struct {name}));\n\
         \tif (!obj) {{\n\t\treturn (struct {name} *)0;\n\t}}\n\
         \tmemset(obj, 0, sizeof(struct {name}));\n",
        name = name
    ));
    c.push_str(&format!(
        "\t((struct {root} *)obj)->oz_class_id = OZ_STATIC_CLASS_{name};\n\
         \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n",
        root = root,
        name = name
    ));
    c.push_str("\treturn obj;\n}\n\n");

    c.push_str(&format!(
        "/* synthesized: releases {name}'s items, its items buffer, and its own\n * \
storage -- called only from oz_static_release, once the refcount reaches\n * \
zero (not from source; OZArray.m has no -dealloc of its own) */\n",
        name = name
    ));
    c.push_str(&format!(
        "void {name}_oz_free(struct {name} *obj)\n{{\n\
         \tfor (unsigned int i = 0; i < obj->_count; i++) {{\n\
         \t\toz_static_release((struct {root} *)obj->_items[i]);\n\
         \t}}\n\
         \tfree(obj->_items);\n\
         \tfree(obj);\n\
         }}\n\n",
        root = root,
        name = name
    ));

    c.push_str(&format!(
        "/* synthesized: builds a fresh {name} from a stack buffer of {root}\n * \
pointers -- backs the '@[...]' boxed array literal desugar (not from\n * \
source) */\n",
        name = name,
        root = root
    ));
    c.push_str(&format!(
        "struct {name} *{name}_oz_initWithItems(void **src, unsigned int count)\n{{\n\
         \tstruct {name} *arr = {name}_oz_alloc();\n\
         \tif (!arr) {{\n\t\treturn (struct {name} *)0;\n\t}}\n\
         \tvoid **items = malloc(count * sizeof(void *));\n\
         \tif (!items) {{\n\t\t{name}_oz_free(arr);\n\t\treturn (struct {name} *)0;\n\t}}\n\
         \tfor (unsigned int i = 0; i < count; i++) {{\n\t\titems[i] = src[i];\n\t}}\n\
         \tarr->_items = items;\n\
         \tarr->_count = count;\n\
         \treturn arr;\n\
         }}\n\n",
        name = name
    ));
    c
}

/// OZDictionary-specific replacement for `render_alloc_free`, the same
/// shape as `render_array_support`: alloc is identical, but free must
/// also release every key and value and free their buffer -- OZDictionary.m
/// (transplanted from `src/OZDictionary.m`) has no `-dealloc` of its own,
/// same reason as OZArray. Also emits `OZDictionary_oz_initWithKeysValues`,
/// the malloc-based builder backing the `@{...}` boxed dictionary literal
/// desugar in `emit.rs`. Keys and values share one contiguous buffer
/// (`_keys` pointing at its first half, `_values` at its second),
/// mirroring the real Python pipeline's own `{Name}_initWithKeysValues`
/// template (`tools/oz_transpile/templates/class_header.h.j2`) -- pool-
/// backed there, malloc-based here.
pub(crate) fn render_dict_support(name: &str, root: &str) -> String {
    let mut c = String::new();
    c.push_str(&format!(
        "/* synthesized: allocates and zero-initializes a new {name} (not from source) */\n",
        name = name
    ));
    c.push_str(&format!("struct {name} *{name}_oz_alloc(void)\n{{\n", name = name));
    c.push_str(&format!(
        "\tstruct {name} *obj = malloc(sizeof(struct {name}));\n\
         \tif (!obj) {{\n\t\treturn (struct {name} *)0;\n\t}}\n\
         \tmemset(obj, 0, sizeof(struct {name}));\n",
        name = name
    ));
    c.push_str(&format!(
        "\t((struct {root} *)obj)->oz_class_id = OZ_STATIC_CLASS_{name};\n\
         \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n",
        root = root,
        name = name
    ));
    c.push_str("\treturn obj;\n}\n\n");

    c.push_str(&format!(
        "/* synthesized: releases {name}'s keys, its values, their shared\n * \
buffer, and its own storage -- called only from oz_static_release, once\n * \
the refcount reaches zero (not from source; OZDictionary.m has no\n * \
-dealloc of its own) */\n",
        name = name
    ));
    c.push_str(&format!(
        "void {name}_oz_free(struct {name} *obj)\n{{\n\
         \tfor (unsigned int i = 0; i < obj->_count; i++) {{\n\
         \t\toz_static_release((struct {root} *)obj->_keys[i]);\n\
         \t\toz_static_release((struct {root} *)obj->_values[i]);\n\
         \t}}\n\
         \tfree(obj->_keys);\n\
         \tfree(obj);\n\
         }}\n\n",
        root = root,
        name = name
    ));

    c.push_str(&format!(
        "/* synthesized: builds a fresh {name} from parallel stack buffers of\n * \
{root} pointers -- backs the '@{{...}}' boxed dictionary literal desugar\n * \
(not from source) */\n",
        name = name,
        root = root
    ));
    c.push_str(&format!(
        "struct {name} *{name}_oz_initWithKeysValues(void **keys, void **values, unsigned int count)\n{{\n\
         \tstruct {name} *dict = {name}_oz_alloc();\n\
         \tif (!dict) {{\n\t\treturn (struct {name} *)0;\n\t}}\n\
         \tvoid **buf = malloc(count * 2 * sizeof(void *));\n\
         \tif (!buf) {{\n\t\t{name}_oz_free(dict);\n\t\treturn (struct {name} *)0;\n\t}}\n\
         \tfor (unsigned int i = 0; i < count; i++) {{\n\
         \t\tbuf[i] = keys[i];\n\
         \t\tbuf[count + i] = values[i];\n\
         \t}}\n\
         \tdict->_keys = buf;\n\
         \tdict->_values = buf + count;\n\
         \tdict->_count = count;\n\
         \treturn dict;\n\
         }}\n\n",
        name = name
    ));
    c
}

/// `OZ_PROTOCOL_SEND_{selector}`: routes a dynamically-dispatched
/// selector (see `Program::is_dynamically_dispatched` -- protocol-
/// declared, always-polymorphic like `isEqual:`, or implemented by more
/// than one class) to whichever class implements it, switching on
/// `self->oz_class_id`. Real Objective-C dispatch doesn't check
/// protocol conformance at the call site either -- a protocol is a
/// compile-time contract, not a runtime filter -- so this includes
/// every class in the program with a `case`, not just the ones that
/// implement the selector directly: a class that *inherits* it (no
/// override of its own) still needs to route to whichever ancestor
/// actually defines it (`find_defining_method`, the same single-
/// inheritance lookup `find_defining_dealloc` already does) -- left
/// out, its instances would silently fall through to `default` instead.
/// Generated once per distinct (selector, is_class_method) pair;
/// skipped entirely if nothing in the program implements it at all.
/// Return type/params come from whichever implementing class was
/// declared first, since a shared dispatch function needs one
/// signature and every implementor of a given selector is expected to
/// match it.
fn render_protocol_dispatch(program: &Program, root: &str) -> (String, String) {
    let mut h = String::new();
    let mut c = String::new();
    for m in program.dynamic_dispatch_methods() {
        let routed: Vec<(&String, String)> = program
            .class_order
            .iter()
            .filter_map(|name| {
                find_defining_method(program, name, &m.selector, m.is_class_method)
                    .map(|defining| (name, defining))
            })
            .collect();
        if routed.is_empty() {
            continue;
        }
        let selc = crate::emit::selector_to_c(&m.selector);
        let fn_name = format!("OZ_PROTOCOL_SEND_{}", selc);
        let mut params = format!("struct {} *self", root);
        for (pname, ptype) in &m.params {
            params.push_str(", ");
            params.push_str(&crate::emit::render_param(ptype, pname));
        }
        let arg_names: Vec<&str> = m.params.iter().map(|(n, _)| n.as_str()).collect();

        // `m.return_type` is one implementor's own resolved `instancetype`
        // (the first one found declaring this selector), e.g. `struct
        // OZArray *` -- fine for *that* class's own prototype, but wrong
        // here: this one shared function also routes to every other
        // implementor (`struct OZDictionary *`, ...), whose concrete type
        // the caller never statically knows anyway (that's the reason
        // this needs a runtime switch at all). `void *` is the same
        // "any object" stand-in `render_type` already uses for a bare
        // `id` -- every struct pointer converts to it with no cast.
        let ret_ty = if m.returns_instancetype { "void *".to_string() } else { m.return_type.clone() };

        h.push_str(&format!(
            "/* protocol dispatch: routes '{}' to whichever class implements it */\n",
            m.selector
        ));
        h.push_str(&format!("{} {}({});\n", ret_ty, fn_name, params));

        c.push_str(&format!(
            "/* protocol dispatch: routes '{}' to whichever class implements it\n * (not from source) */\n",
            m.selector
        ));
        c.push_str(&format!("{} {}({})\n{{\n\tswitch (self->oz_class_id) {{\n", ret_ty, fn_name, params));
        for (name, defining) in &routed {
            let target = crate::emit::method_fn_name(defining, &m.selector, m.is_class_method);
            let mut call_args = vec![format!("(struct {} *)self", defining)];
            call_args.extend(arg_names.iter().map(|a| a.to_string()));
            let call = format!("{}({})", target, call_args.join(", "));
            if m.return_type == "void" {
                c.push_str(&format!("\tcase OZ_STATIC_CLASS_{}: {}; return;\n", name, call));
            } else {
                c.push_str(&format!("\tcase OZ_STATIC_CLASS_{}: return {};\n", name, call));
            }
        }
        if m.return_type == "void" {
            c.push_str("\tdefault: return;\n\t}\n}\n\n");
        } else {
            c.push_str("\tdefault: return (");
            c.push_str(&ret_ty);
            c.push_str(")0;\n\t}\n}\n\n");
        }
    }
    (h, c)
}

fn class_label(program: &Program, name: &str, id: usize) -> String {
    match &program.classes[name].superclass {
        Some(sup) => format!("-- {} (id {}, extends {}) --", name, id, sup),
        None => format!("-- {} (id {}, root) --", name, id),
    }
}

pub fn render(
    program: &Program,
    hoisted_structs: &[(String, String)],
    hoisted_enums: &[String],
    hoisted_forward_decls: &[String],
) -> (String, String) {
    let root = program.root_class().map(|s| s.to_string());
    // The root class always terminates the [super dealloc] chain. If the
    // user didn't write one, synthesize a no-op so every subclass's chain
    // still resolves statically.
    let root_needs_synthetic_dealloc =
        root.as_deref().is_some_and(|r| find_defining_dealloc(program, r).is_none());
    let struct_order = topological_order(program);
    let struct_text: std::collections::HashMap<&str, &str> =
        hoisted_structs.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();

    let mut h = String::new();
    h.push_str("/* Auto-generated by oz_static -- do not edit */\n#pragma once\n\n");
    h.push_str("#include \"platform/oz_platform.h\"\n#include <stdbool.h>\n#include <stdlib.h>\n#include <string.h>\n\n");
    // `id`/`Class`/`BOOL` are real Objective-C built-in types with no
    // plain-C equivalent, so left undefined they'd be invalid C tokens
    // wherever this spike can't translate them (ivar declarations, the
    // inner parameter list of a `(^)`-to-`(*)`-converted block type, a
    // plain top-level C function's own signature) -- `collect::render_type`
    // separately resolves a method's own `id` parameter/return type to
    // `void *`, but that doesn't reach those other spots. Defining all
    // three here, included by both the primary source and this companion,
    // covers every spot at once. `Class` has no runtime representation in
    // this design either (no class-object introspection) -- `void *` is
    // just a placeholder so a declared-but-uncalled `+ (Class)class`-style
    // method still compiles.
    h.push_str("typedef void *id;\ntypedef void *Class;\ntypedef bool BOOL;\n\n");

    // Hardcoded rather than pulled from the real `Foundation/OZLog.h` via
    // `#import` splicing: that header has no class/protocol node for
    // `emit.rs`'s per-origin split to hang it on, so its spliced-in text
    // lands in an origin nothing else `#include`s. Mirrors
    // `oz_dispatch.h.j2`'s own hardcoded line in the Python pipeline --
    // `src/OZLog.c` (linked in unconditionally by both backends) provides
    // the one real definition either way.
    // `_oz_get_log_precision` is called directly by any class's own
    // `cDescription:maxLength:` (not just through `OZLog()` itself) --
    // same splice-visibility gap as `OZLog` above, same fix. Its one real
    // definition is `src/OZLog.c:26`, linked in unconditionally.
    h.push_str(
        "/* OZLog -- formatted logging with %@ object support; defined in src/OZLog.c */\n\
         void OZLog(const char *fmt, ...);\n\
         int _oz_get_log_precision(void);\n\n",
    );

    if !hoisted_forward_decls.is_empty() {
        h.push_str("/* forward-declared structs (no body in source), hoisted here so a\n * method prototype below referencing one as a pointer type still compiles */\n");
        for d in hoisted_forward_decls {
            h.push_str(d);
            h.push_str(";\n");
        }
        h.push('\n');
    }

    if !hoisted_enums.is_empty() {
        h.push_str("/* enum definitions, hoisted here from source so they're complete\n * before any method prototype below references one by value */\n");
        for e in hoisted_enums {
            h.push_str(e);
            h.push_str(";\n");
        }
        h.push('\n');
    }

    // One labeled section per class: its id, its struct (full for root,
    // forward-declared otherwise), its method prototypes (needed here so
    // the dealloc-dispatch switch below can call them across the
    // translation-unit boundary), and its alloc/free prototypes.
    for name in &struct_order {
        let id = program.class_id(name).unwrap_or(0);
        h.push_str(&format!("/* {} */\n", class_label(program, name, id)));
        h.push_str(&format!("#define OZ_STATIC_CLASS_{} {}\n", name, id));
        match struct_text.get(name.as_str()) {
            Some(text) => h.push_str(text),
            None => h.push_str(&format!("struct {};\n", name)),
        }
        for m in &program.classes[name].methods {
            h.push_str(&crate::emit::render_prototype(name, m));
        }
        h.push_str(&format!(
            "struct {name} *{name}_oz_alloc(void);\nvoid {name}_oz_free(struct {name} *obj);\n",
            name = name
        ));
        if root.as_deref() == Some(name.as_str()) {
            h.push_str(&format!(
                "struct {root} *oz_static_retain(struct {root} *self);\n\
                 void oz_static_release(struct {root} *self);\n\
                 int oz_static_retain_count(struct {root} *self);\n",
                root = name
            ));
            if root_needs_synthetic_dealloc {
                h.push_str(&format!("void {root}_dealloc(struct {root} *self);\n", root = name));
            }
        }
        h.push('\n');
    }

    let mut c = String::new();
    c.push_str("/* Auto-generated by oz_static -- do not edit */\n#include \"oz_static_dispatch.h\"\n\n");

    if let Some(root) = &root {
        c.push_str(&render_alloc_free(root, root));

        if root_needs_synthetic_dealloc {
            c.push_str(&format!(
                "/* synthesized: {root} has no -dealloc in source -- a no-op so every\n * \
subclass's [super dealloc] chain still resolves statically (not from source) */\n\
void {root}_dealloc(struct {root} *self)\n{{\n\t(void)self;\n}}\n\n",
                root = root
            ));
        }

        c.push_str(
            "/* synthesized: increments the retain count; shared by every class,\n * \
not tied to one (not from source) */\n",
        );
        c.push_str(&format!(
            "struct {root} *oz_static_retain(struct {root} *self)\n{{\n\
             \tif (self) {{\n\t\toz_atomic_inc(&self->oz_refcount);\n\t}}\n\treturn self;\n}}\n\n",
            root = root
        ));

        c.push_str(
            "/* synthesized: reads the current retain count; 0 for a nil\n * \
receiver (not from source) */\n",
        );
        c.push_str(&format!(
            "int oz_static_retain_count(struct {root} *self)\n{{\n\
             \tif (!self) {{\n\t\treturn 0;\n\t}}\n\treturn oz_atomic_get(&self->oz_refcount);\n}}\n\n",
            root = root
        ));

        c.push_str(
            "/* dealloc dispatch: the one virtual call this design needs. Resolved\n * \
entirely at compile time via this class_id switch (the \"const\n * \
vtable\") -- never mutated at runtime. */\n",
        );
        c.push_str(&format!(
            "void oz_static_release(struct {root} *self)\n{{\n\
             \tif (!self) {{\n\t\treturn;\n\t}}\n\
             \tif (!oz_atomic_dec_and_test(&self->oz_refcount)) {{\n\t\treturn;\n\t}}\n\
             \tif (self->oz_deallocating) {{\n\t\treturn;\n\t}}\n\
             \tself->oz_deallocating = 1;\n\
             \tswitch (self->oz_class_id) {{\n",
            root = root
        ));
        for name in &program.class_order {
            let defining =
                find_defining_dealloc(program, name).unwrap_or_else(|| root.clone());
            c.push_str(&format!("\tcase OZ_STATIC_CLASS_{}: /* {} */\n", name, name));
            c.push_str(&format!("\t\t{}_dealloc((struct {} *)self);\n", defining, defining));
            c.push_str(&format!("\t\t{}_oz_free((struct {} *)self);\n\t\tbreak;\n", name, name));
        }
        c.push_str("\tdefault:\n\t\tbreak;\n\t}\n}\n\n");
    }

    for name in &program.class_order {
        if program.classes[name].has_class_initialize {
            c.push_str(&format!(
                "/* {name}: +initialize registration (runs once, before main()) */\n\
                 OZ_AUTO_INIT({name}_oz_auto_init, {name}_initialize_cls);\n\n",
                name = name
            ));
        }
    }

    if let Some(root) = &root {
        let (proto_h, proto_c) = render_protocol_dispatch(program, root);
        h.push_str(&proto_h);
        c.push_str(&proto_c);
    }

    (h, c)
}
