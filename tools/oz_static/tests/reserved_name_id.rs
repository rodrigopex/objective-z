// SPDX-License-Identifier: Apache-2.0
//
// reserved_name_id.rs - #317: `id` is a reserved word, so nothing may be
// *declared* with that name.
//
// The bug that motivated the rule was one position of many: a block
// parameter named `id` lowered to `uint8_t struct OZObject *` -- two type
// specifiers and no parameter name -- because `render_block` rewrote the
// parameter list as flat text and could not tell a parameter *typed* `id`
// from one *named* `id`. Reserving the name is what makes that
// undecidable-from-text distinction unnecessary.
//
// Two halves, and the second matters as much as the first: every position
// where the name is refused, and every position where `id` is still
// perfectly legal. `id` is a type, and member access reaches `.id` fields
// on foreign structs -- `imports::resolve_imports` never expands a plain
// `#include`, so a Zephyr struct's `.id` is not even visible to the bar.
// px-keyboard's `sAdvParam.id` depends on that staying true.

mod common;
use common::{compile_and_run, expect_reject, ozobject_src as PREAMBLE};

/// Every rejection carries the same actionable message.
fn assert_reserved(diags: &str) {
    assert!(diags.contains("'id' is a reserved word"), "diagnostics: {}", diags);
    assert!(diags.contains("rename it"), "diagnostics: {}", diags);
}

// ---------------------------------------------------------------------------
// Rejected: `id` in declarator position
// ---------------------------------------------------------------------------

/// The case from #317 itself. Reached `uint8_t struct OZObject *` before.
#[test]
fn block_literal_parameter_named_id_rejected() {
    let src = format!(
        "{}\n\
@interface Blk : OZObject
- (void)go;
@end

@implementation Blk
- (void)go {{
\tvoid (^b)(unsigned char) = ^(unsigned char id) {{
\t\t(void)id;
\t}};
\tb(1);
}}
@end
",
        PREAMBLE()
    );
    assert_reserved(&expect_reject(&src));
}

/// The second broken site, found while planning #317 rather than filed with
/// it: the nested parameter name inside a block-*typed* method parameter,
/// lowered by `render_param` through the same flat-text path.
#[test]
fn block_typed_parameter_nested_name_id_rejected() {
    let src = format!(
        "{}\n\
@interface Blk : OZObject
- (void)run:(void (^)(unsigned char id))b;
@end

@implementation Blk
- (void)run:(void (^)(unsigned char id))b {{
\t(void)b;
}}
@end
",
        PREAMBLE()
    );
    assert_reserved(&expect_reject(&src));
}

/// An Objective-C method parameter is `:(type)name`, whose name is a plain
/// identifier sibling of the `method_type` -- not a `parameter_declaration`,
/// so it needs its own arm in the check.
#[test]
fn method_parameter_named_id_rejected() {
    let src = format!(
        "{}\n\
@interface Meth : OZObject
- (void)withParam:(unsigned char)id;
@end

@implementation Meth
- (void)withParam:(unsigned char)id {{
\t(void)id;
}}
@end
",
        PREAMBLE()
    );
    assert_reserved(&expect_reject(&src));
}

#[test]
fn function_parameter_named_id_rejected() {
    let src = format!(
        "{}\n\
static void take(unsigned char id)
{{
\t(void)id;
}}

@interface Fn : OZObject
- (void)go;
@end

@implementation Fn
- (void)go {{
\ttake(1);
}}
@end
",
        PREAMBLE()
    );
    assert_reserved(&expect_reject(&src));
}

/// An ivar block is the one place the grammar reads the *name* as a type:
/// `unsigned char id;` parses as two stacked type specifiers with an empty
/// declarator, so there is no identifier node spelling `id` to find. The
/// check keys on the stacked specifiers instead.
#[test]
fn ivar_named_id_rejected() {
    let src = format!(
        "{}\n\
@interface Iv : OZObject {{
\tunsigned char id;
}}
- (void)go;
@end

@implementation Iv
- (void)go {{
}}
@end
",
        PREAMBLE()
    );
    assert_reserved(&expect_reject(&src));
}

/// A plain C struct spells its field name `field_identifier`, not
/// `identifier`.
#[test]
fn c_struct_field_named_id_rejected() {
    let src = format!(
        "{}\n\
struct thing {{
\tunsigned char id;
}};

@interface Cs : OZObject
- (void)go;
@end

