// SPDX-License-Identifier: Apache-2.0
//
// behavior_immortal_literals.rs - objects that must never be passed to
// free(): a boxed string literal (`@"..."`), which lives in static storage,
// and a singleton, whose own protocol declares it immortal.
//
// Releasing a literal does happen in ordinary code: a collection that
// absorbed one releases its elements when it is itself deallocated, so a
// literal's refcount really does reach zero. `companion`'s release path
// calls `{class}_oz_free` at zero, which for OZString is `free(obj)` -- on a
// static, that aborts. `emit::render_boxed_string_literal` marks literals
// `_meta.immortal = 1`, and `oz_static_release` returns on that bit before
// it even decrements (#228).
//
// It used to mark them `_meta.deallocating = 1` from birth instead and rely
// on the re-entrancy guard, which sits *after* the decrement -- so the crash
// was avoided but the refcount sank through zero. That is the difference the
// refcount assertions below pin down; the abort-or-not cases pass under
// either mechanism.
//
// The abort asymmetry is what made a dictionary literal abort on release
// while an array literal released cleanly: dictionary *keys* here are string
// literals, whereas `@[ @10, @20 ]`'s elements are heap-allocated OZQ31
// boxes.

mod common;
use common::{
    compile_and_run, iterator_protocol_src, ozarray_src, ozdictionary_src, ozobject_src,
    ozq31_src, ozstring_src, singleton_protocol_src,
};

/// A class conforming to `SingletonProtocol`, in the shape the three real
/// singletons use (`samples/arc_demo`'s AppConfig, `samples/heap_alloc`'s
/// App, `samples/zbus_service`'s TemperatureService): built once and handed
/// out by `+sharedInstance`.
///
/// This now uses the real spelling -- a file-scope `static Config *_shared;`
/// assigned in `+initialize`. It could not, until #246: the single-file
/// emitter this harness drives had no `declaration` arm, so it copied
/// `static Config *_shared;` through untagged and the C compiler rejected it.
/// A method-local static stood in here in the meantime. Keeping the fixture
/// on the shape the samples actually use is the point -- a test that has to
/// avoid the production spelling is testing something adjacent to the truth.
fn singleton_decls() -> String {
    format!(
        "{}\n{}",
        singleton_protocol_src(),
        "\
@interface Config : OZObject <SingletonProtocol> {
	int _rate;
}
+ (instancetype)sharedInstance;
- (int)rate;
- (void)dealloc;
@end

static Config *_shared;

@implementation Config
+ (void)initialize
{
	_shared = [[Config alloc] init];
}
+ (instancetype)sharedInstance
{
	return _shared;
}
- (id)init
{
	self = [super init];
	if (self != nil) {
		_rate = 60;
	}
	return self;
}
- (int)rate
{
	return _rate;
}
- (void)dealloc
{
	printf(\"config dealloc\\n\");
}
@end
"
    )
}

