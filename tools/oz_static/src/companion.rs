// SPDX-License-Identifier: Apache-2.0
//
// companion.rs - the one small generated file for multi-implementor
// dispatch (dealloc's const-vtable, computed entirely at compile time and
// never mutated at runtime) and pool/init registration, mirroring the
// existing oz_dispatch.h/.c pattern used by the Python pipeline.
//
// Only the root class's full struct lives here, because oz_static_retain/
// oz_static_release/the dealloc switch are generic (shared by every
// class) and need its tracking fields (`_meta`, `oz_refcount`) directly
// -- mirroring how the Python pipeline already
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

/// The per-class slab definition plus the `extern` a split output needs to
/// reach it, mirroring the oracle's own emission (`emit.py`:
/// `OZ_SLAB_DEFINE(oz_slab_{name}, sizeof(struct {name}), {count}, 4)`,
/// with `extern oz_slab_t oz_slab_{name};` in the header).
///
/// `OZ_SLAB_DEFINE` is a real definition, so it must appear exactly once
/// per class; it is emitted immediately ahead of that class's alloc
/// function, which also guarantees it is in scope there without relying on
/// the `extern`.
const FORWARD_DECL_MARKER: &str = "/*@@OZ_STRUCT_FORWARD_DECLS@@*/\n";

/// Forward-declare every struct tag the companion header names but never
/// declares, at the marker planted right after the typedefs.
///
/// The companion declares prototypes for every class, and those signatures
/// can name a struct defined in a *per-class* header the companion does
/// not include -- `struct color *` from a sample's own `Car.h`, say. C then
/// treats the tag as new and scoped to that parameter list, and the real
/// declaration elsewhere becomes `error: conflicting types for
/// 'Car_initWithColor_andModel_'`. It is the same failure the propagated
/// angled includes fix for system types, but a quoted project header
/// cannot be copied here (it resolves relative to a directory the
/// companion does not share -- see `imports::collect_system_includes`), so
/// the tag is declared instead of the header included.
///
/// Declaring a tag that a later line fully defines is legal C, and so is
/// declaring one never defined at all as long as it is only used through a
/// pointer -- which is exactly the case here, since a by-value parameter
/// would need the definition and is handled by the existing struct/enum
/// hoisting. So over-declaring is harmless and under-declaring is not.
fn forward_declare_unknown_struct_tags(header: &str) -> String {
    let mut mentioned: Vec<String> = Vec::new();
    let mut declared: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (idx, _) in header.match_indices("struct ") {
        let rest = &header[idx + "struct ".len()..];
        let tag: String =
            rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if tag.is_empty() {
            continue;
        }
        // What follows the tag says whether this is a declaration of it
        // (`struct X;`), a definition (`struct X {`), or merely a use.
        let after = rest[tag.len()..].trim_start();
        if after.starts_with(';') || after.starts_with('{') {
            declared.insert(tag);
        } else if !mentioned.contains(&tag) {
            mentioned.push(tag);
        }
    }

    let missing: Vec<String> =
        mentioned.into_iter().filter(|tag| !declared.contains(tag)).collect();
    if missing.is_empty() {
        return header.replace(FORWARD_DECL_MARKER, "");
    }
    let mut block = String::from(
        "/* struct tags named by a prototype below but declared in no header this\n * \
one includes -- forward-declared so the tag is file-scoped rather than\n * \
scoped to a parameter list */\n",
    );
    for tag in missing {
        block.push_str(&format!("struct {};\n", tag));
    }
    block.push('\n');
    header.replace(FORWARD_DECL_MARKER, &block)
}

/// An opt-in trap for slab exhaustion, emitted inside every class's alloc.
///
/// Returning nil when a pool runs out is the contract, and
/// `tests/behavior/cases/lifecycle/alloc_failure_enomem.m` asserts it
/// exactly -- a one-block pool, second alloc NULL. So this cannot be on by
/// default.
///
/// It exists because that nil then travels. A factory like OZQ31's
/// `+fixedWithInt32:` writes through the alloc result without checking, so
/// exhaustion surfaces as `EXC_BAD_ACCESS` inside a function that has
/// nothing to do with the cause, with no mention of which pool ran out.
/// Building with `-DOZ_STATIC_TRAP_POOL_EXHAUSTION` converts that into an
/// immediate named failure at the point of exhaustion, which is the
/// difference between a five-minute diagnosis and a debugger session.
fn render_exhaustion_trap(name: &str) -> String {
    format!(
        "#ifdef OZ_STATIC_TRAP_POOL_EXHAUSTION\n\t\toz_assert_msg(0, \
         \"{name} pool exhausted -- raise it with --pool-sizes {name}=N or an \
         oz-pool comment\");\n#endif\n",
        name = name
    )
}