@implementation Cs
- (void)go {{
}}
@end
",
        PREAMBLE()
    );
    assert_reserved(&expect_reject(&src));
}

#[test]
fn local_named_id_rejected() {
    let src = format!(
        "{}\n\
@interface Loc : OZObject
- (void)go;
@end

@implementation Loc
- (void)go {{
\tunsigned char id = 3;
\t(void)id;
}}
@end
",
        PREAMBLE()
    );
    assert_reserved(&expect_reject(&src));
}

/// The declared name is the *first* identifier in the declarator, so a
/// pointer or array declarator hides it one level down.
#[test]
fn pointer_and_array_declarators_named_id_rejected() {
    for decl in ["void *id", "unsigned char id[4]"] {
        let src = format!(
            "{}\n\
@interface Ptr : OZObject
- (void)go;
@end

@implementation Ptr
- (void)go {{
\t{};
\t(void)id;
}}
@end
",
            PREAMBLE(),
            decl
        );
        assert_reserved(&expect_reject(&src));
    }
}

/// Both names in one declaration, so the check cannot stop at the first
/// declarator it finds.
#[test]
fn second_declarator_named_id_rejected() {
    let src = format!(
        "{}\n\
@interface Two : OZObject
- (void)go;
@end

@implementation Two
- (void)go {{
\tint a = 1, id = 2;
\t(void)a;
\t(void)id;
}}
@end
",
        PREAMBLE()
    );
    assert_reserved(&expect_reject(&src));
}

// ---------------------------------------------------------------------------
// Accepted: `id` as a type, and `.id` as a member
// ---------------------------------------------------------------------------

/// `id` in *type* position, in the three places it can appear: an ivar's
/// type, a method parameter's type, and a block parameter's type. A lone
/// `typedefed_specifier` spelling `id` is a genuine type, which is what
/// separates `id _delegate;` from the rejected `unsigned char id;`.
///
/// The block literal is passed straight to a block-typed parameter, which
/// is `render_param`'s path. Routing it through a local block variable
/// instead exercises a different one, and used not to compile: see
/// `id_typed_block_variable_agrees_with_its_hoisted_function` below (#319).
#[test]
fn id_as_a_type_still_accepted() {
    let src = format!(
        "{}\n\
@interface Ty : OZObject {{
\t__unsafe_unretained id _delegate;
}}
- (void)take:(id)obj;
- (void)each:(void (^)(id))cb;
- (int)hits;
@end

@implementation Ty
- (void)take:(id)obj {{
\t_delegate = obj;
}}
- (void)each:(void (^)(id))cb {{
\tcb(_delegate);
}}
- (int)hits {{
\treturn _delegate != nil ? 1 : 0;
}}
@end

#include <stdio.h>

int main(void) {{
\tTy *t = [Ty alloc];
\t[t take:t];
\t[t each:^(id obj) {{
\t\t(void)obj;
\t}}];
\tprintf(\"hits=%d\\n\", [t hits]);
\t[t release];
\treturn 0;
}}
",
        PREAMBLE()
    );
    assert_eq!(compile_and_run(&src, "id_as_a_type_still_accepted"), "hits=1\n");
}

