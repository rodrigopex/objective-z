// SPDX-License-Identifier: Apache-2.0
//
// companion.rs - the one small generated file for multi-implementor
// dispatch (dealloc's const-vtable, computed entirely at compile time and
// never mutated at runtime) and pool/init registration, mirroring the
// existing oz_dispatch.h/.c pattern used by the Python pipeline.
//
// Only the root class's full struct lives here, because oz_retain/
// oz_release/the dealloc switch are generic (shared by every
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

/// The protocol that declares a class's instances immortal
/// (`include/oz_sdk/Foundation/OZSingletonProtocol.h`). Conformance is the
/// signal, rather than a heuristic on the `+sharedInstance` shape: every
/// singleton in the repository declares it, and a wrong guess here would mark
/// an ordinary object immortal, which never gets its slab slot back.
pub(crate) const SINGLETON_PROTOCOL: &str = "OZSingletonProtocol";

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

/// Every `-dealloc` an instance of `start` must run, most-derived first.
///
/// This is the `[super dealloc]` chain, synthesized. It used to be the
/// author's to write: the dispatch called `find_defining_dealloc(start)` and
/// nothing else, so a superclass's own `-dealloc` body ran only because a
/// subclass spelled `[super dealloc]` at the end of its own. With that send
/// rejected (#428 -- ARC is always enabled, and under `-fobjc-arc` Clang
/// refuses it), there would otherwise be **no** spelling that runs a
/// superclass's cleanup and no error saying so, which is the silent
/// degradation this project forbids. Real ARC emits the super call itself;
/// so does this.
///
/// Only classes that define `-dealloc` *themselves* appear, so a class with
/// none anywhere in its chain yields an empty list and the caller falls back
/// to the root's (synthesized, no-op) one exactly as before -- which is what
/// keeps the generated C byte-identical for every class that never had a
/// `-dealloc` to chain from.
fn dealloc_chain(program: &Program, start: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut cur = Some(start.to_string());
    while let Some(name) = cur {
        let Some(info) = program.classes.get(&name) else { break };
        if info.methods.iter().any(|m| !m.is_class_method && m.selector == "dealloc") {
            chain.push(name.clone());
        }
        cur = info.superclass.clone();
    }
    chain
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
/// It exists because that nil then travels. A factory like OZNumber's
/// `+numberWithInt32:` writes through the alloc result without checking, so
/// exhaustion surfaces as `EXC_BAD_ACCESS` inside a function that has
/// nothing to do with the cause, with no mention of which pool ran out.
/// Building with `-DOZ_TRAP_POOL_EXHAUSTION` converts that into an
/// immediate named failure at the point of exhaustion, which is the
/// difference between a five-minute diagnosis and a debugger session.
fn render_exhaustion_trap(name: &str) -> String {
    format!(
        "#ifdef OZ_TRAP_POOL_EXHAUSTION\n\t\toz_assert_msg(0, \
         \"{name} pool exhausted -- raise it with --pool-sizes {name}=N or an \
         oz-pool comment\");\n#endif\n",
        name = name
    )
}

/// The same trap for the *heap* path, and under the same macro (#419).
///
/// The heap allocator had nothing at all: a bare `return (struct {name}
/// *)0;`, so heap exhaustion travelled exactly the way slab exhaustion
/// used to, and a heap can be exhausted far more easily than a statically
/// sized slab. Two arms rather than one message, because "which heap" is
/// half the diagnosis and a nil `heap_obj` means the system heap --
/// `+dynamicAlloc`, which #413 made the ordinary way to reach it -- while
/// a non-nil one is the `OZHeap` the caller passed.
///
/// **Considered and rejected: on by default for the heap.** The argument
/// for it is real -- heap exhaustion is a runtime condition rather than a
/// sizing mistake, so there is no `--pool-sizes` number to go and fix, and
/// the failure is the kind a shipped image hits rather than a developer.
/// It loses on three counts:
///
///   - The two failures are *one* contract to a caller: an allocator
///     returned nil. A build that wants that contract kept -- to implement
///     a fallback, or to test the failure path the way
///     `alloc_failure_enomem` tests the slab's -- has to be able to keep it
///     on both paths, and a default-on heap trap makes the heap path the
///     one that cannot be tested for failure at all.
///   - It would make `[Cls dynamicAlloc]` and `[Cls alloc]` behave
///     differently on the identical failure, one release after
///     `+dynamicAlloc` was introduced. Two spellings of one concept
///     disagreeing is what #418 was about.
///   - The complaint in the issue is that the failure is *unnamed*, not
///     that it is survivable. One switch that names both is the fix; a
///     second policy is not.
///
/// So the macro's name now covers a heap as well as a pool. Renaming it
/// would be a break for every build that already passes it, and `-D` flags
/// are the one interface with no deprecation path.
fn render_heap_exhaustion_trap(name: &str) -> String {
    format!(
        "#ifdef OZ_TRAP_POOL_EXHAUSTION\n\
         \t\tif (heap_obj) {{\n\
         \t\t\toz_assert_msg(0, \"{name} heap allocation failed -- the OZHeap passed \
to '[{name} dynamicAllocWithHeap:]' is exhausted; give it a larger buffer\");\n\
         \t\t}} else {{\n\
         \t\t\toz_assert_msg(0, \"{name} heap allocation failed -- the system heap is \
exhausted; raise CONFIG_HEAP_MEM_POOL_SIZE\");\n\
         \t\t}}\n\
         #endif\n",
        name = name
    )
}

/// The class's `k_mem_slab`, or **nothing at all** when `slots` is zero.
///
/// Zero means `pools::PoolSizes::ever_slab_allocated` said no: this
/// program contains no `[{name} alloc]` site and no override claiming one,
/// so every instance it can have comes from a heap
/// (`+dynamicAlloc`/`+dynamicAllocWithHeap:`) or there are none. Emitting
/// a slab anyway is what #419 was filed about -- `K_MEM_SLAB_DEFINE` with
/// a count of one costs a `struct k_mem_slab` plus `sizeof(struct {name})`
/// of BSS in every program, for every Foundation class it never allocates.
/// A zero *count* is not the alternative: `K_MEM_SLAB_DEFINE(..., 0, ...)`
/// is not a usable slab, which is why the size question was floored at one
/// and why the presence question had to be asked separately.
///
/// Same convention as the shared item pool, which has always treated zero
/// as "emit neither the pool nor the builders that draw from it"
/// (`pools::PoolSizes::item_slots`).
fn render_slab_define(name: &str, slots: usize) -> String {
    if slots == 0 {
        return format!(
            "/* synthesized: no slab for {name} -- nothing in this program sends\n * \
'[{name} alloc]', so no k_mem_slab and no static storage is reserved for\n * \
it. An instance can still exist: '[{name} dynamicAlloc]' takes its\n * \
storage from a heap and is not a slab site. Force one with\n * \
--pool-sizes {name}=N or an oz-pool comment, which is what a caller\n * \
outside the transpiled sources needs (not from source) */\n\n",
            name = name
        );
    }
    format!(
        "/* synthesized: backing storage for every {name} instance -- {slots} slot(s), \
         sized from this translation unit's allocation sites, each counted once per call \
         site of the body it escapes from (override with --pool-sizes) */\n\
         OZ_SLAB_DEFINE(oz_slab_{name}, sizeof(struct {name}), {slots}, {align});\n\n",
        name = name,
        slots = slots,
        align = crate::pools::SLAB_ALIGNMENT
    )
}

/// The body of `{name}_oz_alloc` for a class with no slab.
///
/// The prototype and the definition both stay, rather than being omitted
/// with the slab. Two callers would otherwise reference a symbol that does
/// not exist: a hand-written C caller the transpiler cannot see (the
/// reason the old floor of one existed at all), and -- unconditionally --
/// `OZArray_oz_initWithItems`/`OZDictionary_oz_initWithKeysValues`, which
/// `emit.rs` declares and defines for every program whether or not it has
/// a collection literal. An undefined symbol referenced from a function
/// nothing calls is still a link error, so a missing definition would fail
/// every program with no `@[...]` in it.
///
/// So it traps instead, named, and **not** behind
/// `OZ_TRAP_POOL_EXHAUSTION`: this is not exhaustion, which is a
/// runtime condition a caller may legitimately handle by checking for nil.
/// There is no storage here *by construction*, so reaching this function
/// is a build-time mistake -- the sizing question was answered wrong, or a
/// caller the transpiler cannot see needs an override -- and the message
/// says which flag fixes it.
fn render_no_slab_alloc(name: &str) -> String {
    format!(
        "/* synthesized: {name} has no slab (see above), so this cannot hand back\n * \
storage. It is still defined, because a caller outside the transpiled\n * \
sources references it and so does the collection-literal builder (not\n * \
from source) */\n\
         struct {name} *{name}_oz_alloc(void)\n{{\n\
         \toz_assert_msg(0, \"{name} has no slab -- nothing in this program sends \
'[{name} alloc]'. Use '[{name} dynamicAlloc]', or reserve a slab with \
--pool-sizes {name}=N or an oz-pool comment\");\n\
         \treturn (struct {name} *)0;\n}}\n\n",
        name = name
    )
}

/// The banner over `{name}_oz_free`. It used to say the function "returns
/// {name}'s slot to its slab" unconditionally, which is false for a
/// slab-less class -- and a comment asserting the opposite of the code it
/// sits on is worse than no comment (#419).
fn render_free_banner(name: &str, slots: usize) -> String {
    if slots == 0 {
        return format!(
            "/* synthesized: releases {name}'s storage -- called only from\n * \
oz_release, once the refcount reaches zero. There is no slab here,\n * \
so a heap is the only place an instance can go back to (not from\n * \
source) */\n",
            name = name
        );
    }
    format!(
        "/* synthesized: returns {name}'s slot to its slab -- called only from\n * \
oz_release, once the refcount reaches zero (not from source) */\n",
        name = name
    )
}

/// The `oz_slab_free` line every `{name}_oz_free` ends with, or a comment
/// where there is no slab to return a slot to.
///
/// A slab-less class still needs `{name}_oz_free`: `oz_release`'s
/// class_id switch calls it for every class in the program. What it must
/// not do is name `oz_slab_{name}`, which no longer exists. Anything that
/// reaches here came from a heap and was already returned by
/// `render_heap_free_check` above it; without heap support, nothing can
/// reach it at all.
fn render_slab_free(name: &str, slots: usize) -> String {
    if slots == 0 {
        return "\t/* No slab, so no slot to return. With heap support on, the check\n\
                \t * above has already given a heap-allocated instance back to its\n\
                \t * heap; without it, nothing in this program can have allocated\n\
                \t * one at all. */\n\
                \t(void)obj;\n"
            .to_string();
    }
    format!("\toz_slab_free(&oz_slab_{name}, (void *)obj);\n", name = name)
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
/// This is oz2c's equivalent of the oracle's auto-dealloc
/// (`emit.py::_emit_auto_dealloc`), but deliberately *not* a translation of
/// it. The oracle appends these releases to a user-written `-dealloc` as
/// well, so a class whose `-dealloc` releases its own ivars -- ordinary
/// manual-retain/release teardown -- gets each one released twice. Real ARC
/// avoids that by making `[_ivar release]` in `-dealloc` a compile error
/// rather than by adding a second release, and that is the rule followed
/// here: the release is automatic, and an explicit one is rejected
/// (`staticbar::check_manual_memory_sends`, #428 -- which refuses a
/// `-release` send wherever it appears, not only inside `-dealloc`).
///
/// Lives in the owning class's own file because the companion header only
/// forward-declares non-root structs and so cannot reach an ivar through
/// one.
fn render_release_ivars(name: &str, root: &str, owned: &[(String, Option<String>)]) -> String {
    if owned.is_empty() {
        return String::new();
    }
    let mut c = format!(
        "/* synthesized: releases the {} object ivar(s) a {} owns -- called from\n * oz_release once this class's -dealloc has run (not from source) */\n",
        owned.len(),
        name
    );
    c.push_str(&format!(
        "void {name}_oz_release_ivars(struct {name} *self)\n{{\n",
        name = name
    ));
    for (path, extent) in owned {
        match extent {
            // An array of owned objects releases element by element. The
            // count comes from `sizeof`, not from the recorded extent text:
            // `_leaves[SLOTS]` is as valid as `_leaves[2]` and nothing here
            // can evaluate the former, while the C compiler evaluates both.
            //
            // Casting the array itself -- which is what this did before
            // #287 -- releases the storage as though the first element's
            // pointer were an object header, so the refcount is read out of
            // a pointer value. That is corruption, not a leak, and it
            // compiled silently.
            Some(_) => c.push_str(&format!(
                "\tfor (unsigned int i = 0;\n\
                 \t     i < sizeof(self->{path}) / sizeof(self->{path}[0]);\n\
                 \t     i++) {{\n\
                 \t\toz_release((struct {root} *)self->{path}[i]);\n\
                 \t}}\n",
                root = root,
                path = path
            )),
            None => c.push_str(&format!(
                "\toz_release((struct {root} *)self->{path});\n",
                root = root,
                path = path
            )),
        }
    }
    c.push_str("}\n\n");
    c
}

/// `{name}_oz_dynamic_alloc_with_heap`, backing `[Cls dynamicAllocWithHeap:h]`: the same
/// initialization as `{name}_oz_alloc`, but the storage comes from an
/// `OZHeap` (or the system heap when the argument is nil) instead of the
/// class's slab, and the object is marked so `{name}_oz_free` knows to
/// return it there.
///
/// `oz_heap_alloc` is declared by the PAL and defined in the companion
/// (see `render_heap_bridge`) -- it needs `struct OZHeap` complete, which
/// only generated code has. That split is why it carries `oz2c_` rather
/// than `oz_heap_`: the prefix says which layer *defines* a name, and under
/// `oz_heap_` this pair was an anagram of the PAL pair it calls --
/// `oz_heap_obj_alloc` calling `oz_heap_alloc_obj` (#417).
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

/// `_meta.immortal = 1` for a class whose instances are never deallocated.
///
/// Set for a class conforming to `OZSingletonProtocol`, whose own header states
/// the contract outright: "Singleton objects are immortal -- they are never
/// deallocated." Until #228 nothing marked them, so they relied on nobody ever
/// releasing one; `oz_release` now returns on this bit before it
/// decrements, so an accidental release is a no-op rather than handing the
/// singleton's slab slot back for reuse while every holder keeps pointing at
/// it.
///
/// Emitted in the allocator rather than in `+initialize`, because that is the
/// one place every instance passes through -- both the slab path and the heap
/// path. A singleton class allocating a second instance would mark that one
/// immortal too, which is the conservative direction: the leak is bounded by
/// the class's slab, whereas freeing a live singleton is memory corruption.
fn render_immortal_marker(root: &str, immortal: bool) -> String {
    if !immortal {
        return String::new();
    }
    format!(
        "\t/* conforms to OZSingletonProtocol: immortal, so release never frees it */\n\
         \t((struct {root} *)obj)->_meta.immortal = 1;\n",
        root = root
    )
}

fn render_heap_alloc(name: &str, root: &str, heap_support: bool, immortal: bool) -> String {
    if !heap_support {
        return String::new();
    }
    format!(
        "/* synthesized: allocates a new {name} from an OZHeap rather than its slab --\n * \
backs '[{name} dynamicAllocWithHeap:h]' (not from source) */\n\
         #ifdef OZ_HEAP_SUPPORT\n\
         struct {name} *{name}_oz_dynamic_alloc_with_heap(struct {root} *heap_obj)\n{{\n\
         \tstruct {name} *obj = (struct {name} *)oz_heap_alloc(\n\
         \t\t(struct OZHeap *)heap_obj, sizeof(struct {name}));\n\
         \tif (!obj) {{\n\
         {trap}\
         \t\treturn (struct {name} *)0;\n\t}}\n\
         \tmemset(obj, 0, sizeof(struct {name}));\n\
         \t((struct {root} *)obj)->_meta.class_id = OZ_CLASS_{name};\n\
         \t((struct {root} *)obj)->_meta.heap_allocated = 1;\n\
         {immortal}\
         \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n\
         \treturn obj;\n}}\n\
         #endif\n\n",
        name = name,
        root = root,
        trap = render_heap_exhaustion_trap(name),
        immortal = render_immortal_marker(root, immortal)
    )
}

/// Freed-slot poisoning, the second half of #452: what `{name}_oz_free`
/// writes over an object before it gives the storage back.
///
/// **Ordering matters twice.** It goes before `render_heap_free_check`,
/// which `return`s for a heap-allocated instance -- after it, every heap
/// object would escape unpoisoned. And it goes after the collection
/// emitters' element-release loops, which still read `obj->_count` and
/// `obj->_items`.
///
/// Three stores and a `memset`, and they are not equally useful. This is
/// worth stating precisely, because the obvious reading of "stamp a
/// reserved class id so the over-release trap can name the class" does not
/// survive contact with either allocator:
///
///   * **The body poison is the part that lasts.** `k_mem_slab_free` links
///     a freed block into its free list by writing the next pointer *into
///     the block* (`kernel/mem_slab.c`: `*(char **) mem = slab->free_list`),
///     and host `free()` lets malloc do the same with its own bookkeeping.
///     Both write the *first* word, which is exactly where `_meta` sits --
///     `_meta` is the root struct's first member, so `class_id` is at
///     offset 0. On a slab target everything past the root prefix is
///     untouched, so `0xA5` there is legible for as long as the slot stays
///     free, and a use-after-free that reads an ivar gets an obviously
///     wrong value instead of a plausible stale one. On the host it is not
///     even that: measured on arm64 macOS, a 12-byte block came back
///     `05 00 00 00 00 00 00 00 05 00 00 00` -- malloc's bookkeeping had
///     taken the body too. AddressSanitizer is the host answer and the
///     corpora already run under it.
///   * **`oz_refcount = OZ_REFCOUNT_FREED` is the marker that survives**,
///     and the reason `oz_retain` and `oz_release` can name a
///     use-after-free on target at all (#490). It sits at exactly
///     `sizeof(char *)` -- offset 4 on mps2/an385, offset 8 on
///     qemu_cortex_a53 -- which is the first word the free-list link
///     cannot reach, and structurally so rather than by luck: see the
///     comment on `OZ_REFCOUNT_FREED` in the companion header.
///
///     It used to be `0`, on the reasoning that a zero refcount is what
///     makes the over-release trap deterministic. Measured on target, that
///     reasoning had the wrong subject. `0` is also what a live immortal
///     object and a mid-teardown one hold, so the trap could not tell a
///     freed slot from either -- and worse, it never ran: `oz_release`
///     returned at `_meta.immortal` *above* the trap, and on mps2/an385
///     bit 12 of the free-list link (`0x2000324c`) read back 1, so a
///     release of a freed object was **silent**. On qemu_cortex_a53 the
///     same bit of `0x40062978` read back 0, so there it would have
///     reached the trap. The bits are the measurement; which board aborts
///     follows from them. One fixture, two verdicts, decided by an
///     address. The sentinel check now runs above the immortal check,
///     where no clobbered bit can route around it.
///   * **The `class_id` stamp is legible only before the slot goes back.**
///     That is a narrow window -- the dealloc switch has already read the
///     id by then -- plus the slab path where a thread is waiting and gets
///     the block handed to it directly with no free-list write, and a
///     slab-less class, whose `_oz_free` returns the storage nowhere.
///     It is three cycles behind a debug flag, so it stays, and
///     `oz_class_name` renders it as "freed" in those cases. What it is
///     *not* is a fix for the `?` in the over-release message -- measured,
///     it reads back as the low bits of the free-list link (588 on
///     mps2/an385, 376 on qemu_cortex_a53) and `oz_class_name` answers
///     `?`. The class of a freed object is not recoverable; its *address*
///     is, which is what the freed trap prints instead.
///
/// `immortal` is cleared for completeness rather than need: no immortal
/// object reaches here today, since `oz_release` returns on that bit before
/// the decrement. It costs one store under a flag and removes a way for a
/// future caller to make a poisoned slot invisible.
///
/// A quarantine would make all of this legible, and #452 rejected it: a
/// slot held back is a slot the next allocation cannot have, and
/// `pools.rs` counts one slot per site. Poison costs no slots.
fn render_freed_poison(name: &str, root: &str) -> String {
    /* The root class has no body past its own prefix, so there is nothing
     * to memset -- and `name == root` makes the length expression a
     * literal zero, which is noise in the output rather than a store. */
    let body_poison = if name == root {
        String::new()
    } else {
        format!(
            "\tmemset((char *)obj + sizeof(struct {root}), 0xA5,\n\
             \t       sizeof(struct {name}) - sizeof(struct {root}));\n",
            name = name,
            root = root
        )
    };
    format!(
        "#ifdef OZ_DEBUG_REFCOUNT\n\
         \t/* Poison the slot on the way out (#452, #490). Read the comment\n\
         \t * on `render_freed_poison` before trusting any of this to be\n\
         \t * legible afterwards: both allocators write their free-list\n\
         \t * link over `_meta`, so the class_id stamp is gone the moment\n\
         \t * the slot goes back. The refcount sentinel is the one that\n\
         \t * survives -- it sits at sizeof(char *), just past the link --\n\
         \t * and oz_retain/oz_release check for it first. */\n\
         \t((struct {root} *)obj)->_meta.class_id = OZ_CLASS_ID_FREED;\n\
         \t((struct {root} *)obj)->_meta.immortal = 0;\n\
         \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, OZ_REFCOUNT_FREED);\n\
         {body_poison}\
         #endif\n",
        root = root,
        body_poison = body_poison
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
         \t\toz_heap_free((void *)obj);\n\
         \t\treturn;\n\t}}\n\
         #endif\n",
        root = root
    )
}

