// SPDX-License-Identifier: Apache-2.0
//
// default_description.rs -- `-getDescription:maxLength:` has a useful default
// (#354).
//
// `OZObject`'s implementation is the one every class inherits until it
// writes its own, and it used to be `return 0;` -- an undocumented no-op.
// So `%@` on any class without its own description produced an empty
// field, which is indistinguishable from a description that really is
// empty and from a formatting bug. It now writes
// `<ClassName: 0xADDRESS>`, the shape Objective-C's `-description`
// defaults to.
//
// Put in `OZObject`'s own method rather than injected into the protocol
// dispatch, deliberately: routing only the dispatch would leave a direct
// `[obj getDescription:buf maxLength:n]` send still answering 0, so the two
// spellings of one question would disagree. That is the same defect shape
// as #351 and #352, and the point of both fixes was to stop keying
// behaviour on which spelling the author used.
//
// The class's own name comes from `oz_static_class_name`, synthesized into
// the companion source where the class-id switch is available, and
// declared in `OZObject.h` -- not only in the companion header -- because
// `src/OZObject.m` calls it and that file is also compiled alone for the
// Clang AST dump, which never sees a generated header.
//
// Asserted through a *direct send* rather than through `OZLog`, because
// `src/OZLog.c` includes `zephyr/sys/printk.h` and cannot be linked on the
// host at all. The `%@` path itself is covered on target: built and run on
// `mps2/an385`, where it prints `plain=<Plain: 0x20001930>` while
// `@"hello"`, `@7` and `nil` are unchanged.
//
// The address makes the output nondeterministic, so every assertion here
// is on the prefix, the suffix and the reported length -- never on the
// whole string.

mod common;
use common::{compile_and_run, compile_and_run_with_cc_flags, ozobject_src as PREAMBLE};

fn program(body: &str) -> String {
    format!("/* oz-pool: Plain=1,Custom=1 */\n{}\n{}", PREAMBLE(), body)
}

