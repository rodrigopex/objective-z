// SPDX-License-Identifier: Apache-2.0
//
// emitter_agreement.rs -- do `emit` and `emit_split` agree about each
// top-level node kind? (#254)
//
// The two are independent walks over the same CST, each with its own match on
// node kind, and they have disagreed three times:
//
//   gap R   `staticbar` and `emit::collect_local_decls` listed different
//           declarator kinds as "a local"
//   #246    `class_tag_edits` was called from `emit_split` only; `emit` had no
//           `declaration` arm at all
//   #251    `file_scope_vars` gated on the known-class set keyed by bare name
//           while `collect_local_decls` had no gate
//
// Every one was stumbled into. Nobody had ever compared the two deliberately,
// which is the first task #254 names -- this file is that comparison, kept as
// a test so it runs rather than being a one-off sweep.
//
// **The asymmetry that makes this necessary**: every other test in this suite
// drives `oz_static::transpile()`, hence `emit`. Every real build drives the
// CLI, hence `emit_split`. So the path with test coverage is not the path that
// ships, and a divergence can exist in either direction without anything going
// red.
//
// What is compared, and why that and not the text:
//
//   - **Diagnostics must be identical.** This is the accept/reject question,
//     and it is where the damage lives: one emitter refusing what the other
//     accepts means either a build that fails for no reason or a construct
//     that reaches the C compiler unchecked.
//   - **A named symbol must appear in both outputs.** The two deliberately
//     place text differently -- one file versus a per-origin pair -- so the
//     text cannot be diffed. That a construct is *present somewhere* can be.
//
// Everything before `emit` is shared (`collect`, `generics`, `arc`, `pools`),
// so any divergence found here is genuinely the emitters' own.

mod common;
use common::ozobject_src;

/// Run both emitters over one source. Returns (single-file diagnostics,
/// split diagnostics, single-file text, split text-of-everything).
fn both(source: &str) -> (Vec<String>, Vec<String>, String, String) {
    let origins = vec![("audit".to_string(), 0..source.len())];

    let (single_diags, single_text) = match oz_static::transpile(source) {
        Ok(out) => (Vec::new(), format!("{}{}{}", out.source_c, out.companion_h, out.companion_c)),
        Err(diags) => (diags.iter().map(|d| d.message.clone()).collect(), String::new()),
    };

    let (split_diags, split_text) = match oz_static::transpile_split(source, &origins) {
        Ok(out) => {
            let mut text = String::new();
            for (_stem, h, c) in &out.files {
                text.push_str(h);
                text.push_str(c);
            }
            text.push_str(&out.companion_h);
            text.push_str(&out.companion_c);
            (out.diagnostics.iter().map(|d| d.message.clone()).collect(), text)
        }
        Err(diags) => (diags.iter().map(|d| d.message.clone()).collect(), String::new()),
    };

    (single_diags, split_diags, single_text, split_text)
}

/// Both emitters must accept or reject alike, and both must emit `expect`
/// somewhere. `label` names the node kind under audit.
fn assert_agree(label: &str, source: &str, expect: &str) {
    let (single_diags, split_diags, single_text, split_text) = both(source);

    assert_eq!(
        single_diags, split_diags,
        "{}: the two emitters disagree on diagnostics.\n  emit():       {:?}\n  emit_split(): {:?}",
        label, single_diags, split_diags
    );
    assert!(
        single_diags.is_empty(),
        "{}: both rejected, so the audit says nothing about emission: {:?}",
        label,
        single_diags
    );
    assert!(
        single_text.contains(expect),
        "{}: emit() did not emit `{}`:\n{}",
        label,
        expect,
        single_text
    );
    assert!(
        split_text.contains(expect),
        "{}: emit_split() did not emit `{}`:\n{}",
        label,
        expect,
        split_text
    );
}

#[test]
fn class_interface_and_implementation_agree() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Widget : OZObject { int _n; }
- (int)n;
@end
@implementation Widget
- (int)n { return _n; }
@end
int main(void) { return 0; }
"
    );
    assert_agree("class_interface/class_implementation", &src, "int Widget_n(struct Widget *self)");
}

/// The kind #246 was about: `emit` had no `declaration` arm, so a bare class
/// name in a top-level declaration was copied through untagged and the C
/// compiler rejected it. This is the regression guard as an *agreement* check
/// rather than a one-emitter one.
#[test]
fn top_level_object_declaration_agrees() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Widget : OZObject { int _n; }
@end
@implementation Widget
@end
static Widget *g_widget;
int main(void) { return 0; }
"
    );
    assert_agree("declaration (file-scope object)", &src, "struct Widget *g_widget");
}