/// `oz_heap_alloc`/`oz_heap_free`, which
/// `platform/oz_platform_{zephyr,host}.h` declare and deliberately leave to
/// generated code: both need `struct OZHeap` to be a complete type, and the
/// PAL cannot see it. Same division as the oracle's `oz_dispatch.c.j2`.
///
/// **Two shapes, because a program can want a heap without wanting an
/// `OZHeap`.** The named-heap arms call `OZHeap_oz_inner`, which
/// `render_heap_inner_accessor` emits only into `OZHeap`'s own file -- so a
/// program that enables heap support and never declares `OZHeap` referenced
/// an undeclared function and failed to compile, on
/// `-Wimplicit-function-declaration` and `-Wint-conversion`, in generated
/// code rather than in anything the author wrote.
///
/// That was reachable before `+dynamicAlloc` existed (`[Cls
/// allocWithHeap:nil]` needed the same flag) but it was obscure, because
/// wanting the system heap meant writing `nil` where an `OZHeap` belonged.
/// `+dynamicAlloc` (#413) makes it the *ordinary* case: the whole point of
/// that selector is allocating without an `OZHeap` anywhere. So when the
/// program has no `OZHeap`, the named-heap arms are dead by construction --
/// `heap` and `hdr->heap` can only ever be null -- and emitting them is
/// emitting a call that cannot resolve.
fn render_heap_bridge(heap_support: bool, has_ozheap: bool) -> String {
    if !heap_support {
        return String::new();
    }
    if !has_ozheap {
        return "/* synthesized: the two heap entry points the PAL declares but leaves to\n * generated code. This program declares no OZHeap, so every allocation\n * comes from the system heap and the named-heap arms would call an\n * accessor that is never generated (not from source) */\n#ifdef OZ_HEAP_SUPPORT\nvoid *oz_heap_alloc(struct OZHeap *heap, size_t size)\n{\n\t(void)heap;\n\treturn oz_sys_heap_alloc(size);\n}\n\nvoid oz_heap_free(void *obj)\n{\n\toz_sys_heap_free(obj);\n}\n#endif\n\n"
            .to_string();
    }
    "/* synthesized: the two heap entry points the PAL declares but leaves to\n * generated code -- both need 'struct OZHeap' complete, which only this\n * file has (not from source) */\n#ifdef OZ_HEAP_SUPPORT\nvoid *oz_heap_alloc(struct OZHeap *heap, size_t size)\n{\n\tif (heap) {\n\t\treturn oz_heap_alloc_obj(OZHeap_oz_inner(heap), heap, size);\n\t}\n\treturn oz_sys_heap_alloc(size);\n}\n\nvoid oz_heap_free(void *obj)\n{\n\tstruct oz_heap_hdr *hdr = (struct oz_heap_hdr *)\n\t\t((char *)obj - offsetof(struct oz_heap_hdr, obj));\n\tif (hdr->heap) {\n\t\toz_heap_free_obj(OZHeap_oz_inner(hdr->heap), obj);\n\t} else {\n\t\toz_sys_heap_free(obj);\n\t}\n}\n#endif\n\n"
        .to_string()
}