/// The default names the class and its address, and reports exactly what
/// it wrote.
#[test]
fn the_default_description_names_the_class_and_its_address() {
    let src = program(
        "\
@interface Plain : OZObject
@end
@implementation Plain
@end

#include <stdio.h>
#include <string.h>

int main(void)
{
	char buf[64];
	int n = 0;
	Plain *p = [[Plain alloc] init];

	memset(buf, 0, sizeof(buf));
	n = [p getDescription:buf maxLength:sizeof(buf) - 1];

	/* Split so the assertions can be exact about everything except the
	 * address, which is not reproducible. */
	printf(\"len_matches=%d\\n\", n == (int)strlen(buf));
	printf(\"prefix=%d\\n\", strncmp(buf, \"<Plain: 0x\", 10) == 0);
	printf(\"suffix=%d\\n\", buf[strlen(buf) - 1] == '>');
	printf(\"has_digits=%d\\n\", (int)strlen(buf) > 11);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "default_description_plain");
    assert_eq!(
        stdout, "len_matches=1\nprefix=1\nsuffix=1\nhas_digits=1\n",
        "expected `<Plain: 0x...>` from the inherited default"
    );
}

/// A class that writes its own description still wins -- the default is a
/// default, not an override.
#[test]
fn a_class_with_its_own_description_is_unaffected() {
    let src = program(
        "\
@interface Custom : OZObject
@end
@implementation Custom
- (int)getDescription:(char *)buf maxLength:(size_t)maxLen
{
	const char *s = \"custom!\";
	size_t n = strlen(s);

	if (n > maxLen) {
		n = maxLen;
	}
	memcpy(buf, s, n);
	return (int)n;
}
@end

#include <stdio.h>
#include <string.h>

int main(void)
{
	char buf[64];
	int n = 0;
	Custom *c = [[Custom alloc] init];

	memset(buf, 0, sizeof(buf));
	n = [c getDescription:buf maxLength:sizeof(buf) - 1];
	printf(\"n=%d text=%s\\n\", n, buf);
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "default_description_override");
    assert_eq!(stdout, "n=7 text=custom!\n");
}

/// `maxLen` is a hard bound. A description cut short is cosmetic; one byte
/// past the end of a log buffer is not -- and this default writes into
/// whatever `OZLog` hands it, mid-format, with the remaining space as the
/// bound.
///
/// Run under the sanitizers in CI, where an overflow of the stack buffer
/// would be reported rather than merely mis-measured.
#[test]
fn the_default_never_writes_past_max_length() {
    let src = program(
        "\
@interface Plain : OZObject
@end
@implementation Plain
@end

#include <stdio.h>
#include <string.h>

int main(void)
{
	Plain *p = [[Plain alloc] init];
	size_t limit = 0;

	/* Every bound from 0 up to well past the full description, so the
	 * truncation is exercised at each byte boundary -- including in the
	 * middle of the class name and in the middle of the hex digits. */
	for (limit = 0; limit <= 32; limit++) {
		char buf[64];
		int n = 0;

		memset(buf, '#', sizeof(buf));
		n = [p getDescription:buf maxLength:limit];
		if (n < 0 || (size_t)n > limit) {
			printf(\"overran at limit=%zu with n=%d\\n\", limit, n);
			return 1;
		}
		/* Nothing beyond what it reported may have been touched. */
		if (buf[n] != '#') {
			printf(\"wrote past its own report at limit=%zu\\n\", limit);
			return 1;
		}
	}
	printf(\"bounded\\n\");
	return 0;
}
",
    );

    let stdout = compile_and_run(&src, "default_description_bounds");
    assert_eq!(stdout, "bounded\n");
}

/// The name lookup covers every class in the program, and answers for a
/// nil receiver rather than dereferencing it -- so a caller never has to
/// check first.
#[test]
fn the_class_name_lookup_covers_every_class() {
    let src = program(
        "\
@interface Plain : OZObject
@end
@implementation Plain
@end
",
    );

    let out = oz_static::transpile(&src).expect("should transpile");
    let c = &out.companion_c;
    assert!(
        c.contains("const char *oz_static_class_name(struct OZObject *self)"),
        "no class-name lookup was synthesized:\n{}",
        c
    );
    for class in ["OZObject", "Plain"] {
        assert!(
            c.contains(&format!("case OZ_STATIC_CLASS_{}: return \"{}\";", class, class)),
            "`{}` is missing from the class-name lookup:\n{}",
            class,
            c
        );
    }
    assert!(c.contains("return \"nil\";"), "a nil receiver is dereferenced:\n{}", c);
    assert!(
        out.companion_h.contains("const char *oz_static_class_name(struct OZObject *self);"),
        "the lookup is not declared in the companion header:\n{}",
        out.companion_h
    );
}

/// `CONFIG_OBJZ_DEFAULT_DESCRIPTION=n` restores the old no-op, for builds
/// that need the ~360 bytes back.
///
/// Reached by defining the macros the Kconfig would, since a host build
/// has no Kconfig at all: `OZ_DEFAULT_DESCRIPTION` is derived from
/// `CONFIG_OBJZ` being defined *without*
/// `CONFIG_OBJZ_DEFAULT_DESCRIPTION`, which is exactly the shape a Zephyr
/// build with the option off produces.
///
/// Worth a test rather than trusting the `#if`: a guarded-out branch is
/// code nothing compiles, and the last thing anyone notices is that the
/// disabled path stopped building -- or, worse, that the *enabled* path is
/// what the corpora were silently exercising all along.
#[test]
fn the_option_can_be_turned_off_and_restores_the_no_op() {
    let src = program(
        "\
@interface Plain : OZObject
@end
@implementation Plain
@end

#include <stdio.h>
#include <string.h>

int main(void)
{
	char buf[64];
	int n = 0;
	Plain *p = [[Plain alloc] init];

	memset(buf, '#', sizeof(buf));
	n = [p getDescription:buf maxLength:sizeof(buf) - 1];

	/* Nothing written, nothing reported, and the buffer untouched. */
	printf(\"n=%d untouched=%d\\n\", n, buf[0] == '#');
	return 0;
}
",
    );

    let stdout = compile_and_run_with_cc_flags(
        &src,
        "default_description_disabled",
        &["-DCONFIG_OBJZ=1"],
    );
    assert_eq!(
        stdout, "n=0 untouched=1\n",
        "with the option off the inherited description must write nothing at all"
    );
}