fn render_slab_define(name: &str, slots: usize) -> String {
    format!(
        "/* synthesized: backing storage for every {name} instance -- {slots} slot(s), \
         sized from this translation unit's allocation sites (override with --pool-sizes) */\n\
         OZ_SLAB_DEFINE(oz_slab_{name}, sizeof(struct {name}), {slots}, {align});\n\n",
        name = name,
        slots = slots,
        align = crate::pools::SLAB_ALIGNMENT
    )
}

/// `{name}_oz_alloc`/`{name}_oz_free`, backed by the PAL slab allocator
/// (`oz_slab_alloc`/`oz_slab_free`) rather than malloc: a real
/// `k_mem_slab` on Zephyr, and on host a malloc-backed slab that still
/// enforces the block count, so pool exhaustion is observable in host
/// tests instead of only on hardware
/// (`platform/oz_platform_{zephyr,host}.h`).
///
/// `root` is whichever class has no superclass; alloc always needs it to
/// set the tracking fields regardless of which class is being allocated.
/// `{name}_oz_release_ivars`: releases every object ivar an instance owns,
/// called from the release path once the class's `-dealloc` body has run.
///
/// This is oz_static's equivalent of the oracle's auto-dealloc
/// (`emit.py::_emit_auto_dealloc`), but deliberately *not* a translation of
/// it. The oracle appends these releases to a user-written `-dealloc` as
/// well, so a class whose `-dealloc` releases its own ivars -- ordinary
/// manual-retain/release teardown -- gets each one released twice. Real ARC
/// avoids that by making `[_ivar release]` in `-dealloc` a compile error
/// rather than by adding a second release, and that is the rule followed
/// here: the release is automatic, and an explicit one is rejected
/// (`staticbar::check_dealloc_body`).
///
/// Lives in the owning class's own file because the companion header only
/// forward-declares non-root structs and so cannot reach an ivar through
/// one.
fn render_release_ivars(name: &str, root: &str, owned: &[String]) -> String {
    if owned.is_empty() {
        return String::new();
    }
    let mut c = format!(
        "/* synthesized: releases the {} object ivar(s) a {} owns -- called from\n * oz_static_release once this class's -dealloc has run (not from source) */\n",
        owned.len(),
        name
    );
    c.push_str(&format!(
        "void {name}_oz_release_ivars(struct {name} *self)\n{{\n",
        name = name
    ));
    for path in owned {
        c.push_str(&format!(
            "\toz_static_release((struct {root} *)self->{path});\n",
            root = root,
            path = path
        ));
    }
    c.push_str("}\n\n");
    c
}

/// `{name}_oz_alloc_with_heap`, backing `[Cls allocWithHeap:h]`: the same
/// initialization as `{name}_oz_alloc`, but the storage comes from an
/// `OZHeap` (or the system heap when the argument is nil) instead of the
/// class's slab, and the object is marked so `{name}_oz_free` knows to
/// return it there.
///
/// `oz_heap_obj_alloc` is declared by the PAL and defined in the companion
/// (see `render_heap_bridge`) -- it needs `struct OZHeap` complete, which
/// only generated code has.
///
/// Guarded by `OZ_HEAP_SUPPORT` as well as by `--heap-support`, matching the
/// oracle (`templates/class_header.h.j2`): the flag decides whether the code
/// is generated at all, the macro whether the PAL exposes the heap it needs.
/// `OZHeap_oz_inner`: hands back the address of OZHeap's `_inner` ivar.
///
/// The heap bridge (`render_heap_bridge`) needs it, and cannot reach it
/// itself: the companion header only forward-declares any class that is not
/// the root, so `heap->_inner` is not available there -- and in single-file
/// mode the full struct is in the one output file, not in the companion at
/// all. So the accessor is defined where the struct *is* complete, which is
/// OZHeap's own file, and only its prototype crosses into the companion.
/// `render_release_ivars` is split for exactly the same reason.
fn render_heap_inner_accessor(name: &str, heap_support: bool) -> String {
    if !heap_support || name != "OZHeap" {
        return String::new();
    }
    "/* synthesized: the companion's heap bridge needs OZHeap's inner store, and\n * only this file has the complete struct to reach it through (not from\n * source) */\n#ifdef OZ_HEAP_SUPPORT\nstruct oz_heap_inner *OZHeap_oz_inner(struct OZHeap *self)\n{\n\treturn &self->_inner;\n}\n#endif\n\n"
        .to_string()
}

