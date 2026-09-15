// SPDX-License-Identifier: Apache-2.0
//
// poison_emission.rs - freed-slot poisoning is emitted, in the right place,
// and only where it does something (#452).
//
// **Why this pins emitted text rather than behaviour.** The poison is not
// observable from a host test, and the freed object it describes is not a
// testable subject at all. `oz_slab_free` on the host backend is `free()`,
// and the allocator owns the block from that moment -- so what a later read
// finds there is the allocator's business, and the two hosts this ran on
// disagree:
//
//   * arm64 macOS -- an object whose ivar held `0x11111111` and whose body
//     poison was `0xA5` read back `0x00000003` after the free. Neither the
//     stamp nor the body poison survived.
//   * Linux/glibc in CI -- worse than lost. glibc writes a tcache `next`
//     pointer over the first word, and `_meta` is the root struct's first
//     member, so bit 12 of that pointer *is* `_meta.immortal`: a release of
//     the freed object returns at the immortal check above the trap, and
//     the over-release is never seen. A double-release fixture that aborts
//     on macOS exits 0 there.
//
// Zephyr's `k_mem_slab_free` is the gentle case -- it writes only the
// free-list link, into the block's first word -- so on target the body
// poison survives and the `class_id` stamp does not.
//
// So the only thing any host harness can honestly check is that the right C
// is generated. `refcount_traps.rs` carries the rest of the reasoning, and
// stages its over-release on a *live* object for exactly this reason.
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

/// The freed *refcount* sentinel is defined, and defined unconditionally.
///
/// Same reasoning as `OZ_CLASS_ID_FREED` above: what a reserved value needs
/// is that nothing else ever produces it, which is a fact about the
/// numbering rather than about the instruments.
#[test]
fn the_freed_refcount_sentinel_is_defined_unconditionally() {
    let h = companion_h();
    assert!(
        h.contains("#define OZ_REFCOUNT_FREED 0x0FEEDFEE"),
        "the freed sentinel must be defined -- it is the only marker that outlives a \
         k_mem_slab_free (#490); got:\n{}",
        h
    );

    /* Paired with the presence check above, and proven by what precedes
     * the define rather than by a bare `!contains`, which would pass on a
     * header that had no define at all. */
    let at = h.find("#define OZ_REFCOUNT_FREED").unwrap();
    assert!(
        !inside_debug_guard(&h, at),
        "OZ_REFCOUNT_FREED is inside `#ifdef OZ_DEBUG_REFCOUNT`, so an unflagged build \
         would not reserve it"
    );
}

/// `_oz_free` stamps the **sentinel**, not zero.
///
/// It was `0` until #490. Zero is also what a live immortal object holds
/// and what an object mid-teardown holds, so a trap keying on it could not
/// tell a freed slot from either -- and `<= 0` is the over-release trap's
/// own condition, so the two faults reported as one.
///
/// The absence check is paired with the presence check on the same fixture
/// and the same function body, so "no longer zero" is a finding about a
/// `_oz_free` that demonstrably still stamps something.
#[test]
fn the_free_stamps_the_sentinel_rather_than_zero() {
    let c = emitted_c();
    for class in ["OZObject", "Widget"] {
        let body = free_body(&c, class);
        assert!(
            body.contains("oz_atomic_init(&((struct OZObject *)obj)->oz_refcount, OZ_REFCOUNT_FREED);"),
            "{}_oz_free must stamp the sentinel into the one word the free-list link \
             cannot reach; got:\n{}",
            class,
            body
        );
        assert!(
            !body.contains("oz_refcount, 0)"),
            "{}_oz_free must not stamp 0 -- that is indistinguishable from a live \
             immortal object and from the over-release trap's own condition; got:\n{}",
            class,
            body
        );
    }
}