#[test]
fn releasing_dictionary_literal_with_string_keys_does_not_abort() {
    let src = format!(
        "{}{}{}{}{}\n{}",
        ozobject_src(),
        iterator_protocol_src(),
        ozq31_src(),
        ozstring_src(),
        ozdictionary_src(),
        "\
#include <stdio.h>
int main(void) {
	OZDictionary *scores = @{ @\"alpha\" : @100, @\"beta\" : @200 };
	printf(\"count=%u\\n\", [scores count]);
	[scores release];
	printf(\"released_ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "releasing_dictionary_literal_with_string_keys");
    assert_eq!(stdout, "count=2\nreleased_ok\n");
}

#[test]
fn releasing_array_of_string_literals_does_not_abort() {
    let src = format!(
        "{}{}{}{}{}\n{}",
        ozobject_src(),
        iterator_protocol_src(),
        ozq31_src(),
        ozstring_src(),
        ozarray_src(),
        "\
#include <stdio.h>
int main(void) {
	OZArray *names = @[ @\"zephyr\", @\"objective-z\" ];
	printf(\"count=%u\\n\", [names count]);
	[names release];
	printf(\"released_ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "releasing_array_of_string_literals");
    assert_eq!(stdout, "count=2\nreleased_ok\n");
}

/// A literal released directly, to its refcount floor, then still used --
/// the storage must survive, since nothing may free it.
#[test]
fn literal_survives_release_to_zero() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        ozstring_src(),
        "\
#include <stdio.h>
int main(void) {
	OZString *s = @\"hello\";
	printf(\"len_before=%u\\n\", [s length]);
	[s release];
	printf(\"cstr_after=%s\\n\", [s cString]);
	printf(\"len_after=%u\\n\", [s length]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "literal_survives_release_to_zero");
    assert_eq!(stdout, "len_before=5\ncstr_after=hello\nlen_after=5\n");
}

/// Releasing a literal must not consume its refcount (#228).
///
/// This is the test that separates the two mechanisms. Marking a literal
/// `deallocating = 1` from birth also prevented the crash, so every test
/// above it passes either way -- but that guard sits *after* the decrement,
/// so the refcount really did sink to 0 and then below on each release. The
/// `immortal` check comes before the decrement, so an immortal object's
/// refcount never moves at all.
///
/// Without the fix this reports `rc_after=0`.
#[test]
fn releasing_a_literal_does_not_consume_its_refcount() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        ozstring_src(),
        "\
#include <stdio.h>
int main(void) {
	OZString *s = @\"hello\";
	printf(\"rc_before=%d\\n\", [s retainCount]);
	[s release];
	printf(\"rc_after=%d\\n\", [s retainCount]);
	[s release];
	[s release];
	printf(\"rc_settled=%d\\n\", [s retainCount]);
	printf(\"still=%s\\n\", [s cString]);
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "releasing_a_literal_does_not_consume_its_refcount");
    assert_eq!(stdout, "rc_before=1\nrc_after=1\nrc_settled=1\nstill=hello\n");
}

/// The literal carries `immortal`, and no longer lies with `deallocating`.
#[test]
fn literal_is_marked_immortal_rather_than_deallocating() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        ozstring_src(),
        "int main(void) { OZString *s = @\"hi\"; return [s length]; }\n"
    );
    let out = oz_static::transpile(&src).expect("should transpile").source_c;
    // The stem also carries an `extern struct OZString _oz_str_...;`
    // prototype, so the initializer is what identifies the definition.
    let def = out
        .lines()
        .find(|l| l.contains("struct OZString _oz_str_") && l.contains("._meta"))
        .unwrap_or_else(|| panic!("no hoisted literal definition in:\n{}", out));
    assert!(def.contains(".immortal = 1"), "literal must be immortal; got:\n{}", def);
    assert!(
        !def.contains(".deallocating"),
        "`deallocating` means teardown is running, not never-teardown; got:\n{}",
        def
    );
}

/// Order is the whole point: the immortal check has to precede the
/// decrement, or the refcount is still consumed on the way past it.
#[test]
fn release_checks_immortal_before_decrementing() {
    let src = format!("{}\n{}", ozobject_src(), "int main(void) { return 0; }\n");
    let c = oz_static::transpile(&src).expect("should transpile").companion_c;
    let start = c
        .find("void oz_static_release(")
        .unwrap_or_else(|| panic!("no oz_static_release in:\n{}", c));
    let body = &c[start..];
    let immortal = body.find("_meta.immortal").expect("release must check immortal");
    let dec = body.find("oz_atomic_dec_and_test").expect("release must decrement");
    assert!(
        immortal < dec,
        "immortal check must come before the decrement; got:\n{}",
        &body[..body.find("switch").unwrap_or(body.len())]
    );
}

/// A singleton is immortal too (#228). `Singleton+Protocol.h` states the
/// contract -- "Singleton objects are immortal, they are never deallocated"
/// -- but until now nothing marked them, so it held only because no code
/// happened to release one. Releasing one must not run `-dealloc`, and must
/// not hand its slab slot back while `+sharedInstance` still points at it.
///
/// Without the fix this prints `config dealloc` and reports `rate_after=0`.
#[test]
fn releasing_a_singleton_does_not_deallocate_it() {
    let src = format!(
        "{}{}\n{}",
        ozobject_src(),
        singleton_decls(),
        "\
int main(void) {
	Config *c = [Config sharedInstance];
	printf(\"rate_before=%d\\n\", [c rate]);
	[c release];
	printf(\"rate_after=%d\\n\", [[Config sharedInstance] rate]);
	printf(\"rc=%d\\n\", [c retainCount]);
	printf(\"done\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "releasing_a_singleton_does_not_deallocate_it");
    assert_eq!(stdout, "rate_before=60\nrate_after=60\nrc=1\ndone\n");
}

/// The marker goes in the allocator, so it is set however the instance was
/// made -- and it is keyed on conformance, so an ordinary class next to it
/// in the same program is untouched.
#[test]
fn only_singleton_conformers_are_marked_immortal() {
    let src = format!(
        "{}{}{}\n{}",
        ozobject_src(),
        singleton_decls(),
        "\
@interface Plain : OZObject {
	int _x;
}
@end
@implementation Plain
@end
",
        "int main(void) { return 0; }\n"
    );
    let out = oz_static::transpile(&src).expect("should transpile");
    let all = format!("{}{}", out.source_c, out.companion_c);

    let alloc_body = |name: &str| -> String {
        let sig = format!("struct {name} *{name}_oz_alloc(void)\n{{", name = name);
        let start = all
            .find(&sig)
            .unwrap_or_else(|| panic!("no {}_oz_alloc in:\n{}", name, all));
        let rest = &all[start..];
        let end = rest.find("\n}").map(|e| e + 2).unwrap_or(rest.len());
        rest[..end].to_string()
    };

    let config = alloc_body("Config");
    assert!(
        config.contains("_meta.immortal = 1"),
        "a SingletonProtocol conformer must be marked immortal; got:\n{}",
        config
    );
    let plain = alloc_body("Plain");
    assert!(
        !plain.contains("_meta.immortal"),
        "an ordinary class must not be marked immortal; got:\n{}",
        plain
    );
}

/// A heap-allocated OZString (not a literal) must still be freed
/// normally -- the immortality marker applies only to literals.
#[test]
fn heap_allocated_object_still_freed_normally() {
    let src = format!(
        "{}{}",
        ozobject_src(),
        "\
@interface Counted : OZObject {
	int _n;
}
- (int)n;
@end
@implementation Counted
- (int)n {
	return _n;
}
@end

#include <stdio.h>
int main(void) {
	Counted *c = [Counted alloc];
	printf(\"rc=%d\\n\", [c retainCount]);
	[c retain];
	printf(\"rc2=%d\\n\", [c retainCount]);
	[c release];
	printf(\"rc3=%d\\n\", [c retainCount]);
	[c release];
	printf(\"freed_ok\\n\");
	return 0;
}
"
    );
    let stdout = compile_and_run(&src, "heap_allocated_object_still_freed_normally");
    assert_eq!(stdout, "rc=1\nrc2=2\nrc3=1\nfreed_ok\n");
}