fn render_heap_alloc(name: &str, root: &str, heap_support: bool) -> String {
    if !heap_support {
        return String::new();
    }
    format!(
        "/* synthesized: allocates a new {name} from an OZHeap rather than its slab --\n * \
backs '[{name} allocWithHeap:h]' (not from source) */\n\
         #ifdef OZ_HEAP_SUPPORT\n\
         struct {name} *{name}_oz_alloc_with_heap(struct {root} *heap_obj)\n{{\n\
         \tstruct {name} *obj = (struct {name} *)oz_heap_obj_alloc(\n\
         \t\t(struct OZHeap *)heap_obj, sizeof(struct {name}));\n\
         \tif (!obj) {{\n\t\treturn (struct {name} *)0;\n\t}}\n\
         \tmemset(obj, 0, sizeof(struct {name}));\n\
         \t((struct {root} *)obj)->_meta.class_id = OZ_STATIC_CLASS_{name};\n\
         \t((struct {root} *)obj)->_meta.heap_allocated = 1;\n\
         \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n\
         \treturn obj;\n}}\n\
         #endif\n\n",
        name = name,
        root = root
    )
}

/// The first lines of every `{name}_oz_free`: an object that came from a
/// heap has no slot in the class's slab to return, so it goes back to the
/// heap and the slab is never touched.
fn render_heap_free_check(root: &str, heap_support: bool) -> String {
    if !heap_support {
        return String::new();
    }
    format!(
        "#ifdef OZ_HEAP_SUPPORT\n\
         \tif (((struct {root} *)obj)->_meta.heap_allocated) {{\n\
         \t\toz_heap_obj_free((void *)obj);\n\
         \t\treturn;\n\t}}\n\
         #endif\n",
        root = root
    )
}

/// `oz_heap_obj_alloc`/`oz_heap_obj_free`, which
/// `platform/oz_platform_{zephyr,host}.h` declare and deliberately leave to
/// generated code: both need `struct OZHeap` to be a complete type, and the
/// PAL cannot see it. Same division as the oracle's `oz_dispatch.c.j2`.
fn render_heap_bridge(heap_support: bool) -> String {
    if !heap_support {
        return String::new();
    }
    "/* synthesized: the two heap entry points the PAL declares but leaves to\n * generated code -- both need 'struct OZHeap' complete, which only this\n * file has (not from source) */\n#ifdef OZ_HEAP_SUPPORT\nvoid *oz_heap_obj_alloc(struct OZHeap *heap, size_t size)\n{\n\tif (heap) {\n\t\treturn oz_heap_alloc_obj(OZHeap_oz_inner(heap), heap, size);\n\t}\n\treturn oz_sys_heap_alloc(size);\n}\n\nvoid oz_heap_obj_free(void *obj)\n{\n\tstruct oz_heap_hdr *hdr = (struct oz_heap_hdr *)\n\t\t((char *)obj - offsetof(struct oz_heap_hdr, obj));\n\tif (hdr->heap) {\n\t\toz_heap_free_obj(OZHeap_oz_inner(hdr->heap), obj);\n\t} else {\n\t\toz_sys_heap_free(obj);\n\t}\n}\n#endif\n\n"
        .to_string()
}