/// #319: a block *type*'s own parameter list has to spell an `id` parameter
/// the way `render_param` does, and it did not. Every shape below hoists a
/// function taking `struct OZObject *` while declaring the thing that holds
/// it as taking `id` -- `void *` -- so the initialization or the call was
/// "incompatible function pointer types ... with an expression of type
/// 'void (struct OZObject *)'".
///
/// Four shapes, because the `^` -> `*` lowering is spelled twice: in
/// `render_expr`'s `function_declarator` arm for a declarator inside a
/// method body, and as text edits in `block_pointer_edits` for one at file
/// scope. Neither half covers the other -- disabling either leaves two of
/// the four broken.
///
///   - `b`: a block variable in a method body, the case as filed
///   - `mixed`: an `id` in the middle of a list of plain scalars, so the
///     lowering neither misses it nor disturbs its neighbours -- and, since
///     #326, a *class*-typed neighbour beside it. The two promotions are
///     independent (`id` -> the root class pointer; `Blkv` -> `struct Blkv`)
///     and reach `apply_edits` as one merged edit list, so one list carrying
///     both is the case that says they do not truncate each other. Before
///     #326 a class-typed block parameter did not compile at all, which is
///     why this list was scalars-only when it was written
///   - `sHook`: a file-scope block variable
///   - `take_cb`: a free function's block-typed parameter, prototype and
///     definition both
///
/// Running it, not just compiling it, is what proves the two sides ended up
/// as the *same* type rather than merely two spellings the compiler
/// tolerated.
#[test]
fn id_typed_block_variable_agrees_with_its_hoisted_function() {
    let src = format!(
        "{}\n\
#include <stdio.h>

static int gSeen = 0;

static void (^sHook)(id) = ^(id o) {{
\tgSeen += o != 0 ? 100 : 0;
}};

static void take_cb(void (^cb)(id));

static void take_cb(void (^cb)(id))
{{
\tcb(0);
}}

@interface Blkv : OZObject {{
\t__unsafe_unretained id _delegate;
}}
- (void)hold:(id)obj;
- (int)run;
@end

@implementation Blkv
- (void)hold:(id)obj {{
\t_delegate = obj;
}}
- (int)run {{
\tvoid (^b)(id) = ^(id obj) {{
\t\tgSeen += obj != 0 ? 1 : 0;
\t}};
\tvoid (^mixed)(int, id, Blkv *, int) = ^(int seed, id obj, Blkv *owner, int bump) {{
\t\tgSeen += seed + (obj != 0 ? 4 : 0) + (owner != 0 ? 8 : 0) + bump;
\t}};
\tb(_delegate);
\tmixed(1, _delegate, self, 10);
\tsHook(_delegate);
\ttake_cb(^(id obj) {{
\t\tgSeen += obj == 0 ? 1000 : 0;
\t}});
\treturn gSeen;
}}
@end

int main(void) {{
\tBlkv *v = [Blkv alloc];
\t[v hold:v];
\tprintf(\"seen=%d\\n\", [v run]);
\t[v release];
\treturn 0;
}}
",
        PREAMBLE()
    );
    assert_eq!(
        compile_and_run(&src, "id_typed_block_variable_agrees_with_its_hoisted_function"),
        "seen=1124\n"
    );
}

/// The px-keyboard shape, and the reason C struct fields are the *only*
/// `.id` the bar can see: a plain `#include` is never expanded, so a
/// foreign struct's `id` field never reaches the CST. Declared here as an
/// opaque extern so the field is reached without declaring it locally --
/// exactly how `sAdvParam.id` reaches `struct bt_le_adv_param`.
#[test]
fn foreign_id_member_access_still_accepted() {
    let src = format!(
        "{}\n\
#include <stdio.h>

struct adv_param;
extern struct adv_param *adv_lookup(void);
extern unsigned char adv_read_id(struct adv_param *p);
extern void adv_write_id(struct adv_param *p, unsigned char v);

@interface Fgn : OZObject
- (int)run;
@end

@implementation Fgn
- (int)run {{
\tstruct adv_param *p = adv_lookup();
\tadv_write_id(p, 7);
\treturn (int)adv_read_id(p);
}}
@end

struct adv_param {{
\tunsigned char ident;
}};

static struct adv_param g_param = {{.ident = 0}};

struct adv_param *adv_lookup(void) {{
\treturn &g_param;
}}

unsigned char adv_read_id(struct adv_param *p) {{
\treturn p->ident;
}}

void adv_write_id(struct adv_param *p, unsigned char v) {{
\tp->ident = v;
}}

int main(void) {{
\tFgn *f = [Fgn alloc];
\tprintf(\"id=%d\\n\", [f run]);
\t[f release];
\treturn 0;
}}
",
        PREAMBLE()
    );
    assert_eq!(compile_and_run(&src, "foreign_id_member_access_still_accepted"), "id=7\n");
}

/// `id` as a substring is not the reserved word.
#[test]
fn names_containing_id_still_accepted() {
    let src = format!(
        "{}\n\
@interface Sub : OZObject
- (int)runWithIdx:(unsigned int)idx valid:(int)valid identity:(unsigned char)identity;
@end

@implementation Sub
- (int)runWithIdx:(unsigned int)idx valid:(int)valid identity:(unsigned char)identity {{
\treturn (int)idx + valid + (int)identity;
}}
@end

#include <stdio.h>

int main(void) {{
\tSub *s = [Sub alloc];
\tprintf(\"sum=%d\\n\", [s runWithIdx:1 valid:2 identity:3]);
\t[s release];
\treturn 0;
}}
",
        PREAMBLE()
    );
    assert_eq!(compile_and_run(&src, "names_containing_id_still_accepted"), "sum=6\n");
}