pub(crate) fn render_alloc_free(
    name: &str,
    root: &str,
    slots: usize,
    owned_ivars: &[(String, Option<String>)],
    heap_support: bool,
    immortal: bool,
) -> String {
    let mut c = render_slab_define(name, slots);
    c.push_str(&render_release_ivars(name, root, owned_ivars));
    if slots == 0 {
        c.push_str(&render_no_slab_alloc(name));
    } else {
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
            "\t((struct {root} *)obj)->_meta.class_id = OZ_CLASS_{name};\n\
             {immortal}\
             \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n",
            root = root,
            name = name,
            immortal = render_immortal_marker(root, immortal)
        ));
        c.push_str("\treturn obj;\n}\n\n");
    }
    c.push_str(&render_heap_alloc(name, root, heap_support, immortal));
    c.push_str(&render_heap_inner_accessor(name, heap_support));
    c.push_str(&render_free_banner(name, slots));
    c.push_str(&format!(
        "void {name}_oz_free(struct {name} *obj)\n{{\n\
         {poison}\
         {heap_check}\
         {slab_free}}}\n\n",
        name = name,
        poison = render_freed_poison(name, root),
        heap_check = render_heap_free_check(root, heap_support),
        slab_free = render_slab_free(name, slots)
    ));
    c
}

/// OZArray-specific replacement for `render_alloc_free`: alloc is
/// identical, but free must also release every held item and give the
/// items buffer back -- OZArray.m (transplanted verbatim from
/// `src/OZArray.m`) has no `-dealloc` of its own, so the generic dealloc
/// dispatch would otherwise fall through to OZObject's no-op `-dealloc`
/// and leak both. The oracle synthesizes the same thing at emit-time.
/// Also emits `OZArray_oz_initWithItems`, the builder backing the
/// `@[...]` boxed array literal desugar in `emit.rs`; its buffer comes
/// from the shared `oz_item_pool` (see `render_item_buffer_alloc`), not
/// from malloc.
pub(crate) fn render_array_support(
    name: &str,
    root: &str,
    slots: usize,
    owned_ivars: &[(String, Option<String>)],
    heap_support: bool,
    item_slots: usize,
) -> String {
    let mut c = render_slab_define(name, slots);
    c.push_str(&render_release_ivars(name, root, owned_ivars));
    if slots == 0 {
        c.push_str(&render_no_slab_alloc(name));
    } else {
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
            "\t((struct {root} *)obj)->_meta.class_id = OZ_CLASS_{name};\n\
             \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n",
            root = root,
            name = name
        ));
        c.push_str("\treturn obj;\n}\n\n");
    }

    /* OZArray/OZDictionary are Foundation collections, never singletons. */
    c.push_str(&render_heap_alloc(name, root, heap_support, false));
    c.push_str(&format!(
        "/* synthesized: releases {name}'s items, its items buffer, and its own\n * \
storage -- called only from oz_release, once the refcount reaches\n * \
zero (not from source; OZArray.m has no -dealloc of its own) */\n",
        name = name
    ));
    c.push_str(&format!(
        "void {name}_oz_free(struct {name} *obj)\n{{\n\
         \tfor (unsigned int i = 0; i < obj->_count; i++) {{\n\
         \t\toz_release((struct {root} *)obj->_items[i]);\n\
         \t}}\n\
         {items_free}\
         {poison}\
         {heap_check}\
         {slab_free}\
         }}\n\n",
        root = root,
        name = name,
        items_free = render_item_buffer_free("_items", "obj->_count", item_slots),
        poison = render_freed_poison(name, root),
        heap_check = render_heap_free_check(root, heap_support),
        slab_free = render_slab_free(name, slots)
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
         \tvoid **items;\n\
         {items_alloc}\
         \tfor (unsigned int i = 0; i < count; i++) {{\n\t\titems[i] = src[i];\n\t}}\n\
         \tarr->_items = items;\n\
         \tarr->_count = count;\n\
         \treturn arr;\n\
         }}\n\n",
        name = name,
        items_alloc = render_item_buffer_alloc(name, "items", "count", item_slots)
    ));
    c
}

/// Take a run of `count` element slots from the shared item pool, or fall
/// back to `malloc` when there is no pool.
///
/// The fallback is not a second allocator to maintain: `item_slots` is
/// zero only when the source contains no `@[...]`/`@{...}` at all, in
/// which case nothing calls the builder this lands in and the branch is
/// dead. It exists so the emitted C still compiles in that case, since
/// the builder itself is emitted unconditionally (it has a prototype in
/// the shared header, which every translation unit sees).
///
/// On failure the just-allocated collection object is handed back to its
/// slab and the builder returns NULL, which is what the oracle does
/// (`templates/class_header.h.j2`) -- a caller already has to handle a
/// null return from slab exhaustion, so pool exhaustion needs no new
/// contract.
fn render_item_buffer_alloc(name: &str, var: &str, count: &str, item_slots: usize) -> String {
    if item_slots == 0 {
        return format!(
            "\t{var} = malloc({count} * sizeof(void *));\n\
             \tif (!{var}) {{\n\t\t{name}_oz_free({obj});\n\t\treturn (struct {name} *)0;\n\t}}\n",
            var = var,
            count = count,
            name = name,
            obj = if name == "OZArray" { "arr" } else { "dict" }
        );
    }
    format!(
        "\tif (oz_mem_blocks_alloc_contiguous(&oz_item_pool, {count},\n\
         \t\t\t\t\t   (void **)&{var}) != 0) {{\n\
         \t\t{name}_oz_free({obj});\n\
         \t\treturn (struct {name} *)0;\n\t}}\n",
        count = count,
        var = var,
        name = name,
        obj = if name == "OZArray" { "arr" } else { "dict" }
    )
}

