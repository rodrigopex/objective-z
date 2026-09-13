// SPDX-License-Identifier: Apache-2.0
//
// poison_emission.rs - freed-slot poisoning is emitted, in the right place,
// and only where it does something (#452).
//
// **Why this pins emitted text rather than behaviour.** The poison is not
// observable from a host test. `oz_slab_free` on the host backend is
// `free()`, and the allocator owns the block from that moment: probed on
// arm64 macOS, an object whose ivar held `0x11111111` and whose body poison
// was `0xA5` read back `0x00000003` after the free. Zephyr's
// `k_mem_slab_free` is gentler -- it writes only the free-list link, into
// the block's first word -- so on target the body poison survives and the
// `class_id` stamp does not. Either way the only thing a host harness can
// check is that the right C is generated; `refcount_traps.rs` carries the
// measurement and the reasoning.
//
// That makes these assertions worth more than they look: they are the whole
// gate on a feature whose effect nothing here can see.
//
// Every absence check below is paired with a presence check on the same
// fixture. An unpaired `!contains` is the trap #462's rename walked into --
// a negative assertion against a name that no longer exists passes while
// asserting nothing.

mod common;
use common::ozobject_src as PREAMBLE;

/// A root class plus one subclass carrying an ivar, so the two shapes of
/// `_oz_free` -- body and no body -- both appear in one program.
fn source() -> String {
    format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Widget : OZObject {
	int _v;
}
@end
@implementation Widget
@end

int main(void)
{
	Widget *w = [Widget alloc];
	(void)w;
	return 0;
}
"
    )
}

/// Both emitted C files, concatenated.
///
/// The two `_oz_free` shapes are not in the same file: the root's is
/// emitted into the shared companion, and a subclass's into its own
/// translation unit (`render_interface` calls `render_alloc_free`). A check
/// that read only the companion would find the root's stamp, report the
/// feature present, and never look at the one with a body to poison.
fn emitted_c() -> String {
    let out = oz2c::transpile(&source()).expect("the fixture must transpile");
    format!("{}\n{}", out.source_c, out.companion_c)
}

fn companion_h() -> String {
    let out = oz2c::transpile(&source()).expect("the fixture must transpile");
    out.companion_h
}

/// The reserved id is defined, and defined *unconditionally*.
///
/// Not behind `OZ_DEBUG_REFCOUNT`: what a reserved id needs is that no
/// class ever takes it, which is a fact about the numbering rather than
/// about the instruments. A program compiled without the flag still must
/// not hand 1023 to a class.
#[test]
fn the_reserved_id_is_defined_unconditionally() {
    let h = companion_h();
    assert!(
        h.contains("#define OZ_CLASS_ID_FREED 1023"),
        "the reserved id must be defined, at the top of the 10-bit class_id range; got:\n{}",
        h
    );

    /* Paired with the presence check above: the `#define` must not sit
     * inside the debug guard. Proven by looking at what precedes it rather
     * than by a bare `!contains`, which would pass on a header that had no
     * define at all. */
    let at = h.find("#define OZ_CLASS_ID_FREED").unwrap();
    assert!(
        !inside_debug_guard(&h, at),
        "OZ_CLASS_ID_FREED is inside `#ifdef OZ_DEBUG_REFCOUNT`, so an unflagged build \
         would not reserve it and a class could be given 1023"
    );
}

/// `oz_class_name` renders the reserved id, and still renders real classes.
///
/// The second half is the pairing: an arm for the reserved id that replaced
/// the per-class arms would satisfy the first assertion on its own.
#[test]
fn class_name_renders_the_reserved_id_and_the_real_classes() {
    let c = emitted_c();
    assert!(
        c.contains("case OZ_CLASS_ID_FREED: return \"freed\";"),
        "a poisoned object must render as `freed` rather than falling to `?`; got:\n{}",
        c
    );
    assert!(
        c.contains("case OZ_CLASS_Widget: return \"Widget\";"),
        "the reserved arm must be an addition, not a replacement of the per-class arms",
    );
    assert!(
        c.contains("default: return \"?\";"),
        "`?` stays the answer for an id that matches nothing at all -- a corrupt pointer \
         is not the same fault as a poisoned one",
    );
}