pub(crate) fn render_alloc_free(
    name: &str,
    root: &str,
    slots: usize,
    owned_ivars: &[String],
    heap_support: bool,
) -> String {
    let mut c = render_slab_define(name, slots);
    c.push_str(&render_release_ivars(name, root, owned_ivars));
    c.push_str(&format!(
        "/* synthesized: allocates and zero-initializes a new {name} (not from source) */\n",
        name = name
    ));
    c.push_str(&format!("struct {name} *{name}_oz_alloc(void)\n{{\n", name = name));
    c.push_str(&format!(
        "\tstruct {name} *obj;\n\
         \tif (oz_slab_alloc(&oz_slab_{name}, (void **)&obj) != 0) {{\n\
         {trap}\
         \t\treturn (struct {name} *)0;\n\t}}\n\
         \tmemset(obj, 0, sizeof(struct {name}));\n",
        name = name,
        trap = render_exhaustion_trap(name)
    ));
    c.push_str(&format!(
        "\t((struct {root} *)obj)->_meta.class_id = OZ_STATIC_CLASS_{name};\n\
         \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n",
        root = root,
        name = name
    ));
    c.push_str("\treturn obj;\n}\n\n");
    c.push_str(&render_heap_alloc(name, root, heap_support));
    c.push_str(&render_heap_inner_accessor(name, heap_support));
    c.push_str(&format!(
        "/* synthesized: returns {name}'s slot to its slab -- called only from\n * \
oz_static_release, once the refcount reaches zero (not from source) */\n",
        name = name
    ));
    c.push_str(&format!(
        "void {name}_oz_free(struct {name} *obj)\n{{\n\
         {heap_check}\
         \toz_slab_free(&oz_slab_{name}, (void *)obj);\n}}\n\n",
        name = name,
        heap_check = render_heap_free_check(root, heap_support)
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
pub(crate) fn render_array_support(
    name: &str,
    root: &str,
    slots: usize,
    owned_ivars: &[String],
    heap_support: bool,
) -> String {
    let mut c = render_slab_define(name, slots);
    c.push_str(&render_release_ivars(name, root, owned_ivars));
    c.push_str(&format!(
        "/* synthesized: allocates and zero-initializes a new {name} (not from source) */\n",
        name = name
    ));
    c.push_str(&format!("struct {name} *{name}_oz_alloc(void)\n{{\n", name = name));
    c.push_str(&format!(
        "\tstruct {name} *obj;\n\
         \tif (oz_slab_alloc(&oz_slab_{name}, (void **)&obj) != 0) {{\n\
         {trap}\
         \t\treturn (struct {name} *)0;\n\t}}\n\
         \tmemset(obj, 0, sizeof(struct {name}));\n",
        name = name,
        trap = render_exhaustion_trap(name)
    ));
    c.push_str(&format!(
        "\t((struct {root} *)obj)->_meta.class_id = OZ_STATIC_CLASS_{name};\n\
         \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n",
        root = root,
        name = name
    ));
    c.push_str("\treturn obj;\n}\n\n");

    c.push_str(&render_heap_alloc(name, root, heap_support));
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
         {heap_check}\
         \toz_slab_free(&oz_slab_{name}, (void *)obj);\n\
         }}\n\n",
        root = root,
        name = name,
        heap_check = render_heap_free_check(root, heap_support)
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
pub(crate) fn render_dict_support(
    name: &str,
    root: &str,
    slots: usize,
    owned_ivars: &[String],
    heap_support: bool,
) -> String {
    let mut c = render_slab_define(name, slots);
    c.push_str(&render_release_ivars(name, root, owned_ivars));
    c.push_str(&format!(
        "/* synthesized: allocates and zero-initializes a new {name} (not from source) */\n",
        name = name
    ));
    c.push_str(&format!("struct {name} *{name}_oz_alloc(void)\n{{\n", name = name));
    c.push_str(&format!(
        "\tstruct {name} *obj;\n\
         \tif (oz_slab_alloc(&oz_slab_{name}, (void **)&obj) != 0) {{\n\
         {trap}\
         \t\treturn (struct {name} *)0;\n\t}}\n\
         \tmemset(obj, 0, sizeof(struct {name}));\n",
        name = name,
        trap = render_exhaustion_trap(name)
    ));
    c.push_str(&format!(
        "\t((struct {root} *)obj)->_meta.class_id = OZ_STATIC_CLASS_{name};\n\
         \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n",
        root = root,
        name = name
    ));
    c.push_str("\treturn obj;\n}\n\n");

    c.push_str(&render_heap_alloc(name, root, heap_support));
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
         {heap_check}\
         \toz_slab_free(&oz_slab_{name}, (void *)obj);\n\
         }}\n\n",
        root = root,
        name = name,
        heap_check = render_heap_free_check(root, heap_support)
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
/// `self->_meta.class_id`. Real Objective-C dispatch doesn't check
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
            // A selector declared but never defined is not a callable
            // function, so routing to it emits a call that fails at link
            // time with an undefined symbol -- see
            // `Program::method_is_defined` for the concrete case.
            .filter(|(_, defining)| {
                program.method_is_defined(defining, &m.selector, m.is_class_method)
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
            params.push_str(&crate::emit::render_param(ptype, pname, Some(root)));
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
        c.push_str(&format!("{} {}({})\n{{\n\tswitch (self->_meta.class_id) {{\n", ret_ty, fn_name, params));
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
    hoisted_c_structs: &[String],
    pools: &crate::pools::PoolSizes,
    system_includes: &[String],
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
    // The `id`/`Class`/`BOOL` typedefs come first, before any `#include`,
    // because an include here can re-enter the generated headers: the PAL
    // (`platform/oz_assert.h`) includes `assert.h`, which in a split
    // output resolves to oz_static's *own* generated `assert.h` -- itself
    // a translation of the SDK shim -- which pulls in the class headers,
    // whose prototypes name `Class` and `BOOL`. Declared after the
    // includes, those prototypes are reached while this header is still
    // only four lines in, and the build fails with `unknown type name
    // 'Class'`. They depend on nothing but `bool`, so hoisting them above
    // every include is both safe and sufficient.
    h.push_str("#include <stdbool.h>\n\n");
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
    // Replaced, once the whole header is built, by a forward declaration
    // for every struct tag it mentions but never declares -- see
    // `forward_declare_unknown_struct_tags`.
    h.push_str(FORWARD_DECL_MARKER);

    h.push_str("#include \"platform/oz_platform.h\"\n#include <stdlib.h>\n#include <string.h>\n\n");
    // Ahead of every prototype below, because a prototype may name a type
    // only one of these headers declares -- see
    // `imports::collect_system_includes` for the whole reasoning and for
    // why only angled includes are carried.
    if !system_includes.is_empty() {
        h.push_str(
            "/* carried over from the source's own #include lines, so a prototype\n * \
below naming a type one of them declares sees the real definition */\n",
        );
        for line in system_includes {
            h.push_str(line);
            h.push('\n');
        }
        h.push('\n');
    }

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

    // Declared here, defined in OZHeap's own file -- see
    // `render_heap_inner_accessor`.
    if program.heap_support && program.is_class("OZHeap") {
        h.push_str(
            "/* OZHeap's inner store, reached through an accessor because this header\n * only forward-declares the struct -- see the definition in OZHeap's file */\n#ifdef OZ_HEAP_SUPPORT\nstruct oz_heap_inner *OZHeap_oz_inner(struct OZHeap *self);\n#endif\n\n",
        );
    }

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

    // After the enums, not before: a hoisted struct can have an enum field
    // by value, and then needs that enum complete first --
    // `tests/behavior/cases/regression/issue_090_header_preservation.m`
    // has exactly that ("field has incomplete type 'enum sensor_state'").
    // Nothing runs the other way: an enum cannot contain a struct.
    if !hoisted_c_structs.is_empty() {
        h.push_str(
            "/* plain C struct and union definitions, hoisted here from source so\n * the type is complete before any method prototype below returns or\n * takes one, and in every generated file rather than only the one it\n * was written in. Source order is kept: one may contain the other. */\n",
        );
        for d in hoisted_c_structs {
            h.push_str(d);
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
            h.push_str(&crate::emit::render_prototype(name, m, root.as_deref()));
        }
        // The heap allocator's prototype is guarded, not omitted: the
        // definition is `#ifdef OZ_HEAP_SUPPORT` too, so a caller compiled
        // without the macro must not see a declaration for a function that
        // will not exist.
        let heap_proto = if program.heap_support {
            format!(
                "#ifdef OZ_HEAP_SUPPORT\nstruct {name} *{name}_oz_alloc_with_heap(struct {root} *heap_obj);\n#endif\n",
                name = name,
                root = root.as_deref().unwrap_or(name)
            )
        } else {
            String::new()
        };
        h.push_str(&format!(
            "struct {name} *{name}_oz_alloc(void);\nvoid {name}_oz_free(struct {name} *obj);\n{heap_proto}",
            name = name,
            heap_proto = heap_proto
        ));
        // Defined in the owning class's own file (see `render_release_ivars`),
        // declared here because the release switch below calls through it.
        if !program.owned_object_ivars(name).is_empty() {
            h.push_str(&format!(
                "void {name}_oz_release_ivars(struct {name} *self);\n",
                name = name
            ));
        }
        if root.as_deref() == Some(name.as_str()) {
            h.push_str(&format!(
                "struct {root} *oz_static_retain(struct {root} *self);\n\
                 void oz_static_release(struct {root} *self);\n\
                 int oz_static_retain_count(struct {root} *self);\n\
                 /* Refcount introspection under the name the legacy runtime used, so\n \
                 * source written against it keeps compiling (samples/mem_demo). A\n \
                 * function rather than the oracle's macro, because the real\n \
                 * src/OZObject.m already declares it as one -- a macro of the same\n \
                 * name would be expanded in that declaration and break it. */\n\
                 unsigned int __objc_refcount_get(id obj);\n",
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
    // The header above declares `_oz_get_log_precision` unconditionally,
    // because OZQ31's `-cDescription:maxLength:` calls it. Its real
    // definition is in `src/OZLog.c`, which is pure C and never transpiled,
    // so any build that does not link that file was left with an undefined
    // symbol -- a link error, with nothing naming the cause. A weak default
    // makes the symbol always resolve and lets `OZLog.c` override it where
    // it is linked. This is the oracle's own mechanism, verbatim: see
    // `tools/oz_transpile/tests/golden/simple_led/expected/Foundation/oz_dispatch.c`.
    c.push_str(
        "/* Weak default: returns -1 (no precision override).\n * \
src/OZLog.c provides the strong definition where it is linked. */\n\
         __attribute__((weak)) int _oz_get_log_precision(void) { return -1; }\n\n",
    );

    if let Some(root) = &root {
        c.push_str(&render_alloc_free(
            root,
            root,
            pools.for_class(root),
            &program.owned_object_ivars(root),
            program.heap_support,
        ));

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
        c.push_str(&render_heap_bridge(program.heap_support));
        c.push_str(&format!(
            "struct {root} *oz_static_retain(struct {root} *self)\n{{\n\
             \tif (self) {{\n\t\toz_atomic_inc(&self->oz_refcount);\n\t}}\n\treturn self;\n}}\n\n",
            root = root
        ));
        c.push_str(&format!(
            "/* Refcount introspection under the legacy runtime's name -- see the\n * \
declaration in the companion header for why this is a function. */\n\
             unsigned int __objc_refcount_get(id obj)\n{{\n\
             \treturn (unsigned int)oz_static_retain_count((struct {root} *)obj);\n}}\n\n",
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
             \tif (self->_meta.deallocating) {{\n\t\treturn;\n\t}}\n\
             \tself->_meta.deallocating = 1;\n\
             \tswitch (self->_meta.class_id) {{\n",
            root = root
        ));
        for name in &program.class_order {
            let defining =
                find_defining_dealloc(program, name).unwrap_or_else(|| root.clone());
            c.push_str(&format!("\tcase OZ_STATIC_CLASS_{}: /* {} */\n", name, name));
            c.push_str(&format!("\t\t{}_dealloc((struct {} *)self);\n", defining, defining));
            // Owned object ivars are released after the class's own
            // -dealloc body has run, so that body can still read them --
            // the order the oracle uses too. The releases cannot be inlined
            // here: this file only forward-declares non-root structs, so it
            // cannot reach an ivar through one. They live in the class's own
            // file, where its struct is complete (see
            // `render_release_ivars`), and are called through.
            if !program.owned_object_ivars(name).is_empty() {
                c.push_str(&format!(
                    "\t\t{}_oz_release_ivars((struct {} *)self);\n",
                    name, name
                ));
            }
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

    // Last, so it sees every prototype this header ended up with.
    let h = forward_declare_unknown_struct_tags(&h);

    (h, c)
}