/// Give a collection's element buffer back, mirroring
/// `render_item_buffer_alloc`.
///
/// Guarded on the pointer being non-null, unlike the `free()` it replaces:
/// `free(NULL)` is defined to do nothing, but handing a null pointer to
/// `sys_mem_blocks_free_contiguous` is not, and a collection whose
/// builder failed (or which was never given a buffer) reaches here with
/// `_items`/`_keys` still zeroed by `_oz_alloc`'s memset.
fn render_item_buffer_free(field: &str, count: &str, item_slots: usize) -> String {
    if item_slots == 0 {
        return format!("\tfree(obj->{field});\n", field = field);
    }
    format!(
        "\tif (obj->{field}) {{\n\
         \t\toz_mem_blocks_free_contiguous(&oz_item_pool,\n\
         \t\t\t\t\t      obj->{field}, {count});\n\
         \t}}\n",
        field = field,
        count = count
    )
}

/// OZDictionary-specific replacement for `render_alloc_free`, the same
/// shape as `render_array_support`: alloc is identical, but free must
/// also release every key and value and free their buffer -- OZDictionary.m
/// (transplanted from `src/OZDictionary.m`) has no `-dealloc` of its own,
/// same reason as OZArray. Also emits `OZDictionary_oz_initWithKeysValues`,
/// the builder backing the `@{...}` boxed dictionary literal desugar in
/// `emit.rs`. Keys and values share one contiguous run of `2 * count`
/// slots (`_keys` pointing at its first half, `_values` at its second),
/// taken from the shared `oz_item_pool` -- the same shape, and now the
/// same allocator, as the oracle's own `{Name}_initWithKeysValues`
/// template (`tools/oz_transpile/templates/class_header.h.j2`).
pub(crate) fn render_dict_support(
    name: &str,
    root: &str,
    slots: usize,
    owned_ivars: &[(String, Option<String>)],
    heap_support: bool,
    item_slots: usize,
) -> String {
    let mut c = render_slab_define(name, slots);
    c.push_str(&render_release_ivars(name, root, owned_ivars));
    if slots == 0 {
        c.push_str(&render_no_slab_alloc(name));
    } else {
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
            "\t((struct {root} *)obj)->_meta.class_id = OZ_CLASS_{name};\n\
             \toz_atomic_init(&((struct {root} *)obj)->oz_refcount, 1);\n",
            root = root,
            name = name
        ));
        c.push_str("\treturn obj;\n}\n\n");
    }

    /* OZArray/OZDictionary are Foundation collections, never singletons. */
    c.push_str(&render_heap_alloc(name, root, heap_support, false));
    c.push_str(&format!(
        "/* synthesized: releases {name}'s keys, its values, their shared\n * \
buffer, and its own storage -- called only from oz_release, once\n * \
the refcount reaches zero (not from source; OZDictionary.m has no\n * \
-dealloc of its own) */\n",
        name = name
    ));
    c.push_str(&format!(
        "void {name}_oz_free(struct {name} *obj)\n{{\n\
         \tfor (unsigned int i = 0; i < obj->_count; i++) {{\n\
         \t\toz_release((struct {root} *)obj->_keys[i]);\n\
         \t\toz_release((struct {root} *)obj->_values[i]);\n\
         \t}}\n\
         {keys_free}\
         {poison}\
         {heap_check}\
         {slab_free}\
         }}\n\n",
        root = root,
        name = name,
        keys_free = render_item_buffer_free("_keys", "obj->_count * 2", item_slots),
        poison = render_freed_poison(name, root),
        heap_check = render_heap_free_check(root, heap_support),
        slab_free = render_slab_free(name, slots)
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
         \tvoid **buf;\n\
         {buf_alloc}\
         \tfor (unsigned int i = 0; i < count; i++) {{\n\
         \t\tbuf[i] = keys[i];\n\
         \t\tbuf[count + i] = values[i];\n\
         \t}}\n\
         \tdict->_keys = buf;\n\
         \tdict->_values = buf + count;\n\
         \tdict->_count = count;\n\
         \treturn dict;\n\
         }}\n\n",
        name = name,
        buf_alloc = render_item_buffer_alloc(name, "buf", "count * 2", item_slots)
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
        c.push_str(&format!("{} {}({})\n{{\n", ret_ty, fn_name, params));
        /* The dispatcher reads the receiver's class id to route, so it
         * dereferences `self` *before* any method body is reached -- the
         * prologue guard `emit` puts in each method cannot help here, and
         * a dispatched send to nil crashed in this switch (#528).
         *
         * Enumerated rather than assumed: 9 of the 12 pointer-parameter
         * dereferences in a reflection-enabled companion were unguarded,
         * and every one of them was an `OZ_PROTOCOL_SEND_*`.
         * `oz_retain`, `oz_release` and `oz_class_name` already guarded,
         * so the pattern was in the tree and the dispatchers were simply
         * missed. */
        if !program.nil_sends_unchecked {
            c.push_str(&format!(
                "\tif (!self) {{\n\t\t{}\n\t}}\n",
                crate::emit::nil_send_return(&ret_ty)
            ));
        }
        c.push_str("\tswitch (self->_meta.class_id) {\n");
        for (name, defining) in &routed {
            let target = crate::emit::method_fn_name(defining, &m.selector, m.is_class_method);
            let mut call_args = vec![format!("(struct {} *)self", defining)];
            call_args.extend(arg_names.iter().map(|a| a.to_string()));
            let call = format!("{}({})", target, call_args.join(", "));
            if m.return_type == "void" {
                c.push_str(&format!("\tcase OZ_CLASS_{}: {}; return;\n", name, call));
            } else {
                c.push_str(&format!("\tcase OZ_CLASS_{}: return {};\n", name, call));
            }
        }
        /* One spelling of "zero of this return type", shared with the
         * guard above and with `emit`'s method prologue. This arm used to
         * write `({ret_ty})0` inline, which is not a conversion C allows
         * to a struct type. */
        c.push_str(&format!("\tdefault: {}\n\t}}\n}}\n\n", crate::emit::nil_send_return(&ret_ty)));
    }
    (h, c)
}

fn class_label(program: &Program, name: &str, id: usize) -> String {
    match &program.classes[name].superclass {
        Some(sup) => format!("-- {} (id {}, extends {}) --", name, id, sup),
        None => format!("-- {} (id {}, root) --", name, id),
    }
}

/// The introspection tables and the helpers that read them, for exactly
/// the constructs the emitted code referenced (`emit::IntrospectionUse`).
///
/// Everything here is `const`, so it lands in flash and costs no RAM at
/// all, and nothing is emitted for a construct no call site used -- a
/// program that never introspects pays nothing even with
/// `CONFIG_OBJZ_INTROSPECTION=y`.
///
/// The helpers are deliberately *not* `static inline`. Measured on
/// Cortex-M3 at `-Os`, inlining `oz_is_kind_of` costs 40 bytes at every
/// call site against 20 for a call to one 32-byte copy, so inlining is
/// only cheaper for the first two or three sites and grows without bound
/// after that. `oz_class_of` stays inline in the preamble because it is a
/// single bitfield read.
///
/// `oz_superclass_of` is indexed by `class_id` and holds each class's
/// superclass id, `Nil` terminating the chain -- the same relation
/// `Program::is_descendant_of` walks over `ClassInfo::superclass`, moved
/// to run time because `-isKindOfClass:` asks about the receiver's
/// *actual* class, which a declared type is only an upper bound on.
/// The per-selector records `@selector(...)` resolves to, their
/// uniform-shape wrappers, and the two helpers that read them.
///
/// Emitted for exactly the selectors some `@selector(...)` named
/// (`Program::reflected_selectors`), so a program that never writes one
/// pays nothing even with `CONFIG_OBJZ_REFLECTION=y`. Within that, the
/// `responds` bitmap appears only if the program asks about responding and
/// the `perform` wrapper only if it performs -- the two halves are
/// independent, and either alone is a real pattern.
///
/// A bit in `responds` is set for a class whose lookup for this selector
/// both resolves (`find_defining_method`, the same single-inheritance walk
/// the dispatch tables use) and lands on something that will exist
/// (`Program::method_is_defined`). The second half matters: a selector
/// declared in an `@interface` and never given a body is not callable, so
/// reporting YES for it would promise a call that fails at link time.
fn render_reflection(program: &Program, root: &str) -> (String, String) {
    let mut h = String::new();
    let mut c = String::new();
    // Not keyed on `reflected_selectors` alone: a `SEL` is a value, so a
    // program can send `-respondsToSelector:` or `-performSelector:` with
    // one that came from a parameter, an ivar or a cast and never write a
    // `@selector(...)` at all. Returning early on an empty record set left
    // the helpers those sends call undeclared, which surfaced as an
    // implicit-declaration error rather than anything located.
    if program.reflected_selectors.is_empty()
        && !program.uses_responds_to_selector
        && !program.uses_perform_selector
    {
        return (h, c);
    }
    let words = program.class_order.len().div_ceil(32).max(1);

    for selector in &program.reflected_selectors {
        let selc = crate::emit::selector_to_c(selector);
        let sig = program.class_order.iter().find_map(|name| {
            program.classes[name]
                .methods
                .iter()
                .find(|m| &m.selector == selector && !m.is_class_method)
        });

        let responds_name = if program.uses_responds_to_selector {
            let mut bits = vec![0u32; words];
            let mut implementors: Vec<&str> = Vec::new();
            for name in &program.class_order {
                let resolves = find_defining_method(program, name, selector, false)
                    .is_some_and(|defining| {
                        program.method_is_defined(&defining, selector, false)
                    });
                if resolves {
                    if let Some(id) = program.class_id(name) {
                        bits[id / 32] |= 1u32 << (id % 32);
                    }
                    implementors.push(name.as_str());
                }
            }
            let words_text =
                bits.iter().map(|w| format!("0x{:08x}u", w)).collect::<Vec<_>>().join(", ");
            c.push_str(&format!(
                "/* classes responding to '{}', one bit per class_id: {} */\nstatic const uint32_t oz_responds_{}[{}] = {{ {} }};\n\n",
                selector,
                if implementors.is_empty() { "none".to_string() } else { implementors.join(", ") },
                selc,
                words,
                words_text
            ));
            format!("oz_responds_{}", selc)
        } else {
            "((void *)0)".to_string()
        };

        let perform_name = if program.needs_perform_wrapper(selector) {
            let m = sig.expect("a reflected selector with no implementor is refused in emit");
            let arg_names: Vec<String> = m
                .params
                .iter()
                .enumerate()
                .map(|(i, _)| format!("a{}", i))
                .collect();
            let mut call_args = vec![format!("(struct {} *)self", root)];
            call_args.extend(arg_names.iter().cloned());
            let call = format!(
                "OZ_PROTOCOL_SEND_{}({})",
                selc,
                call_args.join(", ")
            );
            // `void` methods have nothing to hand back, and real
            // Objective-C hands back a garbage `id` for them. NULL is the
            // honest answer, and the wrapper is where it belongs -- the
            // call itself stays properly typed.
            let body = if m.return_type == "void" {
                format!("\t{};\n\treturn ((void *)0);\n", call)
            } else {
                format!("\treturn (void *)({});\n", call)
            };
            let unused: String = ["a0", "a1"]
                .iter()
                .filter(|a| !arg_names.iter().any(|n| n == *a))
                .map(|a| format!("\t(void){};\n", a))
                .collect();
            c.push_str(&format!(
                "/* uniform-shape wrapper for '{}', so a SEL can be called\n * without a cast (not from source) */\nstatic void *oz_perform_{}(void *self, void *a0, void *a1)\n{{\n{}{}}}\n\n",
                selector, selc, unused, body
            ));
            format!("oz_perform_{}", selc)
        } else {
            "((oz_imp_t)0)".to_string()
        };

        let arity = sig.map(|m| m.params.len()).unwrap_or(0);
        c.push_str(&format!(
            "/* the selector '{}' -- what `@selector({})` resolves to */\nconst struct oz_selector oz_sel_{} = {{ {}, {}, {} }};\n\n",
            selector, selector, selc, perform_name, responds_name, arity
        ));
        h.push_str(&format!("extern const struct oz_selector oz_sel_{};\n", selc));
    }

    if program.uses_responds_to_selector {
        c.push_str(
            "/* does class `k` implement the selector this record describes?\n \
* A null SEL answers NO rather than dereferencing: `SEL` is a plain\n \
* pointer, so nothing stops a caller passing 0, and the emitted C is\n \
* held to having no undefined behaviour in it (not from source) */\n\
BOOL oz_responds(SEL sel, Class k)\n{\n\
\treturn sel != ((void *)0) && k != Nil && sel->responds != ((void *)0) &&\n\
\t       (sel->responds[k >> 5] & (1u << (k & 31))) != 0;\n}\n\n",
        );
        h.push_str("BOOL oz_responds(SEL sel, Class k);\n");
    }
    if program.uses_perform_selector {
        c.push_str(
            "/* send `sel` to `obj`, or nothing at all if `obj` is nil --\n \
* the same answer Objective-C gives. A null SEL, or one whose selector\n \
* this program never performs, likewise yields nil instead of calling\n \
* through a null pointer (not from source) */\n\
void *oz_perform(SEL sel, void *obj, void *a0, void *a1)\n{\n\
\tif (obj == ((void *)0) || sel == ((void *)0) || sel->perform == ((oz_imp_t)0)) {\n\
\t\treturn ((void *)0);\n\t}\n\
\treturn sel->perform(obj, a0, a1);\n}\n\n",
        );
        h.push_str("void *oz_perform(SEL sel, void *obj, void *a0, void *a1);\n");
    }
    if !h.is_empty() {
        h.insert_str(0, "/* reflection support -- see `companion::render_reflection` */\n");
        h.push('\n');
    }
    (h, c)
}