/// The stamp lands in `_oz_free`, and **before** the heap check.
///
/// `render_heap_free_check` ends in a `return` for a heap-allocated
/// instance. Poison placed after it would cover slab objects and silently
/// miss every heap one -- which is the kind of half-instrumented state that
/// reads as working.
#[test]
fn the_stamp_precedes_the_heap_check_that_returns() {
    let src = format!(
        "{}{}",
        PREAMBLE(),
        "\
@interface Widget : OZObject {
	int _v;
}
@end
@implementation Widget
@end

int main(void)
{
	Widget *w = [Widget alloc];
	(void)w;
	return 0;
}
"
    );
    let out = oz2c::transpile_with_options(
        &src,
        &oz2c::Options { heap_support: true, ..Default::default() },
    )
    .expect("the fixture must transpile with heap support");
    let c = format!("{}\n{}", out.source_c, out.companion_c);

    let body = free_body(&c, "Widget");
    let stamp = body
        .find("_meta.class_id = OZ_CLASS_ID_FREED")
        .unwrap_or_else(|| panic!("no stamp in Widget_oz_free:\n{}", body));
    let heap = body
        .find("_meta.heap_allocated")
        .unwrap_or_else(|| panic!("no heap check in Widget_oz_free, so this proved nothing:\n{}", body));
    assert!(
        stamp < heap,
        "the stamp must precede the heap check, which returns -- otherwise heap-allocated \
         objects go back unpoisoned. stamp at {}, heap check at {}:\n{}",
        stamp,
        heap,
        body
    );
}

/// The body poison covers the subclass's ivars and is omitted for the root.
///
/// The root class has nothing past its own prefix, so the length would be a
/// literal zero. Emitting it anyway would be a `memset` of 0 bytes on every
/// root free, and noise in output people read.
#[test]
fn the_body_poison_covers_a_subclass_and_is_omitted_for_the_root() {
    let c = emitted_c();

    let widget = free_body(&c, "Widget");
    assert!(
        widget.contains("memset((char *)obj + sizeof(struct OZObject), 0xA5,"),
        "a subclass's ivars are what a use-after-free reads, and the body past the root \
         prefix is the part a free-list write does not touch; got:\n{}",
        widget
    );
    assert!(
        widget.contains("sizeof(struct Widget) - sizeof(struct OZObject)"),
        "the poisoned length must be the subclass's own body, not the whole object -- the \
         header is where the allocator writes; got:\n{}",
        widget
    );

    /* The absence check, paired: the root's `_oz_free` is proven to exist
     * and to carry the stamp, so "no memset" is a real finding about it
     * rather than a fixture that never had one. */
    let root = free_body(&c, "OZObject");
    assert!(
        root.contains("_meta.class_id = OZ_CLASS_ID_FREED"),
        "the root's free must still stamp; without this the next assertion is vacuous:\n{}",
        root
    );
    assert!(
        !root.contains("memset"),
        "the root has no body past its own prefix, so it must emit no body poison; got:\n{}",
        root
    );
}

/// Every poison statement sits under `OZ_DEBUG_REFCOUNT`.
///
/// The instruments are off by default because each costs stores on a path
/// that runs for every object. A stamp that escaped the guard would be a
/// silent cost in a shipped build.
#[test]
fn the_poison_is_entirely_behind_the_debug_flag() {
    let c = emitted_c();
    for (stem, what) in [
        ("_meta.class_id = OZ_CLASS_ID_FREED", "the class_id stamp"),
        ("_meta.immortal = 0", "the immortal clear"),
        ("memset((char *)obj + sizeof(struct OZObject), 0xA5,", "the body poison"),
    ] {
        assert!(c.contains(stem), "{} is missing, so its guard was not tested", what);
        for (i, _) in c.match_indices(stem) {
            assert!(
                inside_debug_guard(&c, i),
                "{} at offset {} is outside `#ifdef OZ_DEBUG_REFCOUNT`, so an unflagged \
                 build pays for it",
                what,
                i
            );
        }
    }
}

/// Is the text at `at` inside an `#ifdef OZ_DEBUG_REFCOUNT` block?
///
/// Nearest preceding directive wins, rather than counting opens against
/// closes across the whole file. Counting is what a first attempt at this
/// did, and it is wrong here for a reason worth keeping: the emitted C is
/// full of *other* conditionals -- `#ifdef OZ_HEAP_SUPPORT`, `#ifndef Nil`,
/// `#ifdef OZ_TRAP_POOL_EXHAUSTION` -- and every one of them contributes an
/// `#endif` that the count attributes to the debug guard. The poison blocks
/// contain no nested conditionals, so a single-level scan is exact.
fn inside_debug_guard(text: &str, at: usize) -> bool {
    let before = &text[..at];
    let open = before.rfind("#ifdef OZ_DEBUG_REFCOUNT");
    let close = before.rfind("#endif");
    match (open, close) {
        (Some(o), Some(c)) => o > c,
        (Some(_), None) => true,
        _ => false,
    }
}

/// The text of one `{name}_oz_free`, from its opening brace to the closing
/// one at column 0.
///
/// A crude scan rather than a parse, and deliberately so: this file exists
/// to read the output the way a person does.
fn free_body<'a>(c: &'a str, name: &str) -> &'a str {
    let sig = format!("void {}_oz_free(", name);
    let start = c
        .find(&sig)
        .unwrap_or_else(|| panic!("no `{}` in the companion source:\n{}", sig, c));
    let rest = &c[start..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("`{}` is never closed:\n{}", sig, rest));
    &rest[..end]
}