/// The sentinel check in `oz_release` sits **above** the immortal check.
///
/// This is the position #490 is about, and the ordering assertion is the
/// only thing that can hold it: after a free, `_meta` is the allocator's
/// free-list link, and bit 12 of a link *is* `_meta.immortal`. Measured on
/// mps2/an385 that bit was 1, so `oz_release` returned above the trap and a
/// release of a freed object was silent; on qemu_cortex_a53 it was 0 and the
/// trap fired. The refcount word is the one the link cannot reach, so the
/// check must be reached before any bit of `_meta` is consulted.
///
/// Every landmark is asserted present before the ordering claim, so a
/// renamed or deleted check cannot make the comparison vacuously true.
/// `refcount_traps.rs` measures the same property by running the C.
#[test]
fn the_freed_check_precedes_every_read_of_meta() {
    let c = emitted_c();

    let release = fn_body(&c, "void oz_release(struct OZObject *self)");
    let freed = release
        .find("== OZ_REFCOUNT_FREED")
        .unwrap_or_else(|| panic!("no freed-sentinel check in oz_release:\n{}", release));
    let immortal = release
        .find("_meta.immortal")
        .unwrap_or_else(|| panic!("no immortal check in oz_release, so this proved nothing:\n{}", release));
    let over = release
        .find("<= 0")
        .unwrap_or_else(|| panic!("no over-release check in oz_release:\n{}", release));
    assert!(
        freed < immortal,
        "the freed check must precede `_meta.immortal`, which is bit 12 of the free-list \
         link after a free. freed at {}, immortal at {}:\n{}",
        freed,
        immortal,
        release
    );
    assert!(
        freed < over,
        "the freed check must precede the over-release check, or a freed slot is reported \
         as an over-release. freed at {}, over-release at {}:\n{}",
        freed,
        over,
        release
    );

    let retain = fn_body(&c, "struct OZObject *oz_retain(struct OZObject *self)");
    let freed = retain
        .find("== OZ_REFCOUNT_FREED")
        .unwrap_or_else(|| panic!("no freed-sentinel check in oz_retain:\n{}", retain));
    let deallocating = retain
        .find("_meta.deallocating")
        .unwrap_or_else(|| panic!("no deallocating check in oz_retain, so this proved nothing:\n{}", retain));
    assert!(
        freed < deallocating,
        "the freed check must precede `_meta.deallocating`, bit 11 of the same clobbered \
         word. freed at {}, deallocating at {}:\n{}",
        freed,
        deallocating,
        retain
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
        ("oz_refcount, OZ_REFCOUNT_FREED)", "the refcount sentinel stamp"),
        ("== OZ_REFCOUNT_FREED", "the freed-slot check in oz_retain/oz_release"),
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
/// The text of one function, from its exact signature to the closing brace
/// at column 0.
///
/// Takes the whole signature rather than a name, because `oz_release` and
/// `oz_retain` are also *called* and *prototyped* in the same file -- a
/// scan for the bare name would land on the header's declaration and return
/// a body containing no checks at all, which would make every ordering
/// assertion below it fail for the wrong reason.
/// **Comments stripped**, and that is not tidiness. The first version of
/// this helper returned the body verbatim, and the ordering assertions
/// below then found `_meta.immortal` at offset 265 -- inside the *comment*
/// explaining why the freed check has to come first. The test failed while
/// the code was correct, which is the same class of error as a guard
/// passing while the property is gone, just pointing the other way. An
/// assertion about where a check *runs* has to read code.
///
/// A naive scan, deliberately: the emitted C contains no `/*` inside a
/// string literal, and adding a lexer here would be a second parser to
/// keep true.
fn fn_body(c: &str, sig: &str) -> String {
    let start = c
        .find(&format!("{}\n{{", sig))
        .unwrap_or_else(|| panic!("no definition of `{}` in the emitted C:\n{}", sig, c));
    let rest = &c[start..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("`{}` is never closed:\n{}", sig, rest));
    strip_block_comments(&rest[..end])
}

/// Every `/* ... */` replaced by a single space, so offsets stay ordered
/// and two tokens either side of a comment do not run together.
fn strip_block_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        out.push(' ');
        let after = &rest[open + 2..];
        match after.find("*/") {
            Some(close) => rest = &after[close + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

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