fn render_introspection(
    program: &Program,
    used: &crate::emit::IntrospectionUse,
) -> (String, String) {
    let mut h = String::new();
    let mut c = String::new();
    if used.is_empty() {
        return (h, c);
    }

    let n_classes = program.class_order.len();
    let words = n_classes.div_ceil(32).max(1);

    if used.kind_of {
        let mut ids: Vec<(usize, String)> = program
            .class_order
            .iter()
            .filter_map(|name| program.class_id(name).map(|id| (id, name.clone())))
            .collect();
        ids.sort_by_key(|(id, _)| *id);
        let mut rows = String::new();
        for (id, name) in &ids {
            let sup = program.classes[name]
                .superclass
                .as_ref()
                .and_then(|s| program.class_id(s))
                .map(|i| i.to_string())
                .unwrap_or_else(|| "Nil".to_string());
            rows.push_str(&format!("\t{}, /* {} ({}) */\n", sup, name, id));
        }
        c.push_str(&format!(
            "/* each class's superclass id, indexed by class_id; Nil ends the chain */\nstatic const Class oz_superclass_of[{}] = {{\n{}}};\n\n/* is `k`, or any class up its chain, `ancestor`? (not from source) */\nBOOL oz_is_kind_of(Class k, Class ancestor)\n{{\n\twhile (k != Nil) {{\n\t\tif (k == ancestor) {{\n\t\t\treturn true;\n\t\t}}\n\t\tk = oz_superclass_of[k];\n\t}}\n\treturn false;\n}}\n\n",
            ids.len().max(1),
            rows
        ));
        h.push_str("BOOL oz_is_kind_of(Class k, Class ancestor);\n");
    }

    if !used.protocols.is_empty() {
        for proto in &used.protocols {
            let mut bits = vec![0u32; words];
            for name in &program.class_order {
                if program.class_conforms_to(name, proto) {
                    if let Some(id) = program.class_id(name) {
                        bits[id / 32] |= 1u32 << (id % 32);
                    }
                }
            }
            let conformers: Vec<&str> = program
                .class_order
                .iter()
                .filter(|n| program.class_conforms_to(n, proto))
                .map(|n| n.as_str())
                .collect();
            let words_text = bits
                .iter()
                .map(|w| format!("0x{:08x}u", w))
                .collect::<Vec<_>>()
                .join(", ");
            c.push_str(&format!(
                "/* classes conforming to '{}', one bit per class_id: {} */\nconst uint32_t oz_proto_{}[{}] = {{ {} }};\n\n",
                proto,
                if conformers.is_empty() {
                    "none".to_string()
                } else {
                    conformers.join(", ")
                },
                proto,
                words,
                words_text
            ));
            h.push_str(&format!("extern const uint32_t oz_proto_{}[{}];\n", proto, words));
        }
        c.push_str(
            "/* does class `k` conform to the protocol this bitmap describes?\n * (not from source) */\nBOOL oz_conforms(Class k, const uint32_t *proto)\n{\n\treturn k != Nil && (proto[k >> 5] & (1u << (k & 31))) != 0;\n}\n\n",
        );
        h.push_str("BOOL oz_conforms(Class k, const uint32_t *proto);\n");
    }
    if !h.is_empty() {
        h.insert_str(0, "/* introspection support -- see `companion::render_introspection` */\n");
        h.push('\n');
    }
    (h, c)
}

/// `oz_check_all_slabs()` -- the exit-time live-object census (#451).
///
/// One `oz_slab_check_leaks` call per class that reserves a slab, returning
/// the number of classes whose slab still holds an outstanding allocation
/// and naming each on stderr (host) or the console (Zephyr). Nothing in the
/// tree used to answer "was every object freed?": `oz_slab_check_leaks` had
/// existed in the host PAL since the beginning with **zero** call sites,
/// and the corpus's oracle was a one-block-slab exhaustion proxy -- alloc,
/// release, alloc again, assert non-NULL -- which is blind three ways. It
/// needs the pool sized to exactly 1 (the harness defaults every class to
/// 4, so three leaks fit in the spare slots unnoticed), it cannot see a
/// leak in any *other* class, and an over-release makes the slab *more*
/// available, so freeing a block twice still passes.
///
/// Why this rather than more LeakSanitizer, which is the other obvious
/// answer and already runs in one CI job:
///
///   * LSan reports *unreachable* blocks. An object still held by a
///     file-scope `static Thing *g`, by a static-storage ivar chain, or by
///     the slab bookkeeping itself is reachable from a root, and LSan is
///     silent. This counts allocations against frees, so reachability does
///     not enter into it.
///   * `-fsanitize=leak` does not exist on arm64 macOS, so that gate is
///     unreachable on a maintainer's machine. This needs no sanitizer.
///   * It runs at every compiler and `-O` cell, not just the one gcc/-O0
///     job, and on target -- where `k_mem_slab` is static memory and an
///     unreleased object is otherwise invisible by construction.
///
/// Two exclusions, both by construction rather than by heuristic:
///
///   * a class with `slots == 0` reserves no slab at all
///     (`render_slab_define`), so there is no counter to read and no
///     `oz_slab_{name}` symbol to name;
///   * a class conforming to `OZSingletonProtocol`, because
///     `render_immortal_marker` marks *every* instance of such a class
///     immortal at alloc and `oz_release` returns before the decrement --
///     the slot is held for the life of the program by design. Immortality
///     is a per-class property here, which is what makes a static
///     exclusion exact rather than approximate; counting these would
///     report px-keyboard's four singletons as four leaks. The excluded
///     classes are still *named* in the emitted comment, so a reader can
///     see what was skipped and why.
///
/// A heap instance (`[X dynamicAlloc]`) draws from no slab and is outside
/// what this can see; LSan remains the instrument for those. An element
/// buffer from `oz_item_pool` is not counted either, and does not need to
/// be: a buffer is owned by the OZArray/OZDictionary that allocated it, so
/// a leaked buffer implies a leaked object, and the object's slot is
/// counted here.
fn render_leak_census(program: &Program, pools: &crate::pools::PoolSizes) -> (String, String) {
    let with_slab: Vec<&str> = program
        .class_order
        .iter()
        .filter(|name| pools.for_class(name) > 0)
        .map(|name| name.as_str())
        .collect();
    let (immortal, counted): (Vec<&str>, Vec<&str>) = with_slab
        .iter()
        .partition(|name| program.class_conforms_to(name, SINGLETON_PROTOCOL));

    let h = "/* synthesized: the exit-time live-object census -- the number of classes\n * \
whose slab still holds an outstanding allocation, each named on stderr.\n * \
Zero means every slab block this program handed out was handed back.\n * \
Defined in oz2c_dispatch.c (not from source) */\nint oz_check_all_slabs(void);\n\n"
        .to_string();

    let mut c = String::from(
        "/* synthesized: exit-time live-object census (#451). Answers \"was every\n * \
object freed?\" by counting allocations against frees, which -- unlike a\n * \
leak sanitizer -- sees an object that is still reachable from a static\n * \
root, needs no sanitizer to run, and works on target where the slab is\n * \
static memory. A heap instance ('[X dynamicAlloc]') comes from no slab\n * \
and is not counted here (not from source) */\n",
    );
    for name in &counted {
        c.push_str(&format!("extern oz_slab_t oz_slab_{};\n", name));
    }
    c.push_str("int oz_check_all_slabs(void)\n{\n");
    for name in &immortal {
        c.push_str(&format!(
            "\t/* {name} is excluded: it conforms to {proto}, so every instance\n\
             \t * is immortal and keeps its slab slot for the life of the\n\
             \t * program -- by design, not a leak */\n",
            name = name,
            proto = SINGLETON_PROTOCOL
        ));
    }
    if counted.is_empty() {
        c.push_str("\t/* no class in this program reserves a countable slab */\n\treturn 0;\n");
    } else {
        c.push_str("\tint leaked = 0;\n\n");
        for name in &counted {
            c.push_str(&format!(
                "\tleaked += oz_slab_check_leaks(&oz_slab_{name}, \"{name}\");\n",
                name = name
            ));
        }
        c.push_str(
            "\n\tif (leaked > 0) {\n\
             \t\t/* The report is the whole value of this call, and a caller\n\
             \t\t * may exit or abort straight after it. #452 measured stdio\n\
             \t\t * losing a diagnostic that way: `printf` to anything but a\n\
             \t\t * terminal is fully buffered and neither `abort()` nor\n\
             \t\t * `_exit()` flushes it. Flushing every stream also keeps the\n\
             \t\t * report ordered against whatever the program printed on\n\
             \t\t * stdout, which is what a reader is comparing it to. No-op\n\
             \t\t * on Zephyr, where printk has already left. */\n\
             \t\toz_platform_flush();\n\t}\n\treturn leaked;\n",
        );
    }
    c.push_str("}\n\n");
    (h, c)
}