/// The other half of gap A, also missing from `emit` until #246.
#[test]
fn free_function_signature_agrees() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Widget : OZObject { int _n; }
@end
@implementation Widget
@end
static Widget *makeWidget(void) { return [Widget alloc]; }
int main(void) { (void)makeWidget(); return 0; }
"
    );
    assert_agree("function_definition (signature)", &src, "struct Widget *makeWidget(void)");
}

#[test]
fn top_level_enum_agrees() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
enum Direction { NORTH, SOUTH };
int main(void) { return NORTH; }
"
    );
    assert_agree("enum_specifier", &src, "NORTH");
}

/// Gap C's seventh cause: `emit_split` dropped a top-level struct definition
/// outright while `emit` kept it by not touching it. The reverse asymmetry to
/// #246, and the reason this audit checks both directions.
#[test]
fn top_level_struct_agrees() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
struct color { int r; };
int main(void) { struct color c; c.r = 1; return c.r; }
"
    );
    assert_agree("struct_specifier", &src, "struct color");
}

#[test]
fn top_level_union_agrees() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
union word { int i; char b[4]; };
int main(void) { union word w; w.i = 0; return w.i; }
"
    );
    assert_agree("union_specifier", &src, "union word");
}

#[test]
fn protocol_declaration_agrees() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@protocol Pokeable
- (void)poke;
@end
@interface Widget : OZObject <Pokeable> { int _n; }
- (void)poke;
@end
@implementation Widget
- (void)poke { _n = 1; }
@end
int main(void) { return 0; }
"
    );
    assert_agree("protocol_declaration", &src, "void Widget_poke(struct Widget *self)");
}

#[test]
fn compatibility_alias_agrees() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@compatibility_alias NSObject OZObject;
int main(void) { return 0; }
"
    );
    // Never valid C, so both must elide it. Both replace it with a *comment*
    // that names it -- so the check is that the declaration does not survive
    // as a statement, not that the words are absent. The trailing `;` is what
    // distinguishes the two: the comment form reads
    // `/* @compatibility_alias NSObject OZObject -- not needed ... */`.
    const AS_STATEMENT: &str = "@compatibility_alias NSObject OZObject;";
    let (single_diags, split_diags, single_text, split_text) = both(&src);
    assert_eq!(
        single_diags, split_diags,
        "compatibility_alias_declaration: diagnostics differ:\n  emit(): {:?}\n  split(): {:?}",
        single_diags, split_diags
    );
    assert!(
        !single_text.contains(AS_STATEMENT),
        "emit() left @compatibility_alias as a statement:\n{}",
        single_text
    );
    assert!(
        !split_text.contains(AS_STATEMENT),
        "emit_split() left @compatibility_alias as a statement:\n{}",
        split_text
    );
    // And they must elide it the same way, not one commenting and one dropping.
    let single_elided = single_text.contains("@compatibility_alias");
    let split_elided = split_text.contains("@compatibility_alias");
    assert_eq!(
        single_elided, split_elided,
        "the two emitters elide @compatibility_alias differently -- one keeps the \
         explanatory comment and the other drops it (emit: {}, split: {})",
        single_elided, split_elided
    );
}

/// A bare top-level macro invocation is neither a `preproc` node nor a
/// declaration -- the shape gap P was about, and the one Zephyr is full of
/// (`ZBUS_CHAN_DECLARE(...)`). Both emitters must pass it through.
#[test]
fn bare_top_level_macro_invocation_agrees() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
#define DECLARE_THING(name) int thing_##name
DECLARE_THING(alpha);
int main(void) { thing_alpha = 1; return thing_alpha; }
"
    );
    assert_agree("bare macro invocation", &src, "DECLARE_THING(alpha)");
}

/// `@synchronized` reaches `render_synchronized_statement` from both walks,
/// and #256 changed what it emits. Both must agree on the object's lock field.
#[test]
fn synchronized_agrees() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Box : OZObject { int _n; }
- (void)set:(int)v;
@end
@implementation Box
- (void)set:(int)v { @synchronized(self) { _n = v; } }
@end
int main(void) { return 0; }
"
    );
    assert_agree("synchronized_statement", &src, "->oz_sync_lock");
}

/// A rejection must be a rejection in both. If one emitter refuses a
/// construct and the other emits it, the static bar is only as strong as
/// whichever path a given caller happens to use -- and the path with test
/// coverage is not the path that ships.
#[test]
fn a_rejection_is_a_rejection_in_both() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Widget : OZObject { int _n; }
- (void)run;
@end
@implementation Widget
- (void)run {
	@try {
		_n = 1;
	} @catch (id e) {
	}
}
@end
int main(void) { return 0; }
"
    );
    let (single_diags, split_diags, _, _) = both(&src);
    assert!(!single_diags.is_empty(), "@try must be rejected by emit()");
    assert_eq!(
        single_diags, split_diags,
        "a construct outside the static subset must be refused identically:\n  \
         emit(): {:?}\n  emit_split(): {:?}",
        single_diags, split_diags
    );
}