pub fn render(
    program: &Program,
    hoisted_structs: &[(String, String)],
    hoisted_c_types: &[String],
    pools: &crate::pools::PoolSizes,
    system_includes: &[String],
    introspection_used: &crate::emit::IntrospectionUse,
) -> (String, String) {
    let root = program.root_class().map(|s| s.to_string());
    // The root class terminates the dealloc chain `dealloc_chain` walks. If
    // the user didn't write one, synthesize a no-op, so a class with no
    // `-dealloc` anywhere in its chain still has something to call.
    let root_needs_synthetic_dealloc =
        root.as_deref().is_some_and(|r| find_defining_dealloc(program, r).is_none());
    let struct_order = topological_order(program);
    let struct_text: std::collections::HashMap<&str, &str> =
        hoisted_structs.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();

    let mut h = String::new();
    h.push_str("/* Auto-generated by oz2c -- do not edit */\n#pragma once\n\n");
    // The `id`/`Class`/`BOOL` typedefs come first, before any `#include`,
    // because an include here can re-enter the generated headers: the PAL
    // (`platform/oz_assert.h`) includes `assert.h`, which in a split
    // output resolves to oz2c's *own* generated `assert.h` -- itself
    // a translation of the SDK shim -- which pulls in the class headers,
    // whose prototypes name `Class` and `BOOL`. Declared after the
    // includes, those prototypes are reached while this header is still
    // only four lines in, and the build fails with `unknown type name
    // 'Class'`. They depend on nothing but `bool`, so hoisting them above
    // every include is both safe and sufficient.
    h.push_str("#include <stdbool.h>\n#include <stdint.h>\n\n");
    // `id`/`Class`/`BOOL` are real Objective-C built-in types with no
    // plain-C equivalent, so left undefined they'd be invalid C tokens
    // wherever this spike can't translate them (ivar declarations, the
    // inner parameter list of a `(^)`-to-`(*)`-converted block type, a
    // plain top-level C function's own signature) -- `collect::render_type`
    // separately resolves a method's own `id` parameter/return type to
    // `void *`, but that doesn't reach those other spots. Defining all
    // three here, included by both the primary source and this companion,
    // covers every spot at once.
    //
    // `Class` is the `class_id` every object already carries in its
    // `_meta` bitfield (`include/platform/oz_platform_types.h`), not a
    // pointer to a class object: the whole class set is known at
    // transpile time, so `[Foo class]` is the constant
    // `OZ_CLASS_Foo` and `[obj class]` is a bitfield read. That
    // makes a `Class` a real value -- storable, comparable, passable --
    // for no flash and no RAM at all, where a class-object pointer would
    // need a `const` record per class. It used to be `void *`, purely as
    // a placeholder on the assumption that `+ (Class)class` was declared
    // but never called; calling it in fact emitted
    // `OZObject_class_cls()`, which drops the receiver class and is
    // defined nowhere, so it failed at *link* time with an undefined
    // symbol (#226).
    //
    // `class_id` is a 10-bit field, so 0xFFFF can never be a real class
    // and serves as `Nil`. Every reflection helper returns or rejects it
    // rather than dereferencing a null receiver, which is what makes
    // `[nil isKindOfClass:...]` answer NO the way Objective-C does.
    h.push_str(
        "typedef void *id;\ntypedef uint16_t Class;\ntypedef bool BOOL;\n\n\
/* no class; `class_id` is 10 bits wide, so this can never collide.\n \
* Guarded because the SDK header declares the same thing for Clang\'s\n \
* benefit during the AST dump, and a translated header carries it into\n \
* the output alongside this one. */\n\
#ifndef Nil\n\
#define Nil ((Class)0xFFFF)\n\
#endif\n\n\
/* The id `_oz_free` stamps over a slot it is returning (#452). Reserved\n \
* unconditionally, whether or not the stamp is compiled in, because the\n \
* guarantee a reserved id needs is that no class ever takes it -- and ids\n \
* are assigned densely from 0, so the top of the 10-bit range is the one\n \
* value a program would have to declare 1024 classes to reach.\n \
* `oz_class_name` renders it as \"freed\" rather than \"?\", which is the\n \
* difference between \"something was over-released\" and \"something\n \
* already freed was released again\" -- when the stamp is still legible.\n \
* It never is on a slab target: measured on mps2/an385 the id read back\n \
* 588 and on qemu_cortex_a53 376, both the low bits of the free-list\n \
* link. See the comment on the stamp itself, and OZ_REFCOUNT_FREED\n \
* below, which is the marker that does survive. */\n\
#define OZ_CLASS_ID_FREED 1023\n\n\
/* The refcount `_oz_free` stamps over a slot it is returning, and the one\n \
* marker on this backend that outlives the free (#490).\n \
*\n \
* `class_id` cannot: `_meta` is the root struct's first member, so it\n \
* occupies `[0, 4)`, and `k_mem_slab_free` writes its free-list link over\n \
* `[0, sizeof(char *))`. `oz_refcount` begins exactly where that link\n \
* ends, and not by luck: `oz_atomic_t` is Zephyr's `atomic_t`, which is a\n \
* `long`, and `sizeof(long) == sizeof(char *)` on both ILP32 and LP64, so\n \
* the alignment of that `long` rounds its offset up to precisely\n \
* `sizeof(char *)`. Measured: offset 4 on mps2/an385 and offset 8 on\n \
* qemu_cortex_a53, the stamped word intact on both.\n \
*\n \
* Reserved unconditionally, for the same reason as the id above: what a\n \
* reserved value needs is that nothing else ever produces it, which is a\n \
* fact about the numbering rather than about the instruments.\n \
*\n \
* The value is a count no live object can hold. A refcount is bounded by\n \
* the number of live strong references, one pointer each, so reaching\n \
* 0x0FEEDFEE would take 267 million of them -- about a gigabyte of\n \
* pointers on a target whose whole SRAM is measured in kilobytes. Chosen\n \
* positive and inside 31 bits so it converts without surprise to both\n \
* `atomic_val_t` (a `long`) and the host backend's `_Atomic(int)`.\n \
*\n \
* One limit, stated rather than designed away: a live refcount of\n \
* OZ_REFCOUNT_FREED + 1 *decrements into* the sentinel, so the next\n \
* release of that object reports a use-after-free on live storage.\n \
* Reaching it needs the same quarter of a billion references the value is\n \
* chosen to be out of reach of, so the collision is recorded rather than\n \
* avoided -- see `a_refcount_beside_the_sentinel_is_not_a_freed_slot` in\n \
* tools/oz2c/tests/refcount_traps.rs, which is why its above-neighbour is\n \
* two rather than one. */\n\
#define OZ_REFCOUNT_FREED 0x0FEEDFEE\n\n",
    );
    // Replaced, once the whole header is built, by a forward declaration
    // for every struct tag it mentions but never declares -- see
    // `forward_declare_unknown_struct_tags`.
    h.push_str(FORWARD_DECL_MARKER);

    h.push_str("#include \"platform/oz_platform.h\"\n#include <stdlib.h>\n#include <string.h>\n\n");
    // After the PAL include, which is what declares `struct oz_metadata`.
    // Reads the class id through the *metadata* type rather than through
    // the root class's struct, so it needs no class declared yet: `_meta`
    // is the first member of the root struct and every object is a root-
    // struct prefix, so a pointer to any object, suitably converted,
    // points to its initial member (C11 6.7.2.1p15). Left inline because
    // it is one bitfield read -- the helpers that walk a table are
    // emitted out of line instead (see `render_introspection`).
    h.push_str(
        "/* the receiver's class, or Nil for a null receiver */\n\
static inline Class oz_class_of(const void *obj)\n\
{\n\
\treturn obj ? (Class)((const struct oz_metadata *)obj)->class_id : Nil;\n\
}\n\n",
    );
    // `SEL` is a pointer to a `const` record per reflectively-named
    // selector, not a pointer straight at a method or at its dispatch
    // function. Two reasons, both structural.
    //
    // A selector has one implementation per class, so it cannot *be* a
    // method pointer; the nearest single function is the selector's
    // `OZ_PROTOCOL_SEND_*` dispatcher, which already switches on
    // `class_id`. But `-respondsToSelector:` is a predicate, not a call,
    // and given only a function pointer there is no way to ask "does class
    // 7 implement this" -- the record gives that bitmap somewhere to live.
    //
    // And dispatch functions have per-selector signatures, so calling one
    // through a differently-typed pointer is undefined behaviour, which
    // the generated C is held to (`just test-pedantic`). `perform` instead
    // has one uniform shape for every selector, and a generated wrapper
    // adapts the real call to it -- dropping unused arguments, returning
    // NULL for a `void` method. So the indirect call needs no cast, no
    // shape tag and no variadics, unlike the retired legacy runtime's
    // per-architecture assembly trampoline (`src/runtime_legacy/`).
    h.push_str(
        "/* uniform shape every `perform` wrapper below has, so an\n \
* indirect call through a SEL needs no cast */\n\
struct oz_selector;\n\
typedef void *(*oz_imp_t)(void *self, void *a0, void *a1);\n\
struct oz_selector {\n\
\toz_imp_t perform;         /* NULL if this program never performs */\n\
\tconst uint32_t *responds; /* one bit per class_id, NULL if unused */\n\
\tuint8_t arity;            /* object arguments the selector takes */\n\
};\n\
typedef const struct oz_selector *SEL;\n\n",
    );
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
    // `oz_log_precision` is called directly by any class's own
    // `getDescription:maxLength:` (not just through `OZLog()` itself) --
    // same splice-visibility gap as `OZLog` above, same fix. Its one real
    // definition is `src/OZLog.c:26`, linked in unconditionally.
    h.push_str(
        "/* OZLog -- formatted logging with %@ object support; defined in src/OZLog.c */\n\
         void OZLog(const char *fmt, ...);\n\
         int oz_log_precision(void);\n\n",
    );

    // Declared here, defined in OZHeap's own file -- see
    // `render_heap_inner_accessor`.
    if program.heap_support && program.is_class("OZHeap") {
        h.push_str(
            "/* OZHeap's inner store, reached through an accessor because this header\n * only forward-declares the struct -- see the definition in OZHeap's file */\n#ifdef OZ_HEAP_SUPPORT\nstruct oz_heap_inner *OZHeap_oz_inner(struct OZHeap *self);\n#endif\n\n",
        );
    }

    // The shared element-buffer pool. Declared here and defined once in
    // the companion source, because both OZArray's and OZDictionary's
    // builders draw from it and each lives in its own translation unit.
    // Omitted entirely when nothing needs it -- see
    // `pools::PoolSizes::item_slots`.
    if pools.item_slots() > 0 {
        h.push_str(
            "/* Shared pool for '@[...]'/'@{...}' element buffers; defined in\n * \
oz2c_dispatch.c. A static, no-heap store on Zephyr\n * \
(`sys_mem_blocks`) and a count-enforcing malloc-backed one on host, both\n * \
via the PAL. */\nextern oz_mem_blocks_t oz_item_pool;\n\n",
        );
    }

    // One list in **source order**, where this used to be three keyed on
    // kind: forward declares, then enums, then structs and unions.
    //
    // Three lists could not be ordered correctly once `typedef` joined
    // them (#533). The old arrangement rested on a real argument -- "a
    // hoisted struct can have an enum field by value, and then needs that
    // enum complete first ... nothing runs the other way: an enum cannot
    // contain a struct", with
    // `tests/behavior/cases/regression/issue_090_header_preservation.m`
    // behind it. That holds for exactly two kinds. A typedef runs **both**
    // ways:
    //
    //     typedef int Celsius;
    //     struct reading { Celsius temp; };      /* typedef first */
    //
    //     struct px_range { int lo, hi; };
    //     typedef struct px_range Range;         /* struct first */
    //
    // so no fixed order over kinds can serve both, and picking one leaves
    // the other emitting a type before its dependency. Measured before the
    // change: the first shape produced `unknown type name 'Celsius'`
    // *inside* the hoisted struct.
    //
    // Source order is the answer and needs no analysis: C required the
    // author to write these in a working order already, and the top-level
    // walk visits them in that order, so appending to one list preserves
    // it. It also still satisfies the enum-before-struct case, because the
    // author had to write the enum first for their own file to compile.
    if !hoisted_c_types.is_empty() {
        h.push_str(
            "/* plain C type declarations hoisted here from source -- enums, structs,\n * unions, forward declares and typedefs -- so each is complete before\n * any method prototype below names it, and in every generated file\n * rather than only the one it was written in.\n *\n * Source order is preserved across all of them: a typedef may name a\n * struct or be named by one, so no ordering keyed on the kind works. */\n",
        );
        for d in hoisted_c_types {
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
        h.push_str(&format!("#define OZ_CLASS_{} {}\n", name, id));
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
                "#ifdef OZ_HEAP_SUPPORT\nstruct {name} *{name}_oz_dynamic_alloc_with_heap(struct {root} *heap_obj);\n#endif\n",
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
            h.push_str(
            "/* The class's own name, for the default `-getDescription:maxLength:`\n * (see `OZObject.m`). A switch rather than a table indexed by class_id:\n * the ids are dense so either would do, but a switch costs no pointer\n * array and the linker drops the whole function when nothing reaches\n * the default -- which is every program that never uses `%@` on a class\n * without its own description (not from source) */\n",
        );
        h.push_str(&format!(
            "const char *oz_class_name(struct {root} *self);\n",
            root = name
        ));
        h.push_str(&format!(
                "struct {root} *oz_retain(struct {root} *self);\n\
                 void oz_release(struct {root} *self);\n\
                 /* `id`, not `struct {root} *`, and alone among the three in that.\n \
                 * This is the only one Objective-C source may call -- ARC forbids\n \
                 * '[obj retainCount]' and owns the retain/release pair -- so\n \
                 * 'include/oz_sdk/Foundation/OZObject.h' declares it too, for\n \
                 * Clang's AST dump, and that header cannot name a generated\n \
                 * struct. The two declarations have to agree: the SDK header is\n \
                 * spliced into this program's C, so a differing parameter type\n \
                 * would be a conflicting declaration rather than a redundant one.\n \
                 * It replaced a separate reserved-prefix forwarder in #418. */\n\
                 int oz_retain_count(id obj);\n",
                root = name
            ));
            if root_needs_synthetic_dealloc {
                h.push_str(&format!("void {root}_dealloc(struct {root} *self);\n", root = name));
            }
        }
        h.push('\n');
    }

    let mut c = String::new();
    c.push_str("/* Auto-generated by oz2c -- do not edit */\n#include \"oz2c_dispatch.h\"\n\n");
    // The header above declares `oz_log_precision` unconditionally,
    // because OZNumber's `-getDescription:maxLength:` calls it. Its real
    // definition is in `src/OZLog.c`, which is pure C and never transpiled,
    // so any build that does not link that file was left with an undefined
    // symbol -- a link error, with nothing naming the cause. A weak default
    // makes the symbol always resolve and lets `OZLog.c` override it where
    // it is linked. This is the oracle's own mechanism, verbatim: see
    // `tools/oz_transpile/tests/golden/simple_led/expected/Foundation/oz_dispatch.c`.
    c.push_str(
        "/* Weak default: returns -1 (no precision override).\n * \
src/OZLog.c provides the strong definition where it is linked. */\n\
         __attribute__((weak)) int oz_log_precision(void) { return -1; }\n\n",
    );

    // The one definition of the element-buffer pool, matching the `extern`
    // in the header above. Block size is one root-class pointer, because
    // that is what an element slot holds; the oracle sizes it the same way
    // (`templates/oz_dispatch.c.j2`: `OZ_MEM_BLOCKS_DEFINE(oz_item_pool,
    // sizeof(struct {{ root_class }} *), ...)`).
    //
    // No trailing `;`: OZ_MEM_BLOCKS_DEFINE is self-terminating on both
    // PAL backends. It has to be, and this line is why (#266). On Zephyr
    // it expands to SYS_MEM_BLOCKS_DEFINE, whose body already ends in
    // `;`, so the one written here became a bare `;` at file scope -- an
    // empty declaration, which ISO C does not allow. The host backend's
    // macro ended in `}` and needed it, so the same emission was correct
    // on host and invalid on target, and no host check could ever see
    // the difference. Unlike gap X's other producers this reached only
    // programs that build an item pool, which is why it outlived them.
    if pools.item_slots() > 0 {
        let pool_root = root.as_deref().unwrap_or("OZObject");
        // No nested comment delimiters in this text: the directive is
        // named without its surrounding slash-star, which would close
        // this comment early.
        c.push_str(&format!(
            "/* Element buffers for '@[...]' and '@{{...}}': {slots} id-slot(s),\n * \
sized by counting literal sites (see pools.rs). Override with the\n * \
--item-pool-size flag or an 'oz-item-pool: N' source directive. */\n\
             OZ_MEM_BLOCKS_DEFINE(oz_item_pool, sizeof(struct {root} *), {slots}, {align})\n\n",
            slots = pools.item_slots(),
            root = pool_root,
            align = crate::pools::SLAB_ALIGNMENT
        ));
    }

    if let Some(root) = &root {
        c.push_str(&render_alloc_free(
            root,
            root,
            pools.for_class(root),
            &program.owned_object_ivars(root),
            program.heap_support,
            program.class_conforms_to(root, SINGLETON_PROTOCOL),
        ));

        if root_needs_synthetic_dealloc {
            c.push_str(&format!(
                "/* synthesized: {root} has no -dealloc in source -- a no-op so a class\n * \
with no -dealloc anywhere in its chain still has one to call (not from source) */\n\
void {root}_dealloc(struct {root} *self)\n{{\n\t(void)self;\n}}\n\n",
                root = root
            ));
        }

        c.push_str(
            "/* synthesized: increments the retain count; shared by every class,\n * \
not tied to one (not from source) */\n",
        );
        c.push_str(&render_heap_bridge(program.heap_support, program.is_class("OZHeap")));
        c.push_str(
            "/* synthesized: the class's own name, read by the default\n * `-getDescription:maxLength:` (not from source) */\n",
        );
        c.push_str(&format!(
            "const char *oz_class_name(struct {root} *self)\n{{\n\
             \tif (!self) {{\n\t\treturn \"nil\";\n\t}}\n\
             \tswitch (self->_meta.class_id) {{\n",
            root = root
        ));
        for name in &program.class_order {
            c.push_str(&format!(
                "\tcase OZ_CLASS_{name}: return \"{name}\";\n",
                name = name
            ));
        }
        c.push_str(
            "\tcase OZ_CLASS_ID_FREED: return \"freed\";\n\
             \tdefault: return \"?\";\n\t}\n}\n\n",
        );
        c.push_str(&format!(
            "struct {root} *oz_retain(struct {root} *self)\n{{\n\
             \t/* An immortal object is not refcounted -- the same rule\n\
             \t * oz_release applies before its decrement. Retaining one\n\
             \t * used to increment a word nothing would ever decrement, which\n\
             \t * both wasted an atomic and left retainCount climbing without\n\
             \t * bound (#373). It also kept a boxed literal out of .rodata:\n\
             \t * anything that writes an object cannot be const. */\n\
             #ifdef OZ_DEBUG_REFCOUNT\n\
             \t/* First, and above every read of `_meta`, because after a free\n\
             \t * `_meta` is the allocator's free-list link and any bit of it\n\
             \t * can send this function down the wrong path (#490). The\n\
             \t * refcount word is the one the link does not reach. The class\n\
             \t * is not recoverable here, so the address is what gets\n\
             \t * printed -- it names the slot, which is what a slab debug\n\
             \t * session needs. */\n\
             \tif (self && oz_atomic_get(&self->oz_refcount) == OZ_REFCOUNT_FREED) {{\n\
             \t\tOZ_PLATFORM_PRINT(\"oz: retain of a freed object at %p\\n\",\n\
             \t\t\t\t  (void *)self);\n\
             \t\toz_platform_flush();\n\
             \t\toz_assert_msg(0, \"retain after free -- this slot was already \
returned to its slab; the address is on the line above\");\n\
             \t}}\n\
             \t/* Retaining an object whose teardown has begun resurrects a\n\
             \t * reference the dealloc switch has already passed, so the retain\n\
             \t * succeeds and the object is freed under its new owner (#452). */\n\
             \tif (self && self->_meta.deallocating) {{\n\
             \t\tOZ_PLATFORM_PRINT(\"oz: retain of %s during its own dealloc\\n\",\n\
             \t\t\t\t  oz_class_name(self));\n\
             \t\toz_platform_flush();\n\
             \t\toz_assert_msg(0, \"retain during dealloc -- this object is being \
torn down; the class is named on the line above\");\n\
             \t}}\n\
             #endif\n\
             \tif (self && !self->_meta.immortal) {{\n\t\toz_atomic_inc(&self->oz_refcount);\n\t}}\n\treturn self;\n}}\n\n",
            root = root
        ));
        c.push_str(
            "/* synthesized: reads the current retain count; 0 for a nil\n * \
receiver. Takes 'id' because Objective-C source calls this one directly\n * \
and 'include/oz_sdk/Foundation/OZObject.h' has to declare it without\n * \
naming a generated struct (not from source) */\n",
        );
        c.push_str(&format!(
            "int oz_retain_count(id obj)\n{{\n\
             \tstruct {root} *self = (struct {root} *)obj;\n\
             \tif (!self) {{\n\t\treturn 0;\n\t}}\n\
             \t/* Not refcounted, so the stored word is not maintained and\n\
             \t * reporting it would be reporting a stale number. One permanent\n\
             \t * reference is the truthful answer (#373). */\n\
             \tif (self->_meta.immortal) {{\n\t\treturn 1;\n\t}}\n\
             \treturn oz_atomic_get(&self->oz_refcount);\n}}\n\n",
            root = root
        ));

        c.push_str(
            "/* dealloc dispatch: the one virtual call this design needs. Resolved\n * \
entirely at compile time via this class_id switch (the \"const\n * \
vtable\") -- never mutated at runtime. */\n",
        );
        c.push_str(&format!(
            "void oz_release(struct {root} *self)\n{{\n\
             \tif (!self) {{\n\t\treturn;\n\t}}\n\
             #ifdef OZ_DEBUG_REFCOUNT\n\
             \t/* **Above the immortal check, and that position is the whole\n\
             \t * point (#490).** After a free, `_meta` holds the allocator's\n\
             \t * free-list link, and bit 12 of a link is `_meta.immortal`:\n\
             \t * measured on mps2/an385 the link was `0x2000324c`, whose bit\n\
             \t * 12 read back 1, so a release of a freed object returned at\n\
             \t * the immortal check and no trap ran at all. On\n\
             \t * qemu_cortex_a53 the link's bit 12 read back 0, so there it\n\
             \t * would have reached the trap. The refcount\n\
             \t * word is the one the link cannot reach -- it begins at\n\
             \t * exactly sizeof(char *) -- so this check is decided by what\n\
             \t * `_oz_free` wrote rather than by an address.\n\
             \t *\n\
             \t * A live immortal object cannot trip it: its refcount is 1 --\n\
             \t * from `_oz_alloc`, or written straight into the initializer\n\
             \t * for a boxed literal -- and nothing maintains it, so it is\n\
             \t * never the sentinel.\n\
             \t *\n\
             \t * It is, however, the first time `oz_release` *reads* an\n\
             \t * immortal object's refcount: the immortal return used to come\n\
             \t * first. Safe, and checked rather than assumed. A boxed literal\n\
             \t * is a `const struct` in .rodata, and both of Zephyr's\n\
             \t * `atomic_get` implementations are pure loads taking a\n\
             \t * `const atomic_t *` -- `__atomic_load_n` in\n\
             \t * sys/atomic_builtin.h, `*target` in kernel/atomic_c.c -- so\n\
             \t * nothing here writes read-only storage. A future backend\n\
             \t * whose atomic read is a compare-and-swap would fault, which\n\
             \t * is the thing to re-check before adding one.\n\
             \t *\n\
             \t * No gate *runs* that combination: `tests/zephyr` is the only\n\
             \t * thing that compiles with this flag and it declares no boxed\n\
             \t * literal, and the samples that do declare one leave the flag\n\
             \t * off. So the two sentences above are a reading of Zephyr's\n\
             \t * headers, not a measurement -- which is the honest label for\n\
             \t * them. */\n\
             \tif (oz_atomic_get(&self->oz_refcount) == OZ_REFCOUNT_FREED) {{\n\
             \t\tOZ_PLATFORM_PRINT(\"oz: release of a freed object at %p\\n\",\n\
             \t\t\t\t  (void *)self);\n\
             \t\toz_platform_flush();\n\
             \t\toz_assert_msg(0, \"release after free -- this slot was already \
returned to its slab; the address is on the line above\");\n\
             \t}}\n\
             #endif\n\
             \t/* Immortal objects live in static storage and are never freed, so\n\
             \t * their refcount is not tracked either -- the check comes before\n\
             \t * the decrement, not after it. */\n\
             \tif (self->_meta.immortal) {{\n\t\treturn;\n\t}}\n\
             #ifdef OZ_DEBUG_REFCOUNT\n\
             \t/* Before the decrement, because afterwards the evidence is gone:\n\
             \t * oz_atomic_dec_and_test is atomic_fetch_sub(t, 1) == 1, so a\n\
             \t * release at 0 leaves -1 and returns as though nothing happened.\n\
             \t * A dealloc counter cannot see it either -- the refcount never\n\
             \t * reaches 0 again -- and nor can a slot count, which the host\n\
             \t * slab clamps (#452).\n\
             \t *\n\
             \t * Printed and then asserted rather than asserted with the class\n\
             \t * in the message: oz_assert_msg takes a plain const char * and no\n\
             \t * format arguments, so naming the class is the print's job.\n\
             \t *\n\
             \t * The flush is not decoration. printf to anything but a terminal\n\
             \t * is fully buffered and abort() does not flush stdio, so on glibc\n\
             \t * this line reached a buffer that was then discarded -- the\n\
             \t * assertion text survived on stderr and the class name did not.\n\
             \t * It read as a trap that could not name a class. */\n\
             \tif (oz_atomic_get(&self->oz_refcount) <= 0) {{\n\
             \t\tOZ_PLATFORM_PRINT(\"oz: over-release of %s\\n\",\n\
             \t\t\t\t  oz_class_name(self));\n\
             \t\toz_platform_flush();\n\
             \t\toz_assert_msg(0, \"over-release -- this refcount was already 0; \
the class is named on the line above\");\n\
             \t}}\n\
             #endif\n\
             \tif (!oz_atomic_dec_and_test(&self->oz_refcount)) {{\n\t\treturn;\n\t}}\n\
             \tif (self->_meta.deallocating) {{\n\t\treturn;\n\t}}\n\
             \tself->_meta.deallocating = 1;\n\
             \tswitch (self->_meta.class_id) {{\n",
            root = root
        ));
        for name in &program.class_order {
            c.push_str(&format!("\tcase OZ_CLASS_{}: /* {} */\n", name, name));
            /* The whole `[super dealloc]` chain, most-derived first,
             * because that send is rejected in source now and something
             * still has to run a superclass's own cleanup (#428 -- see
             * `dealloc_chain`). A class with no `-dealloc` anywhere in its
             * chain calls the root's, which is where the synthesized no-op
             * comes in, and that is the case whose output is unchanged. */
            let mut chain = dealloc_chain(program, name);
            if chain.is_empty() {
                chain.push(root.clone());
            }
            for defining in &chain {
                c.push_str(&format!("\t\t{}_dealloc((struct {} *)self);\n", defining, defining));
            }
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
        /* The `default:` arm is where a freed or corrupt pointer lands: its
         * class_id matches no live class, so a silent `break` returns as
         * though the object had been deallocated. Under the debug flag it
         * names the failure instead (#452). */
        c.push_str(
            "\tdefault:\n\
             #ifdef OZ_DEBUG_REFCOUNT\n\
             \t\toz_assert_msg(0, \"released an object whose class_id matches no \
live class -- a freed, poisoned or corrupt pointer reached oz_release\");\n\
             #endif\n\
             \t\tbreak;\n\t}\n}\n\n",
        );
    }

    /* Beside the class_id switch, and outside the `root` guard: a program
     * with no class has no switch, and the census still has to exist for
     * the generated main() that calls it unconditionally -- it just
     * answers zero. */
    let (census_h, census_c) = render_leak_census(program, pools);
    h.push_str(&census_h);
    c.push_str(&census_c);

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

    let (intro_h, intro_c) = render_introspection(program, introspection_used);
    h.push_str(&intro_h);
    c.push_str(&intro_c);

    if let Some(root) = &root {
        let (refl_h, refl_c) = render_reflection(program, root);
        h.push_str(&refl_h);
        c.push_str(&refl_c);
    }

    // Last, so it sees every prototype this header ended up with.
    let h = forward_declare_unknown_struct_tags(&h);

    (h, c)
}
