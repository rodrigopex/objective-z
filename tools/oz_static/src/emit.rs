// SPDX-License-Identifier: Apache-2.0
//
// emit.rs - in-place textual substitution emitter, with a literate output
// goal: every generated line should be traceable to source. ObjC-specific
// syntax spans are replaced at their original byte position, decorated
// with the original text as a comment (a banner for @interface/
// @implementation boundaries, a one-line `/* ... */` above each
// translated top-level statement/declaration/definition); anything that
// didn't need translation stays byte-identical, no comment noise. Multi-
// implementor dispatch (dealloc's const-vtable) and pool registration are
// isolated into one small generated companion file, mirroring the
// existing oz_dispatch.c/h pattern.
//
// The substitution is per *construct*: `rebuild` and `apply_edits` replace
// spans inside one top-level node and copy the gaps between them verbatim.
// The top level itself is assembled from what `walk_top_level` buckets, not
// patched over the whole file -- `emit()` did once work that way, and its
// doing so is how it managed to disagree with `emit_split()` four times
// (#254): anything no arm claimed simply survived, so a missing arm produced
// no error and no output difference until a C compiler saw it.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use tree_sitter::Node;

use crate::model::{Diagnostic, Program};
use crate::parse::line_col;

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.start_byte()..node.end_byte()]
}

/// Collapse whitespace (including newlines) into single spaces, for a
/// readable one-line `/* ... */` comment out of a possibly multi-line or
/// oddly-indented original statement. Every caller wraps the result in
/// `/* ... */`, so any embedded comment delimiter in the original text (a
/// real inline comment, or even a string literal containing those two
/// characters) is neutralized here -- C block comments don't nest.
fn one_line(text: &str) -> String {
    neutralize_comment_delimiters(&text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Make `text` safe to place inside a `/* ... */` block comment.
///
/// Both delimiters have to go, not just the closing one. `*/` left as-is
/// closes the wrapping comment early and hands the rest to the compiler as
/// live code -- the obvious hazard, and the only one this used to handle.
/// But a surviving `/*` is a diagnostic in its own right: Clang and GCC both
/// warn "'/*' within block comment" under `-Wall`, and Zephyr builds with
/// `-Werror`, so echoing a source comment into a banner was enough to fail
/// the build. 36 of those came from `OZQ31.h`'s ivar doc comments alone.
fn neutralize_comment_delimiters(text: &str) -> String {
    text.replace("*/", "* /").replace("/*", "/ *")
}

const BANNER_WIDTH: usize = 80;

fn rule_fill(width: usize, fill: char) -> String {
    fill.to_string().repeat(width)
}

/// A "boxed" banner opening a section: a top rule, `content` (verbatim,
/// possibly multi-line -- e.g. an interface header through its ivars
/// block) with every line prefixed `" * "`, and a bottom rule -- the
/// classic C block-comment box, so a section boundary is unmistakable at
/// a glance regardless of how much header text it wraps.
///
/// `content` is real source text and may itself contain a `/* ... */`
/// comment (e.g. an ivar's own inline doc comment) -- C block comments
/// don't nest, so an embedded `*/` would otherwise close this banner
/// early, leaving the rest of it (and whatever real code follows) to be
/// parsed as live C, and a surviving `/*` warns under `-Wall`. Both
/// delimiters are neutralized before wrapping -- see
/// `neutralize_comment_delimiters`; cosmetic only, since this text is
/// documentation either way.
fn banner_box(content: &str, fill: char) -> String {
    let content = neutralize_comment_delimiters(content.trim_end());
    let mut out = format!("/* {}\n", rule_fill(BANNER_WIDTH - 3, fill));
    for line in content.lines() {
        out.push_str(" * ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(" * ");
    out.push_str(&rule_fill(BANNER_WIDTH.saturating_sub(6), fill));
    out.push_str(" */\n");
    out
}

/// A single-line centered rule closing a section: "/*== label ==*/"
/// padded to BANNER_WIDTH. Deliberately lighter than `banner_box` -- the
/// open announces a section and carries its source header; the close
/// just marks where it ends.
fn banner_rule(label: &str, fill: char) -> String {
    let text = format!(" {} ", label);
    let total = BANNER_WIDTH.saturating_sub(4 + text.len());
    let left = total / 2;
    let right = total - left;
    format!("/*{}{}{}*/\n", rule_fill(left, fill), text, rule_fill(right, fill))
}

/// The verbatim source text of a `class_interface`/`class_implementation`
/// node up to (not including) its first declaration/definition/`@end` --
/// i.e. just the header (name, superclass, category, ivars block for
/// interfaces), trimmed of trailing whitespace.
fn header_text(node: Node, src: &str, stop_kinds: &[&str]) -> String {
    let mut cursor = node.walk();
    let end = node
        .children(&mut cursor)
        .find(|c| stop_kinds.contains(&c.kind()) || c.kind() == "@end")
        .map(|c| c.start_byte())
        .unwrap_or(node.end_byte());
    src[node.start_byte()..end].trim_end().to_string()
}

/// Pre-scan a body for every local `declaration` and record its
/// (name -> c_type) into `ctx.scope`, so identifiers resolve to a static
/// type regardless of where in the body they're declared relative to
/// where they're used (C requires declare-before-use, but this scan
/// doesn't need to respect that ordering to build the lookup table).
/// Does not descend into block_literal bodies (a separate lexical scope).
/// Build the scope table for `body`, then decide which of its object locals
/// ARC manages as strong variables. Both passes are driven from here so that
/// every caller -- method bodies and the two plain-C-function arms alike --
/// gets the second one; an earlier shape had the strong-local decision at
/// each call site, where a new call site would silently miss it.
fn collect_local_decls(body: Node, ctx: &mut EmitCtx) {
    collect_local_decls_inner(body, ctx);
    let managed = managed_object_locals(body, ctx.src, ctx.program);
    ctx.arc_managed_locals.extend(managed);
}

fn collect_local_decls_inner(node: Node, ctx: &mut EmitCtx) {
    if node.kind() == "block_literal" {
        return;
    }
    if node.kind() == "declaration" {
        // NOTE: extract_type_and_stars walks the whole declaration subtree,
        // so it already picks up '*' tokens from inside the declarator(s).
        // This means a multi-declarator line (`int *a, b;`) would
        // incorrectly give both the same star count -- a known spike
        // limitation; every test/sample uses one declarator per line.
        let known: std::collections::HashSet<String> = ctx.program.classes.keys().cloned().collect();
        let (type_text, stars) = crate::collect::extract_type_and_stars(node, ctx.src);
        let c_type = crate::collect::render_type(&type_text, stars, &known);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // `pointer_declarator` is the shape a declaration with *no*
            // initializer takes -- `Counter *c;` parses as
            // `type_identifier` + `pointer_declarator(* identifier)`, with
            // no `init_declarator` anywhere. Without it such a local never
            // reached `ctx.scope`, so a later `[c poke]` reported its
            // receiver as `id` and was rejected, while the identical code
            // written `Counter *c = ...;` resolved fine. That is the
            // local-scope twin of the file-scope gap fixed by
            // `emit::file_scope_vars`, and it also has to be fixed here for
            // ARC to recognise a strong local declared before the loop that
            // assigns it (see `arc_managed_locals`).
            if child.kind() == "init_declarator"
                || child.kind() == "identifier"
                || child.kind() == "pointer_declarator"
            {
                let name = crate::collect::find_declared_name(child, ctx.src);
                if !name.is_empty() {
                    ctx.scope.insert(name.clone(), c_type.clone());
                    ctx.locals.insert(name);
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_local_decls_inner(child, ctx);
    }
}

/// Is `decl` a declaration of exactly one strong local, written with no
/// initializer? Those need ARC's implicit `= nil` -- see the `"declaration"`
/// arm in `render_expr`.
///
/// The single-declarator requirement is not cosmetic: the initializer is
/// spliced in before the declaration's trailing `;`, which on
/// `Counter *a, *b;` would initialize only `b` and leave `a` indeterminate
/// -- exactly the pointer the first overwrite would then release. A
/// multi-declarator line is therefore left alone rather than half-handled,
/// which also keeps it out of `arc_managed_locals`' release paths (see
/// `owned_locals_of_in`, which is reached through the same declarator kind).
fn declares_bare_managed_local(decl: Node, ctx: &EmitCtx) -> bool {
    let mut cursor = decl.walk();
    let declarators: Vec<Node> = decl
        .children(&mut cursor)
        .filter(|c| {
            matches!(
                c.kind(),
                "pointer_declarator" | "init_declarator" | "array_declarator" | "identifier"
            )
        })
        .collect();
    if declarators.len() != 1 || declarators[0].kind() != "pointer_declarator" {
        return false;
    }
    let name = crate::collect::find_declared_name(declarators[0], ctx.src);
    !name.is_empty() && ctx.arc_managed_locals.contains(&name)
}

/// Is this initializer just "nothing yet" -- `nil`, `NULL` or `0`?
///
/// `Foo *f = nil;` followed by real assignments is an everyday Objective-C
/// idiom and means exactly what a bare `Foo *f;` means: the variable starts
/// empty. Both must be treated the same, or the explicit spelling would
/// silently lose ARC while the implicit one kept it. It also keeps sources
/// portable to the Python pipeline, which cannot emit the implicit nil at all
/// (`OZ003: unhandled AST node 'ImplicitValueInitExpr'`) and so needs the
/// explicit form.
///
/// `oz_static_release` is null-safe, so a first overwrite releasing this is a
/// no-op either way.
fn is_null_initializer(node: Node, src: &str) -> bool {
    let text = node_text(node, src).trim();
    matches!(text, "0" | "nil" | "NULL" | "((id)0)" | "(id)0")
}

/// `(void)name;` acknowledgements for every parameter a translated method body
/// never mentions, innermost-scope first in signature order.
///
/// Zephyr's own warning set does not include `-Wextra`, so an unused parameter
/// is not a build failure -- but it is noise that hides the next real warning,
/// and three of the four defects gap M found were only visible because someone
/// counted warnings by kind. 58 of these across the samples made that counting
/// harder than it should be.
///
/// The same acknowledgement the SDK's own C already uses: `(void)inner;` in
/// `oz_platform.h`'s heap stubs, `(void)expr;` in `oz_sdk/assert.h`.
///
/// Decided from the **rendered** body -- the C a compiler will actually see --
/// not from the Objective-C source it came from. That distinction is not
/// pedantic: an ivar reference like `_n` lowers to `self->_n`, so a method
/// whose source never writes the word `self` can still use the parameter.
/// Checking the source marked `- (int)useAll:… { return a + b + _n; }` as not
/// using `self` and emitted a redundant `(void)self;` for it.
///
/// `self` is included at all because an empty `-dealloc` is idiomatic
/// Objective-C, so the warning fires on entirely correct code.
///
/// Word-boundary matched, so `_next` does not count as a use of `n`. The
/// rendered body also carries the per-statement source comments, so a name a
/// comment mentions but the code does not is treated as used and keeps its
/// warning -- the safe direction, and the only inaccuracy left: a false "used"
/// leaves a warning in place, while a false "unused" would emit a redundant
/// `(void)x;`, which is valid C either way. Neither can change behaviour.
fn unused_param_acks(
    rendered_body: &str,
    params: &[(String, String)],
    is_class_method: bool,
) -> Vec<String> {
    let mut names: Vec<&str> = Vec::new();
    if !is_class_method {
        names.push("self");
    }
    for (pname, _) in params {
        names.push(pname.as_str());
    }
    acks_for_names(rendered_body, &names)
}

/// The name-list form, shared with `render_block`: a hoisted block literal is
/// a function oz_static synthesizes outright, signature included, so its own
/// unused parameters are its to acknowledge. `samples/gpio_demo`'s
/// `blockCallback:^(const struct device *port, struct gpio_callback *cb,
/// gpio_port_pins_t pins)` accounts for three of them and
/// `transpiled_generics`'s `^(id obj, unsigned int idx, BOOL *stop)` for a
/// fourth.
fn acks_for_names(rendered_body: &str, names: &[&str]) -> Vec<String> {
    fn mentions_word(haystack: &str, name: &str) -> bool {
        let bytes = haystack.as_bytes();
        let mut from = 0;
        while let Some(rel) = haystack[from..].find(name) {
            let start = from + rel;
            let end = start + name.len();
            let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
            let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
            if before_ok && after_ok {
                return true;
            }
            from = start + 1;
        }
        false
    }
    fn is_ident_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }

    names
        .iter()
        .filter(|name| !name.is_empty() && !mentions_word(rendered_body, name))
        .map(|name| format!("\t(void){};", name))
        .collect()
}

/// Parameter names declared by a `parameter_list`, in order. An unnamed or
/// `void` parameter yields nothing.
fn parameter_list_names(plist: Node, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = plist.walk();
    for child in plist.children(&mut cursor) {
        if child.kind() != "parameter_declaration" {
            continue;
        }
        // The *last* identifier in the subtree is the declared name: the
        // earlier ones belong to the type (`const struct device *port` has
        // `device` before `port`).
        let mut found: Option<String> = None;
        fn walk(node: Node, src: &str, found: &mut Option<String>) {
            if node.kind() == "identifier" {
                *found = Some(node_text(node, src).to_string());
            }
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                walk(ch, src, found);
            }
        }
        walk(child, src, &mut found);
        if let Some(name) = found {
            out.push(name);
        }
    }
    out
}

/// Splice `lines` in directly after a rendered body's opening `{`.
///
/// The body always starts with `{` (`render_body_with_comments` builds it that
/// way, and the untranslated fallback is the source text of a
/// `compound_statement`). If it somehow does not, the body is returned
/// unchanged rather than corrupted -- an acknowledgement is a nicety, and
/// mangling a body to add one would not be.
fn splice_after_open_brace(body_text: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return body_text.to_string();
    }
    match body_text.find('{') {
        Some(i) => {
            let (head, tail) = body_text.split_at(i + 1);
            // A single-line body (`{ return a; }`) leaves the next statement
            // sharing the last acknowledgement's line, so start a fresh one.
            // Generated C is read by people; `(void)b; return a; }` on one line
            // is valid and unpleasant.
            if tail.starts_with('\n') {
                format!("{}\n{}{}", head, lines.join("\n"), tail)
            } else {
                format!("{}\n{}\n\t{}", head, lines.join("\n"), tail.trim_start())
            }
        }
        None => body_text.to_string(),
    }
}

/// Does `root`'s subtree read the identifier `name`?
fn references_identifier(name: &str, root: Node, src: &str) -> bool {
    if root.kind() == "identifier" && node_text(root, src) == name {
        return true;
    }
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    children.into_iter().any(|child| references_identifier(name, child, src))
}

/// How a single store to a strong local can be emitted.
#[derive(PartialEq, Clone, Copy)]
enum LocalStore {
    /// A `+1` right-hand side that does not mention the variable: the old
    /// value can be released *before* evaluating it.
    Owning,
    /// A plain identifier: free of side effects, so it can be named twice
    /// and retained before the release, which is what makes `c = c` safe.
    BorrowedIdent,
    /// Anything else -- a `+0` call, or a `+1` one that reads the variable
    /// it is about to overwrite. Both would need a temporary, and a
    /// temporary cannot be placed correctly here (see
    /// `render_strong_local_assign`), so a local with any such store is not
    /// managed at all.
    Unsupported,
}

fn classify_store(name: &str, rhs: Node, src: &str, program: &Program) -> LocalStore {
    let owning = crate::arc::is_owning_expr(rhs, src, program, &program.owning_methods);
    let mentions_self = references_identifier(name, rhs, src);
    if owning && !mentions_self {
        return LocalStore::Owning;
    }
    if rhs.kind() == "identifier" {
        return LocalStore::BorrowedIdent;
    }
    LocalStore::Unsupported
}

/// Every store to `name` under `root`, in source order.
fn stores_to_local(
    name: &str,
    root: Node,
    src: &str,
    program: &Program,
    out: &mut Vec<LocalStore>,
) {
    if root.kind() == "assignment_expression" {
        let mut cursor = root.walk();
        let parts: Vec<Node> = root.children(&mut cursor).collect();
        if parts.len() >= 3 && parts[0].kind() == "identifier" && node_text(parts[0], src) == name {
            let op = node_text(parts[1], src);
            if op == "=" {
                out.push(classify_store(name, *parts.last().unwrap(), src, program));
            } else {
                // A compound store (`|=` and friends) on an object local is
                // not something ARC can reason about.
                out.push(LocalStore::Unsupported);
            }
        }
    }
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    for child in children {
        stores_to_local(name, child, src, program, out);
    }
}

/// Record which object locals declared under `body` are strong locals ARC
/// manages -- see `EmitCtx::arc_managed_locals`.
///
/// The membership rule is deliberately narrow, because the two halves of
/// ownership have to match exactly: an overwrite may release the previous
/// value only if that value was itself owned. Releasing a reference never
/// taken is a double free, which is precisely the bug gap L was (the
/// release half of strong-ivar ownership shipped without the retain half).
/// So a local qualifies only when every value it can hold is owned:
///
/// - **Bare declaration** (`Counter *c;`) with at least one `+1` assignment.
///   Every assignment then goes through `render_strong_local_assign`, which
///   retains a borrowed right-hand side, so all values are owned. The
///   declaration is also given ARC's implicit `= nil`, without which the
///   first overwrite would release an indeterminate pointer.
/// - **Owning initializer** (`Counter *c = [Counter alloc];`) -- already
///   owned and already released at scope exit today; all this adds is
///   release-on-overwrite.
///
/// A **borrowed initializer** (`Counter *c = [arr objectAtIndex:0];`) is
/// excluded. Real ARC would retain it, but oz_static does not, so its value
/// is unowned and releasing it on overwrite would be that same double free.
/// Making those strong is a larger change to observable refcounts and is not
/// what this fix is for.
///
/// A local the body releases by hand is excluded throughout, keeping the
/// standing rule that ARC defers to manual retain/release -- see
/// `released_by_hand`.
pub(crate) fn managed_object_locals(
    body: Node,
    src: &str,
    program: &Program,
) -> std::collections::HashSet<String> {
    fn walk(
        node: Node,
        body: Node,
        found: &mut Vec<String>,
        src: &str,
        program: &Program,
    ) {
        if node.kind() == "declaration" && !is_block_qualified_declaration(node, src) {
            // `extract_type_and_stars` yields the *source* spelling, so an
            // object type is just the class name -- no `struct` prefix to
            // strip, and `is_class` is the authority on whether the name is
            // a class at all rather than a plain C struct.
            let (type_text, stars) = crate::collect::extract_type_and_stars(node, src);
            let is_object = stars == 1 && program.is_class(type_text.trim());
            if is_object && !node_text(node, src).contains("__unsafe_unretained") {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    let bare = child.kind() == "pointer_declarator";
                    let init = child.kind() == "init_declarator";
                    if !bare && !init {
                        continue;
                    }
                    let name = crate::collect::find_declared_name(child, src);
                    if name.is_empty() || released_by_hand(&name, body, src) {
                        continue;
                    }
                    // Every store has to be one the renderer can emit, or
                    // the variable would be managed for some assignments and
                    // not others -- and then the scope-exit release could
                    // free a value nothing ever retained.
                    let mut stores = Vec::new();
                    stores_to_local(&name, body, src, program, &mut stores);
                    if stores.contains(&LocalStore::Unsupported) {
                        continue;
                    }
                    // An initializer that is just `nil`/`0` leaves the
                    // variable empty, which is what a bare declaration means
                    // too -- so the two are decided by the same rule.
                    let starts_empty = if bare {
                        true
                    } else {
                        let mut c2 = child.walk();
                        let parts: Vec<Node> = child.children(&mut c2).collect();
                        let eq = parts.iter().position(|n| n.kind() == "=");
                        eq.and_then(|i| parts.get(i + 1))
                            .copied()
                            .is_some_and(|v| is_null_initializer(v, src))
                    };
                    if starts_empty {
                        // Strong only if something owned is ever stored. A
                        // local that only ever receives borrowed values is
                        // left alone: retaining those is a broader change to
                        // observable refcounts than this fix is for.
                        if stores.contains(&LocalStore::Owning) {
                            found.push(name);
                        }
                    } else {
                        // An owning initializer is already owned today; a
                        // borrowed one is deliberately left alone.
                        let mut c2 = child.walk();
                        let parts: Vec<Node> = child.children(&mut c2).collect();
                        let eq = parts.iter().position(|n| n.kind() == "=");
                        let Some(value) = eq.and_then(|i| parts.get(i + 1)).copied() else {
                            continue;
                        };
                        if crate::arc::is_owning_expr(
                            value,
                            src,
                            program,
                            &program.owning_methods,
                        ) {
                            found.push(name);
                        }
                    }
                }
            }
        }
        // A block literal has its own scope and its own locals; those are
        // not this body's to manage.
        if node.kind() == "block_literal" {
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, body, found, src, program);
        }
    }

    let mut found = Vec::new();
    walk(body, body, &mut found, src, program);
    found.into_iter().collect()
}

/// Does `node` (a `declaration`) carry a `__block` `type_qualifier` child?
/// tree-sitter-objc has no dedicated node kind for `__block` -- confirmed
/// against the vendored 3.0.2 grammar, it's one of the string choices
/// inside the `type_qualifier` rule -- so it shows up as an ordinary
/// `type_qualifier` child whose text happens to be `__block`. Same test
/// `staticbar.rs`'s capture check uses to exempt these locals.
fn is_block_qualified_declaration(node: Node, src: &str) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|c| c.kind() == "type_qualifier" && node_text(c, src) == "__block");
    found
}

/// Promote a `__block`-qualified local to a file-scope static, mirroring
/// oz_transpile's collect.py `_collect_block_vars`/emit.py:1179-1181: the
/// local declaration is skipped entirely (its statement renders to an
/// empty string -- see the `render_expr` "declaration" arm) and a
/// `static TYPE name [= init];` line is queued in `ctx.hoisted_statics`
/// for `emit()`/`emit_split()` to splice in at file scope, right beside
/// the other hoisted-* vectors. Every reference to `name`, inside the
/// block or out, resolves to the same static via plain C lexical scoping
/// -- no renaming needed.
///
/// Only a simple literal initializer (`_extract_init_value`'s Python
/// equivalent) is preserved; anything else is dropped and the static is
/// declared uninitialized, exactly like the Python oracle.
fn hoist_block_var(node: Node, ctx: &mut EmitCtx) {
    let known: std::collections::HashSet<String> = ctx.program.classes.keys().cloned().collect();
    let (type_text, stars) = crate::collect::extract_type_and_stars(node, ctx.src);
    let c_type = crate::collect::render_type(&type_text, stars, &known);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        // `pointer_declarator` covers the one shape the other two miss: a
        // *pointer* declared with no initializer. `__block int q;` was already
        // handled, because a bare non-pointer declarator is itself an
        // `identifier` -- which is exactly why this went unnoticed. Only
        // `__block Foo *p;` fell through, and then nothing was hoisted at all,
        // leaving the block referencing a name that was not there.
        if !matches!(child.kind(), "init_declarator" | "identifier" | "pointer_declarator") {
            continue;
        }
        let name = crate::collect::find_declared_name(child, ctx.src);
        if name.is_empty() {
            continue;
        }
        let decl_str = format!("{} {}", c_type, name);
        let init = child.child_by_field_name("value").and_then(|v| simple_literal_text(v, ctx.src));
        let decl = match init {
            Some(init_text) => format!("static {} = {};", decl_str, init_text),
            None => format!("static {};", decl_str),
        };
        ctx.hoisted_statics.push((name, decl));
    }
}

/// Mirrors collect.py's `_extract_init_value`: only a bare number literal
/// (optionally negated) or a null pointer constant survives the promotion
/// to a file-scope static initializer. Anything more complex (a call, an
/// identifier, an arithmetic expression) is dropped, same as the Python
/// oracle -- the static ends up declared with no initializer at all.
fn simple_literal_text(node: Node, src: &str) -> Option<String> {
    match node.kind() {
        "number_literal" => Some(node_text(node, src).to_string()),
        "unary_expression" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            if !children.iter().any(|c| c.kind() == "-") {
                return None;
            }
            let operand = children.into_iter().find(|c| c.kind() == "number_literal")?;
            Some(format!("-{}", node_text(operand, src)))
        }
        "parenthesized_expression" => {
            let mut cursor = node.walk();
            let inner = node.children(&mut cursor).find(|c| c.kind() != "(" && c.kind() != ")")?;
            simple_literal_text(inner, src)
        }
        "identifier" if matches!(node_text(node, src), "NULL" | "nil" | "Nil") => {
            Some("NULL".to_string())
        }
        _ => None,
    }
}

pub(crate) fn selector_to_c(selector: &str) -> String {
    selector.replace(':', "_")
}

/// Render one `(name, c_type)` parameter as C text. Most types are prefix
/// style (`TYPE NAME`), but a function-pointer type needs the name embedded
/// mid-declarator (`RET (*NAME)(ARGS)`) -- `detect_block_param_type` signals
/// that by leaving `PARAM_NAME_PLACEHOLDER` in the type text.
pub(crate) fn render_param(ptype: &str, pname: &str, root: Option<&str>) -> String {
    if ptype.contains(crate::collect::PARAM_NAME_PLACEHOLDER) {
        // A function-pointer parameter: its own parameter list came through
        // verbatim from source (`collect::detect_block_param_type`), so an
        // `id` in it is still spelled `id`. It has to become the root class
        // pointer, for the same reason a function-pointer *ivar*'s does --
        // see `collect_ivar_lowering_edits`. The two must agree: an
        // `-initWithBlock:` parameter is assigned straight into the matching
        // field, and with only the field lowered the assignment itself
        // stopped compiling.
        let rendered = ptype.replace(crate::collect::PARAM_NAME_PLACEHOLDER, pname);
        return match root {
            Some(root) => replace_bare_id(&rendered, &format!("struct {} *", root)),
            None => rendered,
        };
    }
    format!("{} {}", ptype, pname)
}

/// Replace `id` where it stands as a whole word, leaving `id`-containing
/// identifiers (`idx`, `valid`, a parameter actually named `id`) alone.
fn replace_bare_id(text: &str, replacement: &str) -> String {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(text.len());
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let at_word = bytes[i] == 'i'
            && i + 1 < bytes.len()
            && bytes[i + 1] == 'd'
            && (i == 0 || !is_ident(bytes[i - 1]))
            && (i + 2 >= bytes.len() || !is_ident(bytes[i + 2]));
        if at_word {
            out.push_str(replacement);
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Class methods get a `_cls` suffix so `+foo` and `-foo` on the same
/// class never collide on the same C function name.
pub(crate) fn method_fn_name(class_name: &str, selector: &str, is_class_method: bool) -> String {
    if is_class_method {
        format!("{}_{}_cls", class_name, selector_to_c(selector))
    } else {
        format!("{}_{}", class_name, selector_to_c(selector))
    }
}

/// Note this is purely a spelling transform: it says nothing about whether
/// the name is a *class*. Every plain C struct type is spelled `struct Foo`
/// too, so a caller about to treat the result as a class must ask
/// `Program::is_class` as well.
fn class_name_from_type(t: &str) -> Option<String> {
    let t = t.trim();
    let rest = t.strip_prefix("struct ")?;
    let name = rest.trim_end_matches('*').trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn find_defining_class(
    program: &Program,
    start: &str,
    selector: &str,
    is_class_method: bool,
) -> Option<String> {
    let mut cur = Some(start.to_string());
    while let Some(name) = cur {
        let info = program.classes.get(&name)?;
        if info.methods.iter().any(|m| m.selector == selector && m.is_class_method == is_class_method) {
            return Some(name);
        }
        cur = info.superclass.clone();
    }
    None
}

/// Returns the method's `(return_type, returns_instancetype)`. A caller
/// dispatching this method through a receiver statically typed as a
/// *subclass* of `class_name` must, when `returns_instancetype` is true,
/// report and cast to the subclass's own pointer type instead of this
/// literal `return_type` -- see `MethodSig::returns_instancetype`.
fn method_return_type(
    program: &Program,
    class_name: &str,
    selector: &str,
    is_class_method: bool,
) -> Option<(String, bool)> {
    program.classes.get(class_name)?.methods.iter().find(|m| {
        m.selector == selector && m.is_class_method == is_class_method
    }).map(|m| (m.return_type.clone(), m.returns_instancetype))
}

struct EmitCtx<'a> {
    src: &'a str,
    program: &'a Program,
    class_name: String,
    /// Static type of every name currently in scope (ivars + params +
    /// locals), used to resolve message-send receivers.
    scope: HashMap<String, String>,
    /// Names bound by a param or a local declaration -- these shadow an
    /// ivar of the same name, exactly like plain C/ObjC scoping.
    locals: std::collections::HashSet<String>,
    diags: Vec<Diagnostic>,
    /// (prototype, full definition) pairs for blocks hoisted out of this
    /// class's methods -- both go into the *primary* generated source (see
    /// `emit()`), not the companion file; the prototype goes ahead of
    /// every call site, the definition once at the very end. See the
    /// comment in `render_block` for why it can't live in the companion
    /// file instead.
    hoisted_blocks: Vec<(String, String)>,
    hoisted_structs: Vec<(String, String)>,
    /// (extern forward-declaration, real definition) pairs for boxed
    /// string literals -- see `render_boxed_string_literal`. Assembled
    /// into the primary source exactly like `hoisted_blocks`.
    hoisted_string_literals: Vec<(String, String)>,
    /// (name, full `static TYPE name [= init];` declaration) pairs for
    /// `__block`-qualified locals -- promoted to file scope exactly like
    /// oz_transpile's collect.py `_collect_block_vars` (see
    /// `hoist_block_var`), so a block can reference them without being a
    /// real capture. Assembled into the primary source like
    /// `hoisted_blocks`.
    hoisted_statics: Vec<(String, String)>,
    block_counter: usize,
    /// Statements that must precede the *current top-level statement*
    /// (not hoisted to file scope) -- e.g. the stack buffer an array
    /// literal builds its items into. Pushed by an expression-rendering
    /// helper, drained and prepended by whichever statement-level
    /// renderer (`render_body_with_comments`) is currently walking the
    /// enclosing statement. Mirrors the Python pipeline's `ctx.pre_stmts`
    /// (see `tools/oz_transpile/emit.py`).
    pre_stmts: Vec<String>,
    /// Cleanup statements owed by the `@synchronized` blocks currently
    /// enclosing the node being rendered, outermost first. A `return` has
    /// to replay them (innermost first) before leaving -- see
    /// `render_return_statement`.
    sync_cleanups: Vec<String>,
    /// C return type of the method being rendered, needed to declare the
    /// temporary a `return` inside `@synchronized` evaluates into.
    method_return_type: String,
    /// Slots to reserve in each class's slab -- see `pools`.
    pools: &'a crate::pools::PoolSizes,
    /// Object locals owned by each enclosing block, innermost last. Released
    /// when their block ends and at any jump out of it -- see
    /// `render_scoped_block`.
    arc_scopes: Vec<ArcScope>,
    /// Object locals ARC manages as *strong* variables, in the sense real
    /// ARC does: an overwrite releases what was there, so the variable
    /// holds at most one live object at a time. Decided once per body by
    /// `note_arc_managed_locals` and read by `render_strong_local_assign`.
    ///
    /// This is what makes the ordinary Objective-C shape
    ///
    /// ```objc
    /// Counter *c;
    /// for (int i = 0; i < 100; i++) {
    ///         c = [Counter alloc];
    /// }
    /// ```
    ///
    /// correct on one slab slot instead of leaking 99 objects. oz_static
    /// already did retain-new/release-old for strong *ivars*
    /// (`render_strong_ivar_assign`) and for properties (a synthesized
    /// setter); a plain local was the one strong storage class left doing
    /// neither, which is why `staticbar` had to reject the loop above
    /// rather than emit it.
    arc_managed_locals: std::collections::HashSet<String>,
    /// See `IntrospectionUse`.
    introspection_used: IntrospectionUse,
}

/// One block's worth of owned object locals.
#[derive(Default)]
struct ArcScope {
    owned: Vec<String>,
    /// Is this block a loop's body? `break`/`continue` unwind through
    /// scopes up to and including the nearest one of these, and no further:
    /// a local declared *after* the loop is still live once it exits.
    is_loop_body: bool,
}

impl<'a> EmitCtx<'a> {
    /// A fresh context for one top-level construct.
    ///
    /// Only the four things that actually vary between call sites are
    /// arguments; everything else starts empty or at its one sensible
    /// default. This exists because the eighteen fields used to be spelled
    /// out at six separate call sites -- three per emitter -- so adding a
    /// field meant six edits and seeding something into scope meant at
    /// least two, which is the exact shape of #250's fix (see #254).
    fn new(
        src: &'a str,
        program: &'a Program,
        class_name: String,
        scope: HashMap<String, String>,
        pools: &'a crate::pools::PoolSizes,
    ) -> Self {
        EmitCtx {
            src,
            program,
            class_name,
            scope,
            locals: HashSet::new(),
            diags: Vec::new(),
            hoisted_blocks: Vec::new(),
            hoisted_structs: Vec::new(),
            hoisted_string_literals: Vec::new(),
            hoisted_statics: Vec::new(),
            block_counter: 0,
            pre_stmts: Vec::new(),
            sync_cleanups: Vec::new(),
            method_return_type: "int".to_string(),
            pools,
            arc_scopes: Vec::new(),
            arc_managed_locals: HashSet::new(),
            introspection_used: IntrospectionUse::default(),
        }
    }

    fn err(&mut self, node: Node, message: impl Into<String>) {
        let (line, col) = line_col(self.src, node.start_byte());
        self.diags.push(Diagnostic::new(message, line, col));
    }
}

/// Which introspection support the emitted code actually referenced.
///
/// Gated on *use*, not on `--introspection` being set, so a program that
/// enables the option and never introspects anything pays nothing: the
/// superclass chain and each protocol's conformance bitmap are emitted
/// only if some call site named them. That is why this is threaded back
/// out of the walk instead of being derived from the `Program` -- the
/// emitter is the only thing that knows what it wrote.
#[derive(Default, Debug)]
pub struct IntrospectionUse {
    /// `-isKindOfClass:` appeared, so the ancestry walk and its table are
    /// needed.
    pub kind_of: bool,
    /// Protocols named by a `@protocol(...)` reaching
    /// `-conformsToProtocol:`; one conformance bitmap each. Ordered so the
    /// generated text is deterministic.
    pub protocols: std::collections::BTreeSet<String>,
}

impl IntrospectionUse {
    fn merge(&mut self, other: IntrospectionUse) {
        self.kind_of |= other.kind_of;
        self.protocols.extend(other.protocols);
    }

    pub fn is_empty(&self) -> bool {
        !self.kind_of && self.protocols.is_empty()
    }
}

/// Reconstruct `node`'s original text, but with any child for which
/// `render_child` returns `Some(text)` replaced by that text. Gaps between
/// children (whitespace, punctuation not modeled as separate nodes) are
/// copied verbatim from the source.
fn rebuild(node: Node, ctx: &mut EmitCtx, render_child: &mut dyn FnMut(Node, &mut EmitCtx) -> Option<String>) -> String {
    let mut out = String::new();
    let mut pos = node.start_byte();
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children {
        out.push_str(&ctx.src[pos..child.start_byte()]);
        match render_child(child, ctx) {
            Some(rendered) => out.push_str(&rendered),
            None => out.push_str(node_text(child, ctx.src)),
        }
        pos = child.end_byte();
    }
    out.push_str(&ctx.src[pos..node.end_byte()]);
    out
}

fn needs_translation(node: Node) -> bool {
    if matches!(
        node.kind(),
        "message_expression"
            | "block_literal"
            | "type_identifier"
            | "identifier"
            | "at_expression"
            | "array_literal"
            | "dictionary_literal"
            | "synchronized_statement"
            | "cast_expression"
            // Visited so ARC can prepend the releases a jump out of a loop
            // owes (`render_loop_jump`). With no owned local in scope the
            // keyword is returned unchanged, so this costs no output churn.
            | "break_statement"
            | "continue_statement"
            // `return` for the same reason, and it was missing: a return
            // inside an otherwise pure-C subtree was never visited, so
            // `render_return_statement` never ran and the scopes it unwinds
            // past kept their releases at the end of the block, where the
            // jump had already skipped them. `arc/return_in_nested_scope`
            // leaked its loop-body local on exactly that path -- an early
            // `return` from an `if` inside a `while`, with no Objective-C
            // anywhere in the `if` to force a visit. Found by running the
            // corpus under LeakSanitizer through this backend for the first
            // time; the case passed its own assertions throughout, since a
            // leak is invisible to a test that only checks return values.
            //
            // Costs no churn for the same reason the two above do not:
            // `render_return_statement` returns the original text when
            // there is nothing to release.
            | "return_statement"
    ) {
        return true;
    }
    if is_autoreleasepool_shape(node) {
        return true;
    }
    if node.kind() == "string_literal" {
        return is_boxed_string_literal(node);
    }
    let mut cursor = node.walk();
    let any_child = node.children(&mut cursor).any(needs_translation);
    any_child
}

/// Is `node` (an `at_expression`) shaped like a numeric/boolean boxed
/// literal -- `@42`, `@3.5f`, `@(expr)`, `@YES`/`@NO` -- as opposed to
/// anything else the grammar also parses as `at_expression` (a boxed call
/// expression, `@protocol(...)`, etc.), which has no OZQ31 desugaring and
/// must stay rejected. Used by both `staticbar.rs` (to know what's still
/// rejected) and `render_boxed_at_expression` below (to know how to
/// desugar what isn't).
pub(crate) fn is_numeric_boxed_shape(node: Node, src: &str) -> bool {
    let mut cursor = node.walk();
    let Some(inner) = node.children(&mut cursor).find(|c| c.kind() != "@") else {
        return false;
    };
    match inner.kind() {
        "number_literal" => true,
        "identifier" => matches!(node_text(inner, src), "YES" | "NO"),
        "parenthesized_expression" => true,
        _ => false,
    }
}

/// Is `node` (an `at_expression`) shaped like `@protocol(Name)`? There
/// is no dedicated `protocol_expression` node kind in this grammar
/// version (unlike real Clang's AST) -- `@protocol(Name)` parses as a
/// generic `at_expression` wrapping what looks syntactically like an
/// ordinary call expression to a function named `protocol`. Used only
/// to give this one specific `at_expression` shape a clearer rejection
/// message in `staticbar.rs`; it's still caught by the general
/// "not a numeric/boolean boxed literal" rejection either way.
/// The protocol named by a `@protocol(Name)` expression, or `None` if
/// `node` is not one.
///
/// A protocol has no runtime representation of its own here: the name is
/// resolved to a generated conformance bitmap
/// (`companion::render_introspection`), so `@protocol(...)` is legal only
/// where that bitmap is what is wanted -- as the argument of
/// `-conformsToProtocol:`. The static bar enforces the position; this only
/// reads the name.
pub(crate) fn protocol_literal_name(node: Node, src: &str) -> Option<String> {
    if !is_protocol_literal_shape(node, src) {
        return None;
    }
    let mut cursor = node.walk();
    let inner = node.children(&mut cursor).find(|c| c.kind() != "@")?;
    let mut c2 = inner.walk();
    let args = inner.children(&mut c2).find(|c| c.kind() == "argument_list")?;
    let mut c3 = args.walk();
    let name = args.children(&mut c3).find(|c| c.kind() == "identifier")?;
    Some(node_text(name, src).to_string())
}

pub(crate) fn is_protocol_literal_shape(node: Node, src: &str) -> bool {
    let mut cursor = node.walk();
    let Some(inner) = node.children(&mut cursor).find(|c| c.kind() != "@") else {
        return false;
    };
    if inner.kind() != "call_expression" {
        return false;
    }
    let mut c2 = inner.walk();
    let found =
        inner.children(&mut c2).find(|c| c.kind() == "identifier").is_some_and(|f| node_text(f, src) == "protocol");
    found
}

/// Render `node` to C text, returning (rendered_text, static_type).
/// static_type is "id" when unknown/irrelevant, or "class:Name" when the
/// expression is a bare reference to a known class name (a class-message
/// receiver), or "struct Name *" / a plain C type otherwise.
fn render_expr(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    match node.kind() {
        "message_expression" => render_message(node, ctx),
        "block_literal" => render_block(node, ctx),
        "identifier" => {
            let name = node_text(node, ctx.src).to_string();
            if name == "self" {
                return match ctx.program.classes.get(&ctx.class_name) {
                    Some(_) => ("self".to_string(), format!("struct {} *", ctx.class_name)),
                    None => {
                        ctx.err(node, "'self' used outside a method body");
                        ("self".to_string(), "id".to_string())
                    }
                };
            }
            if name == "super" {
                // `super` is not a value -- the receiver is still `self`;
                // only the *dispatch target* is the superclass.
                return match ctx.program.classes.get(&ctx.class_name).and_then(|c| c.superclass.clone()) {
                    Some(sup) => ("self".to_string(), format!("struct {} *", sup)),
                    None => {
                        ctx.err(node, "'super' used outside a method body, or in a root class with no superclass");
                        ("self".to_string(), "id".to_string())
                    }
                };
            }
            if !ctx.locals.contains(&name) {
                if let Some(path) = ctx.program.ivar_access_path(&ctx.class_name, &name) {
                    let ty = ctx.scope.get(&name).cloned().unwrap_or_else(|| "id".to_string());
                    return (format!("self->{}", path), ty);
                }
            }
            if ctx.program.is_class(&name) {
                return (name.clone(), format!("class:{}", name));
            }
            let ty = ctx.scope.get(&name).cloned().unwrap_or_else(|| "id".to_string());
            (name, ty)
        }
        "type_identifier" => {
            let name = node_text(node, ctx.src).to_string();
            if ctx.program.is_class(&name) {
                (format!("struct {}", name), "id".to_string())
            } else {
                (name, "id".to_string())
            }
        }
        // `id<Proto>` -- a protocol-qualified `id`, parsing as this same
        // node kind wrapping `id` plus a `protocol_reference_list` (see
        // `collect::extract_type_and_stars`'s `typedefed_specifier` arm,
        // fixed for the same reason: without this, the per-child
        // substitution below would leave `Frobbable` untouched inside a
        // literal `id<Frobbable>` in the output, which isn't valid C on
        // its own -- no generic/protocol-qualified type syntax exists in
        // plain C). Any *other* `typedefed_specifier` shape (a bare `id`,
        // or a real typedef'd name) is already valid as its own text, so
        // only this one shape needs rewriting.
        "typedefed_specifier" => {
            let mut cursor = node.walk();
            let has_protocol_list =
                node.children(&mut cursor).any(|c| c.kind() == "protocol_reference_list");
            if has_protocol_list {
                ("void *".to_string(), "id".to_string())
            } else if !needs_translation(node) {
                (node_text(node, ctx.src).to_string(), "id".to_string())
            } else {
                let rebuilt = rebuild(node, ctx, &mut |child, ctx| {
                    if needs_translation(child) {
                        Some(render_expr(child, ctx).0)
                    } else {
                        None
                    }
                });
                (rebuilt, "id".to_string())
            }
        }
        // `Container<Arg, ...>` (e.g. `OZArray<Widget *>`): this spike
        // renders a generic collection's declared type exactly like its
        // non-generic form, the same collapse
        // `collect::extract_type_and_stars`'s `generic_specifier` arm
        // does for an ivar/param/return type -- element-type constraints
        // are a `generics::check_program` concern, not codegen. Without
        // this, the per-child substitution below independently promotes
        // each bare class name it finds (`OZArray` -> `struct OZArray`,
        // and the *argument*'s `Widget` -> `struct Widget` too) while
        // leaving the `<...>` wrapper itself untouched, producing
        // `struct OZArray<struct Widget *>` -- not valid C. Only the
        // base name carries into the declaration; the declarator's own
        // `*` (e.g. `... *a`) is a separate token elsewhere in source
        // and is left alone, exactly like the plain `type_identifier`
        // arm above never itself emits a trailing `*`.
        "generic_specifier" => {
            let mut cursor = node.walk();
            let base = node.children(&mut cursor).find(|c| c.kind() == "type_identifier");
            match base {
                Some(base) => render_expr(base, ctx),
                None => (node_text(node, ctx.src).to_string(), "id".to_string()),
            }
        }
        // ARC's implicit `= nil` on a strong local declared without an
        // initializer. Real ARC zero-initializes every strong variable, and
        // here it is load-bearing rather than tidy: the first
        // `c = [Counter alloc]` releases whatever `c` held, so an
        // indeterminate `c` would be passed to `oz_static_release` and
        // dereferenced. `oz_static_release` is null-safe (`if (!self)
        // return;`), so nil makes that first release a no-op.
        "declaration" if declares_bare_managed_local(node, ctx) => {
            let text = rebuild(node, ctx, &mut |child, ctx| {
                if needs_translation(child) {
                    Some(render_expr(child, ctx).0)
                } else {
                    None
                }
            });
            let initialized = match text.rfind(';') {
                Some(i) => format!("{} = 0{}", text[..i].trim_end(), &text[i..]),
                None => text,
            };
            (initialized, "id".to_string())
        }
        "selector_expression" => render_selector_literal(node, ctx),
        "at_expression" if is_protocol_literal_shape(node, ctx.src) => {
            render_protocol_literal(node, ctx)
        }
        "at_expression" => render_boxed_at_expression(node, ctx),
        "string_literal" => render_boxed_string_literal(node, ctx),
        "array_literal" => render_boxed_array_literal(node, ctx),
        "dictionary_literal" => render_boxed_dictionary_literal(node, ctx),
        "block_pointer_declarator" | "abstract_block_pointer_declarator" => {
            // A block-typed local (`int (^square)(int) = ...;`) keeps its
            // `^` declarator syntax verbatim from source, but its
            // initializer -- a non-capturing block literal -- gets hoisted
            // to a plain static C function (see `render_block`), not a
            // real Objective-C block object. A variable declared with `^`
            // cannot hold a plain function pointer (they're distinct,
            // incompatible C types), so the declarator itself must be
            // rewritten to plain function-pointer syntax (`*`) to match.
            let text = rebuild(node, ctx, &mut |child, ctx| {
                if child.kind() == "^" {
                    Some("*".to_string())
                } else if needs_translation(child) {
                    Some(render_expr(child, ctx).0)
                } else {
                    None
                }
            });
            (text, "id".to_string())
        }
        "parenthesized_expression" => {
            let mut cursor = node.walk();
            let inner = node.children(&mut cursor).find(|c| c.kind() != "(" && c.kind() != ")");
            match inner {
                Some(inner) => {
                    let (text, ty) = render_expr(inner, ctx);
                    (format!("({})", text), ty)
                }
                None => (node_text(node, ctx.src).to_string(), "id".to_string()),
            }
        }
        "subscript_expression" => render_subscript_expression(node, ctx),
        "field_expression" => render_field_expression(node, ctx),
        "assignment_expression" => render_assignment_expression(node, ctx),
        "cast_expression" => render_cast_expression(node, ctx),
        "for_statement" if is_forin_shape(node) => render_forin_statement(node, ctx),
        "synchronized_statement" => render_synchronized_statement(node, ctx),
        "return_statement" => render_return_statement(node, ctx),
        "compound_statement" if is_autoreleasepool_shape(node) => {
            render_autoreleasepool_statement(node, ctx)
        }
        // Only a block that owns object locals is rewritten; every other one
        // stays byte-identical, so ARC adds no churn where it changes nothing.
        "compound_statement" if declares_owned_local(node, ctx) => {
            render_scoped_block(node, ctx)
        }
        "break_statement" | "continue_statement" if !ctx.arc_scopes.is_empty() => {
            render_loop_jump(node, ctx)
        }
        "declaration" if is_block_qualified_declaration(node, ctx.src) => {
            hoist_block_var(node, ctx);
            (String::new(), "id".to_string())
        }
        _ => {
            if !needs_translation(node) {
                (node_text(node, ctx.src).to_string(), "id".to_string())
            } else {
                let rebuilt = rebuild(node, ctx, &mut |child, ctx| {
                    if needs_translation(child) {
                        Some(render_expr(child, ctx).0)
                    } else {
                        None
                    }
                });
                (rebuilt, "id".to_string())
            }
        }
    }
}

struct MessageParts<'a> {
    receiver: Node<'a>,
    selector: String,
    args: Vec<Node<'a>>,
}

/// Is this send's receiver the literal `super`? A super send names one
/// specific implementation, so it is always a direct call -- it must
/// never be routed through the receiver's own class_id switch, which
/// would re-enter whichever override issued the send.
fn is_super_receiver(parts: &MessageParts, ctx: &EmitCtx) -> bool {
    parts.receiver.kind() == "identifier" && node_text(parts.receiver, ctx.src) == "super"
}

fn parse_message<'a>(node: Node<'a>, src: &str) -> MessageParts<'a> {
    let mut cursor = node.walk();
    let children: Vec<Node> =
        node.children(&mut cursor).filter(|c| c.kind() != "[" && c.kind() != "]").collect();
    let receiver = children[0];
    let mut selector = String::new();
    let mut args = Vec::new();
    if children.len() == 2 {
        selector = node_text(children[1], src).to_string();
    } else {
        let mut i = 1;
        while i + 1 < children.len() {
            let piece = children[i];
            selector.push_str(node_text(piece, src));
            selector.push(':');
            let arg = children[i + 2];
            args.push(arg);
            i += 3;
        }
    }
    MessageParts { receiver, selector, args }
}

/// Build a call to `{class_name} {selector}` as a class method, the way
/// `render_message` would for a real `[ClassName selector:arg]` send --
/// used to desugar a boxed literal into a call on the user-defined class
/// that must exist for the literal to mean anything (there's no built-in
/// Foundation in this design; `OZQ31`/`OZString` are ordinary classes the
/// static subset already knows how to compile). Returns `None` (leaving
/// the caller to raise a clear error) if the class or the method don't
/// exist, rather than emitting a call to a function that was never
/// generated.
fn synthetic_class_call(
    ctx: &EmitCtx,
    class_name: &str,
    selector: &str,
    arg_texts: &[String],
) -> Option<(String, String)> {
    let defining = find_defining_class(ctx.program, class_name, selector, true)?;
    let ret_ty = method_return_type(ctx.program, &defining, selector, true)
        .map(|(t, _)| t)
        .unwrap_or_else(|| "void".to_string());
    Some((
        format!("{}({})", method_fn_name(&defining, selector, true), arg_texts.join(", ")),
        ret_ty,
    ))
}

/// Desugars a numeric/boolean boxed literal (`@42`, `@3.5f`, `@(expr)`,
/// `@YES`/`@NO` -- see `is_numeric_boxed_shape`, which gates whether the
/// static bar even lets this node through) into a class-method call on
/// `OZQ31`: `fixedWithInt32:` for an integer-shaped value, `fixedWithFloat:`
/// for a float-shaped one. There's no real type-checker here to decide
/// int vs. float for an arbitrary expression, so this uses the same
/// heuristic Python's oracle output suggests: a literal token containing
/// `.` (or an `f`/`F` suffix) is float-shaped; everything else -- a plain
/// integer literal, `YES`/`NO`, or any non-literal expression like
/// `x + 3` -- defaults to int32.
/// `@selector(name)` -> the address of that selector's generated record.
///
/// The record is `const`, so a `SEL` is a pointer into flash and copying
/// one costs a register. Which selectors get a record is decided in
/// `collect`'s prescan rather than here, because
/// `Program::is_dynamically_dispatched` needs the answer before the
/// dispatch tables are generated -- this only has to refuse the cases
/// that have no record to point at.
fn render_selector_literal(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    if !ctx.program.reflection {
        ctx.err(
            node,
            "'@selector(...)' needs reflection, which is off -- set CONFIG_OBJZ_REFLECTION=y (oz2c --reflection)",
        );
        return (node_text(node, ctx.src).to_string(), "SEL".to_string());
    }
    let Some(name) = crate::collect::selector_literal_name(node, ctx.src) else {
        ctx.err(node, "'@selector(...)' needs a selector name");
        return (node_text(node, ctx.src).to_string(), "SEL".to_string());
    };
    // Every implementation is found through `find_defining_method` from
    // some class, so a selector nothing implements has no record and no
    // dispatch function. Passing it on would emit a reference to a symbol
    // that is never generated -- the same link-time failure `[X class]`
    // used to produce (#226).
    let implemented = ctx.program.class_order.iter().any(|c| {
        ctx.program.classes[c]
            .methods
            .iter()
            .any(|m| m.selector == name && !m.is_class_method)
    });
    if !implemented {
        ctx.err(
            node,
            format!(
                "'@selector({})' names no instance method declared by any class in this program",
                name
            ),
        );
        return (node_text(node, ctx.src).to_string(), "SEL".to_string());
    }
    // A selector that can reach a `-performSelector:` needs a wrapper of
    // the uniform shape, and so has to fit one. Which selectors those are
    // is `Program::needs_perform_wrapper`: the literals named at perform
    // sites, or -- if any site takes its `SEL` from a value, making it
    // undecidable -- every reflectively-named selector. One that cannot
    // have a wrapper is refused here rather than given a null `perform`
    // that would fail, or worse quietly answer nil, at run time.
    if ctx.program.needs_perform_wrapper(&name) {
        if let Some(why) = unperformable_reason(ctx.program, &name) {
            ctx.err(
                node,
                format!(
                    "'@selector({})' cannot be performed: {}{}",
                    name,
                    why,
                    if ctx.program.performs_via_value {
                        ". Some '-performSelector:' in this program takes its selector from a value rather than a literal, so nothing can tell which selector reaches it and every selector named by a '@selector(...)' has to be performable"
                    } else {
                        ", and a '-performSelector:' names it"
                    }
                ),
            );
            return (node_text(node, ctx.src).to_string(), "SEL".to_string());
        }
    }
    (format!("(&oz_sel_{})", selector_to_c(&name)), "SEL".to_string())
}

/// Why `selector` cannot be given a uniform-shape `perform` wrapper, or
/// `None` if it can.
///
/// The wrapper is `void *(*)(void *self, void *a0, void *a1)`, so the
/// selector's own arguments have to survive being passed as `void *` and
/// its result has to survive being handed back as one. Object and other
/// pointer types do; an `int` does not, and neither does a struct by
/// value. Real Objective-C's `-performSelector:` has the same restriction
/// -- it is typed `id (*)(id, SEL, ...)` -- but answers a signature
/// mismatch with garbage rather than a diagnostic.
fn unperformable_reason(program: &Program, selector: &str) -> Option<String> {
    let m = program.class_order.iter().find_map(|c| {
        program.classes[c].methods.iter().find(|m| m.selector == selector && !m.is_class_method)
    })?;
    if m.params.len() > 2 {
        return Some(format!(
            "it takes {} arguments, and '-performSelector:' passes at most two",
            m.params.len()
        ));
    }
    for (pname, ptype) in &m.params {
        let rendered = render_param(ptype, pname, program.root_class());
        if !rendered.contains('*') {
            return Some(format!(
                "its '{}' argument is '{}', which is not an object type",
                pname, ptype
            ));
        }
    }
    if m.return_type != "void" && !m.return_type.contains('*') && !m.returns_instancetype {
        return Some(format!(
            "it returns '{}', which is neither void nor an object type",
            m.return_type
        ));
    }
    None
}

/// `@protocol(Name)` -> the name of that protocol's generated conformance
/// bitmap.
///
/// Recording the use here rather than at the `-conformsToProtocol:` call
/// site is what keeps the footprint honest: exactly the protocols some
/// call site actually named get a bitmap, so enabling
/// `CONFIG_OBJZ_INTROSPECTION` and introspecting nothing costs nothing.
fn render_protocol_literal(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let Some(name) = protocol_literal_name(node, ctx.src) else {
        ctx.err(node, "'@protocol(...)' needs a single protocol name");
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    };
    if !ctx.program.protocols.contains_key(&name) {
        ctx.err(
            node,
            format!(
                "'@protocol({})' names no protocol declared in this program",
                name
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }
    ctx.introspection_used.protocols.insert(name.clone());
    (format!("oz_proto_{}", name), "const uint32_t *".to_string())
}

fn render_boxed_at_expression(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let (line, col) = line_col(ctx.src, node.start_byte());
    let mut cursor = node.walk();
    let Some(inner) = node.children(&mut cursor).find(|c| c.kind() != "@") else {
        ctx.err(node, "empty '@' boxed expression");
        return ("0".to_string(), "int".to_string());
    };

    // A literal token (`3.5f`, `@(3.5f)`) is float-shaped by its spelling.
    // Anything else -- an identifier or a general expression like
    // `@(f)`/`@(val + 3)` -- has no literal to inspect, so its resolved
    // static type (from `render_expr`'s return, backed by the pre-scanned
    // local-declaration scope) decides instead; a `float`/`double`-typed
    // value still boxes as float even though the boxed spelling itself
    // (a bare identifier) carries no hint.
    let literal_is_float = match inner.kind() {
        "parenthesized_expression" => {
            let mut c2 = inner.walk();
            let unwrapped = inner.children(&mut c2).find(|c| c.kind() != "(" && c.kind() != ")");
            unwrapped.is_some_and(|n| {
                n.kind() == "number_literal" && is_float_literal_text(node_text(n, ctx.src))
            })
        }
        "number_literal" => is_float_literal_text(node_text(inner, ctx.src)),
        _ => false,
    };
    let (value_text, value_ty) = render_expr(inner, ctx);
    let is_float = literal_is_float || value_ty == "float" || value_ty == "double";
    let selector = if is_float { "fixedWithFloat:" } else { "fixedWithInt32:" };

    match synthetic_class_call(ctx, "OZQ31", selector, &[value_text]) {
        Some((call, ret_ty)) => (call, ret_ty),
        None => {
            ctx.err(
                node,
                format!(
                    "boxed literal at {}:{} desugars to '[OZQ31 {}]', but no class 'OZQ31' with that class method is defined in this source",
                    line, col, selector
                ),
            );
            ("0".to_string(), "int".to_string())
        }
    }
}

fn is_float_literal_text(text: &str) -> bool {
    text.contains('.') || text.ends_with('f') || text.ends_with('F')
}

/// `string_literal` covers both a plain C string (`"foo"`) and a boxed
/// ObjC one (`@"foo"`) -- distinguished only by a leading `@` child.
pub(crate) fn is_boxed_string_literal(node: Node) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|c| c.kind() == "@");
    found
}

/// Desugars a boxed string literal `@"..."` the same way the Python
/// pipeline's oracle does (see `tools/oz_transpile/emit.py`'s
/// `ObjCStringLiteral` handling): NOT a class-method call -- OZString's
/// ivars (`_length`/`_hash`/`_data`) are all compile-time-computable and
/// its `dealloc` is a no-op (see `src/OZString.m`), so the literal
/// desugars directly to a static, immortal `struct OZString` instance
/// (`_hash` is always `0` -- the real pipeline never actually computes a
/// hash for it either) plus a cast-to-pointer expression at the use site.
/// Each unique literal gets its own instance (no dedup, unlike the Python
/// oracle -- a spike simplification; duplicates just cost a few more
/// bytes of `.rodata`, not correctness). A plain (non-`@`) string literal
/// is left completely untouched -- it's already valid C.
///
/// Placement mirrors `render_block`'s hoisting exactly, and for the same
/// underlying reason (a global/global-like declaration referenced by name
/// at its use site must be visible there, but OZString's own `struct
/// OZString` definition -- inline at OZString's `@interface`, since it's
/// not the root class -- may appear later in the source than an earlier
/// class's use of `@"..."`): an `extern` forward declaration goes ahead
/// of every use site (into `ctx.hoisted_string_literals`, assembled into
/// the *primary* source right after its `#include`, same as block
/// prototypes), and the real definition is appended once, after every
/// class -- by which point `struct OZString` is always already defined.
/// The forward declaration deliberately omits `static` (which the real
/// definition also then can't use, to avoid an extern/static linkage
/// clash) -- internal linkage doesn't matter for a single-translation-
/// unit generated file, so external linkage on a name this specific to
/// its own source position is a harmless simplification.
fn render_boxed_string_literal(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    if !is_boxed_string_literal(node) {
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }
    let (line, col) = line_col(ctx.src, node.start_byte());
    if !ctx.program.is_class("OZString") {
        ctx.err(
            node,
            format!(
                "boxed string literal at {}:{} desugars to a static 'struct OZString' instance, but no class 'OZString' is defined in this source",
                line, col
            ),
        );
        return ("0".to_string(), "int".to_string());
    }

    let mut cursor = node.walk();
    let content = node
        .children(&mut cursor)
        .find(|c| c.kind() == "string_content")
        .map(|c| node_text(c, ctx.src).to_string())
        .unwrap_or_default();
    // Matches the Python oracle's `len(raw)` exactly: the byte length of
    // the literal's source text between the quotes, before any escape
    // sequence is interpreted (so `"\n"` counts as length 2, not 1).
    let byte_len = content.len();
    let c_literal = format!("\"{}\"", content);

    ctx.block_counter += 1;
    let name = format!("_oz_str_L{}_C{}_{}", line, col, ctx.block_counter);
    let prototype = format!("extern struct OZString {};\n", name);
    // `_meta.immortal = 1` is what keeps this literal alive. It lives in
    // static storage, so `free()`-ing it aborts -- and something does try:
    // `companion`'s release path runs `{class}_oz_free` once a refcount hits
    // zero, and a literal's refcount does reach zero, because a collection
    // that absorbed it (`@[ @"a" ]`, or a dictionary key) releases its
    // elements when it is itself deallocated. `oz_static_release` returns on
    // the immortal bit before it even decrements, matching the real
    // `OZString.m`'s own `-dealloc` ("compile-time constant, never freed")
    // and the oracle's `emit.py` literal, which sets the same bit.
    //
    // This used to set `deallocating = 1` from birth instead, relying on the
    // re-entrancy guard to make release a no-op. That worked, but the field
    // said something false -- `deallocating` means "teardown is running right
    // now", not "never tear down" -- and it let the literal's refcount sink
    // to zero and below on the way (#228).
    let definition = format!(
        "struct OZString {} = {{ .base = {{ ._meta = {{ .class_id = OZ_STATIC_CLASS_OZString, .immortal = 1 }}, .oz_refcount = 1 }}, ._length = {}, ._hash = 0, ._data = {} }};\n",
        name, byte_len, c_literal
    );
    ctx.hoisted_string_literals.push((prototype, definition));
    (format!("(struct OZString *)&{}", name), "struct OZString *".to_string())
}

/// Mirrors Python's `_is_fresh_alloc` (`tools/oz_transpile/emit.py`):
/// does `node` produce a fresh +1 reference an array/dictionary literal
/// can absorb without an extra retain? Only a numeric/boolean boxed
/// literal, a boxed string literal, or a nested array/dictionary literal
/// qualify -- everything else (a plain variable reference, a message
/// send, even `[[Foo alloc] init]`) is treated as an existing reference
/// that must be retained before the literal can hold onto it, exactly
/// like the Python oracle (which draws the same line, for the same
/// reason: it has no general-purpose ownership analysis either).
fn is_fresh_alloc(node: Node, src: &str) -> bool {
    match node.kind() {
        "at_expression" => is_numeric_boxed_shape(node, src),
        "string_literal" => is_boxed_string_literal(node),
        "array_literal" | "dictionary_literal" => true,
        _ => false,
    }
}

/// Desugars a boxed array literal (`@[e1, e2, ...]`) into a call to
/// `OZArray_oz_initWithItems` (see `companion::render_array_support`) --
/// the same shape as the Python pipeline's `ObjCArrayLiteral` handling,
/// and since OZ-098 the same allocator behind it: the *stack* buffer
/// built here only carries the element pointers into the builder, which
/// copies them into a run of slots taken from the shared
/// `oz_item_pool`.
///
/// Each element is rendered, then either passed through as-is (a fresh
/// +1 reference, see `is_fresh_alloc`) or retained first (an existing
/// reference the array must now also own). The resulting pointers are
/// collected into a `void *` stack buffer pushed onto `ctx.pre_stmts`, so
/// the enclosing statement-level renderer (`render_body_with_comments`)
/// emits it just ahead of the statement using this literal; the literal
/// itself becomes a call taking that buffer and its length.
fn render_boxed_array_literal(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let (line, col) = line_col(ctx.src, node.start_byte());
    if !ctx.program.is_class("OZArray") {
        ctx.err(
            node,
            format!(
                "boxed array literal at {}:{} desugars to an 'OZArray' instance, but no class 'OZArray' is defined in this source",
                line, col
            ),
        );
        return ("0".to_string(), "int".to_string());
    }
    let root = ctx.program.root_class().unwrap_or("OZArray").to_string();

    let mut cursor = node.walk();
    let elements: Vec<Node> = node
        .children(&mut cursor)
        .filter(|c| !matches!(c.kind(), "@" | "[" | "]" | ","))
        .collect();

    let mut elem_refs = Vec::with_capacity(elements.len());
    for elem in &elements {
        let fresh = is_fresh_alloc(*elem, ctx.src);
        let (text, _) = render_expr(*elem, ctx);
        if fresh {
            elem_refs.push(format!("(void *){}", text));
        } else {
            elem_refs.push(format!("(void *)oz_static_retain((struct {} *)({}))", root, text));
        }
    }

    ctx.block_counter += 1;
    let buf_name = format!("_oz_arr_L{}_C{}_{}", line, col, ctx.block_counter);
    ctx.pre_stmts
        .push(format!("void *{}[] = {{ {} }};", buf_name, elem_refs.join(", ")));

    (
        format!("(struct OZArray *)OZArray_oz_initWithItems({}, {})", buf_name, elements.len()),
        "struct OZArray *".to_string(),
    )
}

/// Desugars a boxed dictionary literal (`@{k1: v1, k2: v2, ...}`) into a
/// call to the `OZDictionary_oz_initWithKeysValues` builder
/// (see `companion::render_dict_support`) -- the dictionary counterpart
/// of `render_boxed_array_literal` above (see its doc comment for the
/// element-ownership rules, identical here for both keys and values).
/// Each `dictionary_pair` child has exactly two named children (key
/// expression, value expression) either side of a `:` token.
fn render_boxed_dictionary_literal(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let (line, col) = line_col(ctx.src, node.start_byte());
    if !ctx.program.is_class("OZDictionary") {
        ctx.err(
            node,
            format!(
                "boxed dictionary literal at {}:{} desugars to an 'OZDictionary' instance, but no class 'OZDictionary' is defined in this source",
                line, col
            ),
        );
        return ("0".to_string(), "int".to_string());
    }
    let root = ctx.program.root_class().unwrap_or("OZDictionary").to_string();

    let mut cursor = node.walk();
    let pairs: Vec<Node> = node.children(&mut cursor).filter(|c| c.kind() == "dictionary_pair").collect();

    let mut key_refs = Vec::with_capacity(pairs.len());
    let mut value_refs = Vec::with_capacity(pairs.len());
    for pair in &pairs {
        let mut pc = pair.walk();
        let exprs: Vec<Node> = pair.children(&mut pc).filter(|c| c.kind() != ":").collect();
        let (key, value) = (exprs[0], exprs[1]);
        for (node, refs) in [(key, &mut key_refs), (value, &mut value_refs)] {
            let fresh = is_fresh_alloc(node, ctx.src);
            let (text, _) = render_expr(node, ctx);
            if fresh {
                refs.push(format!("(void *){}", text));
            } else {
                refs.push(format!("(void *)oz_static_retain((struct {} *)({}))", root, text));
            }
        }
    }

    ctx.block_counter += 1;
    let keys_buf = format!("_oz_dict_L{}_C{}_{}_keys", line, col, ctx.block_counter);
    let values_buf = format!("_oz_dict_L{}_C{}_{}_values", line, col, ctx.block_counter);
    ctx.pre_stmts.push(format!("void *{}[] = {{ {} }};", keys_buf, key_refs.join(", ")));
    ctx.pre_stmts.push(format!("void *{}[] = {{ {} }};", values_buf, value_refs.join(", ")));

    (
        format!(
            "(struct OZDictionary *)OZDictionary_oz_initWithKeysValues({}, {}, {})",
            keys_buf,
            values_buf,
            pairs.len()
        ),
        "struct OZDictionary *".to_string(),
    )
}

/// Is `node` (a `for_statement`) actually an ObjC for-in loop
/// (`for (Type *var in collection) { ... }`) rather than a classic
/// C for loop? tree-sitter-objc parses both under the same
/// `for_statement` node kind -- a classic for loop's clauses are
/// `;`-separated, so the only distinguishing feature is a literal `in`
/// token child.
fn is_forin_shape(node: Node) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|c| c.kind() == "in");
    found
}

/// (name, star_count) from a declarator -- a bare `identifier` (0
/// stars) or a (possibly multi-level) `pointer_declarator` wrapping one.
fn declarator_name_and_stars(node: Node, src: &str) -> (String, usize) {
    if node.kind() != "pointer_declarator" {
        return (node_text(node, src).to_string(), 0);
    }
    let mut cursor = node.walk();
    let mut stars = 0;
    let mut inner = None;
    for c in node.children(&mut cursor) {
        if c.kind() == "*" {
            stars += 1;
        } else {
            inner = Some(c);
        }
    }
    match inner {
        Some(inner) => {
            let (name, inner_stars) = declarator_name_and_stars(inner, src);
            (name, stars + inner_stars)
        }
        None => (node_text(node, src).to_string(), stars),
    }
}

/// Lowers `for (Type *var in collection) { body }` to a scoped,
/// iterator-based C for loop -- the exact same shape the Python
/// pipeline's oracle already uses (`tools/oz_transpile/emit.py`'s
/// `_emit_forin_stmt`):
///
/// ```c
/// {
///     struct OZObject *_oz_iterN = (struct OZObject *)OZ_PROTOCOL_SEND_iter((struct OZObject *)(collection));
///     struct OZObject *_oz_recvN = _oz_iterN;
///     for (Type *var = (Type *)OZ_PROTOCOL_SEND_next(_oz_recvN); var != ((void *)0); var = (Type *)OZ_PROTOCOL_SEND_next(_oz_recvN)) { body }
/// }
/// ```
///
/// `break`/`continue`/nesting all fall out for free -- this desugars to
/// a real C `for` loop wrapped in a block, so they mean exactly what
/// they already mean there; nothing loop-specific to handle. `-iter`/
/// `-next` always route through `OZ_PROTOCOL_SEND_`, matching the
/// oracle's own unconditional choice, since `collection`'s static type
/// might be anywhere from a concrete class to plain `id` -- never a
/// direct call resolved from one receiver type.
/// ObjC subscripting -- `array[0]`, `dict[@"key"]` -- desugared to the
/// message send it stands for, the way Clang resolves it into a
/// `PseudoObjectExpr` for the Python pipeline:
///
///   - `objectAtIndexedSubscript:` when the receiver's class implements it
///   - `objectForKeyedSubscript:` when it implements that instead
///
/// Which one applies is decided by the receiver's class, not by the index
/// expression: the two selectors are declared by different classes
/// (`OZArray` and `OZDictionary` respectively), so a class implementing
/// both is not a shape that arises. If it ever did, the index would have
/// to break the tie.
///
/// A receiver whose static type isn't a resolved class pointer is left
/// exactly as written -- that's ordinary C array indexing, which the
/// Foundation sources themselves rely on (`_items[index]` over an
/// `id *_items`). Only a *resolved object* receiver is rewritten, and one
/// with no subscript method is a hard error rather than being emitted as
/// pointer arithmetic over the object, which is what passing it through
/// used to do.
fn render_subscript_expression(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let open = children.iter().position(|c| c.kind() == "[");
    let close = children.iter().position(|c| c.kind() == "]");
    let pass_through = |ctx: &mut EmitCtx| {
        let rebuilt = rebuild(node, ctx, &mut |child, ctx| {
            if needs_translation(child) {
                Some(render_expr(child, ctx).0)
            } else {
                None
            }
        });
        (rebuilt, "id".to_string())
    };

    let (Some(open), Some(close)) = (open, close) else {
        return pass_through(ctx);
    };
    let (Some(recv_node), Some(index_node)) = (
        children.first().copied().filter(|_| open > 0),
        children.get(open + 1).copied().filter(|_| open + 1 < close),
    ) else {
        return pass_through(ctx);
    };

    let (recv_text, recv_type) = render_expr(recv_node, ctx);
    // Indexing a C array of plain structs is ordinary C, and its element
    // type is spelled `struct Foo` just like a class's -- so the name has
    // to be checked against the program, or `points[0]` on a
    // `struct point points[3]` would be reported as a class that "does not
    // support subscripting".
    let class = match class_name_from_type(&recv_type) {
        Some(class) if ctx.program.is_class(&class) => class,
        _ => return pass_through(ctx),
    };

    const INDEXED: &str = "objectAtIndexedSubscript:";
    const KEYED: &str = "objectForKeyedSubscript:";
    let selector = if find_defining_class(ctx.program, &class, INDEXED, false).is_some() {
        INDEXED
    } else if find_defining_class(ctx.program, &class, KEYED, false).is_some() {
        KEYED
    } else {
        ctx.err(
            node,
            format!(
                "'{}' does not support subscripting (it implements neither '{}' nor '{}'), so '{}' has no meaning on it",
                class,
                INDEXED,
                KEYED,
                one_line(node_text(node, ctx.src))
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    };

    let (index_text, _) = render_expr(index_node, ctx);
    // `super[i]` has no meaning to reach for: a subscript's receiver is a
    // collection, and `super` is not one.
    send_to_resolved_class(ctx, &class, selector, &recv_text, &[index_text], false)
}

/// Is `node` the literal `super`? `render_expr` renders `super` to `self`
/// (the receiver really is `self`; only the dispatch target differs), so
/// this has to be asked of the node, before that.
fn is_super_identifier(node: Node, src: &str) -> bool {
    node.kind() == "identifier" && node_text(node, src) == "super"
}

/// Splits a `field_expression` into (object, field name), but only when it
/// is dot syntax on an Objective-C object -- `None` for anything that is
/// ordinary C member access and must pass through untouched.
///
/// Two things disqualify it. `a->b` is direct ivar access, which is already
/// valid C against the generated struct and means exactly what it says. And
/// `a.b` where `a` is a plain C struct *value* is ordinary member access --
/// `samples/hello_category`'s `struct color`, or the `struct sensor_msg` in
/// `tests/behavior/cases/regression/issue_090_header_preservation.m`. Only
/// an object-typed left side makes the `.` Objective-C's: in C, `.` on a
/// pointer is not legal at all, so there is no ambiguity left to resolve.
fn dot_syntax_parts<'a>(
    node: Node<'a>,
    ctx: &mut EmitCtx,
) -> Option<(Node<'a>, String, String, String)> {
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    if !children.iter().any(|c| c.kind() == ".") {
        return None;
    }
    let object = *children.first()?;
    let field = *children.last()?;
    if field.kind() != "field_identifier" {
        return None;
    }
    let field_name = node_text(field, ctx.src).to_string();
    let (obj_text, obj_type) = render_expr(object, ctx);
    let class = class_name_from_type(&obj_type)?;
    // `struct point` and `struct Widget` are spelled identically; only the
    // program says which is a class. Without this a plain C struct's member
    // access was read as dot syntax and rejected as "'point' has no
    // property or getter named 'x'".
    if !ctx.program.is_class(&class) {
        return None;
    }
    Some((object, obj_text, class, field_name))
}

/// The accessor selector a property is reached through, which `getter=` /
/// `setter=` can rename to anything -- so the field name in source is not
/// necessarily the selector to call.
///
/// Falls back to the plain field name (and its `setX:` form), because
/// Objective-C also accepts dot syntax against a bare getter method with no
/// `@property` behind it at all.
fn accessor_selector(ctx: &EmitCtx, class: &str, field: &str, writing: bool) -> String {
    match ctx.program.find_property(class, field) {
        Some((_, prop)) if writing => prop
            .setter_sel
            .clone()
            .unwrap_or_else(|| crate::collect::default_setter_sel(&prop.name)),
        Some((_, prop)) => prop.getter_sel.clone().unwrap_or_else(|| prop.name.clone()),
        None if writing => crate::collect::default_setter_sel(field),
        None => field.to_string(),
    }
}

/// `obj.prop` -- Objective-C property dot syntax, read form, lowered to the
/// getter call: `[App sharedInstance].heap` becomes
/// `App_heap(App_sharedInstance_cls())`.
///
/// Chaining needs no special handling: `a.b.c` recurses, and the inner
/// call's return type is what resolves `c`'s class, exactly as it would for
/// a chain of message sends.
fn render_field_expression(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let pass_through = |ctx: &mut EmitCtx| {
        let rebuilt = rebuild(node, ctx, &mut |child, ctx| {
            if needs_translation(child) {
                Some(render_expr(child, ctx).0)
            } else {
                None
            }
        });
        (rebuilt, "id".to_string())
    };

    let Some((object, obj_text, class, field)) = dot_syntax_parts(node, ctx) else {
        return pass_through(ctx);
    };
    let super_access = is_super_identifier(object, ctx.src);
    let getter = accessor_selector(ctx, &class, &field, false);
    if find_defining_class(ctx.program, &class, &getter, false).is_none() {
        // Reaching a bare ivar through dot syntax is not Objective-C either
        // -- `.` is accessor syntax, and Clang rejects `obj.someIvar` the
        // same way. Rewriting it to `->` would compile, which is precisely
        // why it is not done: it would accept a program the language does
        // not, and quietly bypass whatever the accessor does.
        ctx.err(
            node,
            format!(
                "'{}' has no property or getter named '{}', so '{}' has no meaning on it",
                class,
                field,
                one_line(node_text(node, ctx.src))
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }
    send_to_resolved_class(ctx, &class, &getter, &obj_text, &[], super_access)
}

/// `_ivar = value` where `_ivar` is a strong object ivar: takes ownership of
/// the new value and gives up the old one, the way assigning to a `__strong`
/// ivar does under ARC. `None` when this is not that.
///
/// oz_static had the release half of strong-ivar ownership without the
/// retain half: `{Class}_oz_release_ivars` releases every owned object ivar
/// when an instance dies, but nothing ever retained what was stored there.
/// `samples/transpiled_led` is a chain of six OZHelpers, each holding the
/// previous one in a strong `_next` ivar assigned straight from a parameter,
/// and it segfaulted -- AddressSanitizer named it exactly:
/// heap-use-after-free in `oz_atomic_dec_and_test`, the object freed once by
/// its owner's `oz_release_ivars` and again by the scope-exit release of the
/// local that created it. Releasing a reference never taken is a double
/// free, so the two halves have to match: retain exactly what dealloc will
/// release, which is why the predicate here is
/// `Program::owned_object_ivar_names` -- the same list that path uses.
///
/// Properties were never affected: a synthesized setter already does
/// retain-new/release-old (`render_synthesized_accessor`). Among *ivars*, only
/// direct assignment was missing it. A plain strong **local** was missing it
/// too, which is a separate storage class and a separate fix
/// (`render_strong_local_assign`, #234) -- worth saying explicitly, because
/// this comment used to read as though locals were already covered.
///
/// A `+1` right-hand side is stored without retaining, because it already
/// carries the reference the ivar is taking over -- retaining it as well
/// would leak, since a temporary has no scope-exit release to balance it.
/// Everything else is borrowed and gets retained; where that value is also
/// an owned local, its own scope-exit release keeps the count right.
///
/// Emitted as a comma expression over a temporary rather than several
/// statements, so it stays usable wherever an assignment was, and in the
/// same order the synthesized setter uses: assign, retain new, release old.
/// That order is what makes self-assignment (`_x = _x`) safe -- releasing
/// first could free the value being stored.
fn render_strong_ivar_assign(
    node: Node,
    left: Node,
    right: Node,
    ctx: &mut EmitCtx,
) -> Option<(String, String)> {
    if left.kind() != "identifier" {
        return None;
    }
    let name = node_text(left, ctx.src).to_string();
    if ctx.locals.contains(&name) {
        return None;
    }
    if !ctx.program.owned_object_ivar_names(&ctx.class_name).contains(&name) {
        return None;
    }
    let path = ctx.program.ivar_access_path(&ctx.class_name, &name)?;
    let root = ctx.program.root_class()?.to_string();

    let takes_ownership = crate::arc::is_owning_expr(right, ctx.src, ctx.program, &ctx.program.owning_methods);
    let (value, value_ty) = render_expr(right, ctx);

    let (line, col) = line_col(ctx.src, node.start_byte());
    ctx.block_counter += 1;
    let prev = format!("_oz_prev_L{}_C{}_{}", line, col, ctx.block_counter);
    ctx.pre_stmts.push(format!(
        "struct {root} *{prev} = (struct {root} *)(self->{path});",
        root = root,
        prev = prev,
        path = path
    ));

    let retain = if takes_ownership {
        String::new()
    } else {
        format!("oz_static_retain((struct {root} *)(self->{path})), ", root = root, path = path)
    };
    // The comma expression yields the stored value only where something can
    // use it. As a bare statement -- which is nearly always -- a trailing
    // `self->_x` is a read whose result is discarded, and Clang says so:
    // "expression result unused" [-Wunused-value]. Zephyr builds with
    // -Werror, so that is a build failure, not just noise.
    let yields = if node
        .parent()
        .is_some_and(|parent| parent.kind() == "expression_statement")
    {
        String::new()
    } else {
        format!(", self->{path}", path = path)
    };
    let expr = format!(
        "(self->{path} = {value}, {retain}oz_static_release({prev}){yields})",
        path = path,
        value = value,
        retain = retain,
        prev = prev,
        yields = yields
    );
    let ty = if value_ty == "id" { format!("struct {} *", root) } else { value_ty };
    Some((expr, ty))
}

/// Assignment to a strong object *local*: release what it held, so the
/// variable holds at most one live object at a time.
///
/// This is what real ARC does at every store to a strong variable, and its
/// absence was the reason `staticbar` had to reject an ordinary loop:
///
/// ```objc
/// Counter *c;
/// for (int i = 0; i < 100; i++) {
///         c = [Counter alloc];   /* previous c released here */
/// }
/// ```
///
/// Without the release each iteration abandoned a live object, so 100
/// iterations needed 100 slab slots while `pools::count_sites` had counted
/// the one allocation site once. The slab ran out and the next send wrote
/// through a null receiver -- an MPU fault on target, and a *silent* one,
/// since nothing about it fails to compile. With the release, one slot is
/// genuinely correct and the shape needs no diagnostic at all.
///
/// The ordering is `render_strong_ivar_assign`'s, for the same reason:
/// assign, retain new, release old. Releasing first could free the very
/// value being stored, which is what makes self-assignment (`c = c`) safe.
/// A `+1` right-hand side is stored without retaining -- it already carries
/// the reference the variable is taking over, and a temporary has no
/// scope-exit release to balance a second one.
///
/// Membership in `arc_managed_locals` is what guarantees the release is
/// sound: every value such a local can hold is owned, so there is never a
/// reference released that was not taken. See `managed_object_locals`.
///
/// Takes no `node`, unlike `render_strong_ivar_assign`: that one needs the
/// assignment's position to name a temporary, and this one deliberately
/// emits no temporary at all.
fn render_strong_local_assign(
    left: Node,
    right: Node,
    ctx: &mut EmitCtx,
) -> Option<(String, String)> {
    if left.kind() != "identifier" {
        return None;
    }
    let name = node_text(left, ctx.src).to_string();
    if !ctx.arc_managed_locals.contains(&name) {
        return None;
    }
    let root = ctx.program.root_class()?.to_string();
    let kind = classify_store(&name, right, ctx.src, ctx.program);
    if kind == LocalStore::Unsupported {
        return None;
    }
    let (value, _value_ty) = render_expr(right, ctx);

    // No temporary, deliberately. An earlier version captured the previous
    // value into a `ctx.pre_stmts` local, which is drained by whichever
    // *top-level* statement is being rendered -- so for an assignment inside
    // a loop the capture was hoisted above the `for`, read `c` once while it
    // was still nil, and every iteration then released nil. The loop leaked
    // exactly as before, and the generated C looked plausible. Naming `c`
    // directly inside the comma expression is both simpler and correct: the
    // comma operator sequences left to right, so a release written before
    // the assignment observes the old value.
    let expr = match kind {
        // A `+1` right-hand side that does not mention the variable: release
        // first, then assign. Releasing *before* the allocation is what lets
        // one slab slot serve the whole loop -- the slot goes back to the
        // slab and the very next allocation can take it again. Allocating
        // first would need two slots live at once. The right-hand side is
        // known not to read the variable (`classify_store`), so freeing it
        // first cannot pull the ground from under the value being computed.
        LocalStore::Owning => format!(
            "(oz_static_release((struct {root} *)({name})), {name} = {value})",
            root = root,
            name = name,
            value = value
        ),
        // A plain identifier: retain new, release old, assign -- the order
        // `render_strong_ivar_assign` uses and for the same reason, that it
        // makes self-assignment (`c = c`) safe. Naming the value twice is
        // free of consequence only because it is an identifier, which is
        // exactly what `classify_store` checked.
        LocalStore::BorrowedIdent => format!(
            "(oz_static_retain((struct {root} *)({value})), \
             oz_static_release((struct {root} *)({name})), {name} = {value})",
            root = root,
            name = name,
            value = value
        ),
        LocalStore::Unsupported => unreachable!("returned above"),
    };
    // The comma expression already yields the assigned value, so unlike the
    // ivar path there is no trailing read to suppress for `-Wunused-value`.
    let ty = ctx.scope.get(&name).cloned().unwrap_or_else(|| format!("struct {} *", root));
    Some((expr, ty))
}

/// Assignment, handled here for three reasons: a property dot-syntax *target*
/// has to become the setter call rather than an assignment to a function
/// call, a strong object ivar has to take ownership of what it is given
/// (`render_strong_ivar_assign`), and a strong object *local* has to release
/// what it held (`render_strong_local_assign`). Every other assignment passes
/// through as the C it already is.
///
/// A compound assignment (`+=`, `<<=`, ...) has to read the property and
/// write it back, which mentions the receiver twice -- so it is only
/// accepted when the receiver is a plain identifier (or `self`), where
/// evaluating it twice provably cannot differ. `[obj thing].count += 1`
/// stays a hard error instead of silently sending `thing` twice.
fn render_assignment_expression(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let pass_through = |ctx: &mut EmitCtx| {
        let rebuilt = rebuild(node, ctx, &mut |child, ctx| {
            if needs_translation(child) {
                Some(render_expr(child, ctx).0)
            } else {
                None
            }
        });
        (rebuilt, "id".to_string())
    };

    let (Some(left), Some(op), Some(right)) =
        (children.first().copied(), children.get(1).copied(), children.last().copied())
    else {
        return pass_through(ctx);
    };
    if children.len() < 3 {
        return pass_through(ctx);
    }
    if node_text(op, ctx.src) == "=" {
        if let Some(rendered) = render_strong_ivar_assign(node, left, right, ctx) {
            return rendered;
        }
        if let Some(rendered) = render_strong_local_assign(left, right, ctx) {
            return rendered;
        }
    }
    if left.kind() != "field_expression" {
        return pass_through(ctx);
    }
    let operator = node_text(op, ctx.src).to_string();
    let Some((object, obj_text, class, field)) = dot_syntax_parts(left, ctx) else {
        return pass_through(ctx);
    };

    let setter = accessor_selector(ctx, &class, &field, true);
    if find_defining_class(ctx.program, &class, &setter, false).is_none() {
        let readonly = ctx
            .program
            .find_property(&class, &field)
            .is_some_and(|(_, prop)| prop.is_readonly);
        ctx.err(
            node,
            if readonly {
                format!(
                    "'{}.{}' is a readonly property, so '{}' cannot assign to it",
                    class,
                    field,
                    one_line(node_text(node, ctx.src))
                )
            } else {
                format!(
                    "'{}' has no property or setter named '{}', so '{}' has no meaning on it",
                    class,
                    field,
                    one_line(node_text(node, ctx.src))
                )
            },
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }

    let super_access = is_super_identifier(object, ctx.src);
    let (right_text, _) = render_expr(right, ctx);
    if operator == "=" {
        return send_to_resolved_class(ctx, &class, &setter, &obj_text, &[right_text], super_access);
    }

    // Compound: read, combine, write back -- so the receiver appears twice.
    if object.kind() != "identifier" {
        ctx.err(
            node,
            format!(
                "'{}' needs to read '{}' and write it back, which would evaluate the receiver '{}' twice -- assign through a local instead",
                one_line(node_text(node, ctx.src)),
                field,
                one_line(node_text(object, ctx.src))
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }
    let getter = accessor_selector(ctx, &class, &field, false);
    if find_defining_class(ctx.program, &class, &getter, false).is_none() {
        ctx.err(
            node,
            format!(
                "'{}' needs to read '{}' first, but '{}' has no property or getter of that name",
                one_line(node_text(node, ctx.src)),
                field,
                class
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }
    let (read, _) = send_to_resolved_class(ctx, &class, &getter, &obj_text, &[], super_access);
    // `x += y` is `x = x + y`: drop the trailing `=` to get the operator.
    let binary_op = operator.trim_end_matches('=').to_string();
    let combined = format!("{} {} ({})", read, binary_op, right_text);
    send_to_resolved_class(ctx, &class, &setter, &obj_text, &[combined], super_access)
}

/// One instance send whose receiver's class is already resolved, routed by
/// the same rule as `render_message`'s resolved-receiver branch: a direct
/// call when no subclass overrides the selector, the `class_id` switch when
/// one does (see `Program::has_overriding_subclass`).
///
/// Used by desugarings that synthesize a send rather than translating a
/// literal `[recv sel:...]` -- subscripting and property dot syntax.
/// Deliberately does not handle class methods, which need care
/// `render_message` already takes.
///
/// `super_send` carries the one thing the receiver *text* cannot: `super`
/// renders to `self`, so by the time there is a string left there is no way
/// to tell the two apart, and they dispatch differently. See the two places
/// it is consulted below; `render_message` applies the same two rules for a
/// literal `[super sel]`.
fn send_to_resolved_class(
    ctx: &mut EmitCtx,
    class: &str,
    selector: &str,
    recv_text: &str,
    arg_texts: &[String],
    super_send: bool,
) -> (String, String) {
    let root = ctx.program.root_class().unwrap_or("OZSRoot").to_string();
    // A `super` access names one specific implementation by definition, so
    // it must stay a direct call. Routing it through the receiver's own
    // class_id would re-enter the override that issued it -- for a property
    // getter, a subclass override reading `super.thing` would call itself
    // forever.
    if !super_send && ctx.program.has_overriding_subclass(class, selector) {
        return dynamic_dispatch_call(ctx.program, &root, selector, recv_text, arg_texts);
    }
    let Some(defining) = find_defining_class(ctx.program, class, selector, false) else {
        return dynamic_dispatch_call(ctx.program, &root, selector, recv_text, arg_texts);
    };
    let (ret_ty, returns_instancetype) = method_return_type(ctx.program, &defining, selector, false)
        .unwrap_or_else(|| ("void".to_string(), false));
    let mut call_args = vec![format!("(struct {} *)({})", defining, recv_text)];
    call_args.extend(arg_texts.iter().cloned());
    let call =
        format!("{}({})", method_fn_name(&defining, selector, false), call_args.join(", "));
    // For a `super` access the real receiver is still `self`, so an
    // `instancetype` result covaries with this class, not with the
    // superclass the call was resolved against.
    let covariant_target = if super_send { ctx.class_name.clone() } else { class.to_string() };
    if returns_instancetype && defining != covariant_target {
        (
            format!("(struct {} *)({})", covariant_target, call),
            format!("struct {} *", covariant_target),
        )
    } else {
        (call, ret_ty)
    }
}

/// `@synchronized(obj) { body }` lowered to a scoped critical section over
/// `obj`'s *own* lock:
///
/// ```c
/// { /* @synchronized(obj) */
///     struct OZObject *_oz_sync_obj_... = (struct OZObject *)(obj);
///     int _oz_sync_held_... = (_oz_sync_obj_...->oz_sync_owner != oz_current_thread());
///     oz_spinlock_key_t _oz_sync_key_... = oz_spin_key_none();
///     if (_oz_sync_held_...) {
///         _oz_sync_key_... = oz_spin_lock(&_oz_sync_obj_...->oz_sync_lock);
///         _oz_sync_obj_...->oz_sync_owner = oz_current_thread();
///     }
///     oz_static_retain(_oz_sync_obj_...);
///     ... body ...
///     oz_static_release(_oz_sync_obj_...);
///     if (_oz_sync_held_...) {
///         _oz_sync_obj_...->oz_sync_owner = (void *)0;
///         oz_spin_unlock(&_oz_sync_obj_...->oz_sync_lock, _oz_sync_key_...);
///     }
/// }
/// ```
///
/// The lock is a field of the object (`SYNC_LOCK_FIELD` in the root struct,
/// present only when the program uses `@synchronized`), so two threads
/// synchronizing on the same object contend on the same lock. The receiver is
/// bound to a temporary and evaluated exactly once -- it is named four times
/// here, and `@synchronized([App sharedInstance])` must not send the message
/// four times.
///
/// **This used to be a lock declared inside the block, on the caller's own
/// stack, fresh per call**, matching the Python pipeline's per-block
/// `OZSpinLock` (`emit.py::_emit_synchronized_stmt`). Two threads then locked
/// two different locks, so it bought an interrupt-disabled critical section
/// and no mutual exclusion keyed on `obj` at all. It looked correct because
/// `k_spin_lock` calls `arch_irq_lock()` unconditionally, which on a single
/// core does serialize the section -- and every board in use was single-core.
/// Measured on two cores it was indistinguishable from no lock:
/// `count=2015 expected=4000` against `2023` unlocked (`samples/smp_shared`,
/// gap W of the retired PARITY.md; see docs/STATUS.md).
///
/// `oz_sync_owner` is what makes the per-object lock safe, and it is not a
/// recursive lock -- a `k_spinlock` cannot be one. A re-entrant
/// `@synchronized` on the same object *does not attempt the second acquire*:
/// it sees itself as owner and skips both lock and unlock. `held` is a
/// per-block local, so nesting unwinds correctly at any depth with no counter,
/// since inner blocks never acquired. Without this, the oracle's own
/// `tests/behavior/cases/synchronized/nested.m` shape -- two receivers that
/// may alias, as `[n runNested:n]` does -- would deadlock on hardware while
/// passing on host, where `oz_spin_lock` is a no-op.
///
/// That is checked rather than reasoned about since #278:
/// `just test-spin-validate` enables `CONFIG_SPIN_VALIDATE`, under which a
/// second acquire fails Zephyr's own `z_spin_lock_valid()`. Removing the
/// owner check makes `samples/smp_shared` (two cores) and
/// `samples/pool_demo` (one core, nesting across a method boundary) both
/// report `ASSERTION FAIL [z_spin_lock_valid(l)] ... Invalid spinlock`.
///
/// The unlock is emitted as plain statements rather than through the
/// scoped `OZ_SPINLOCK` macro because that macro is a `for` loop, so a
/// `break` inside it would skip the unlock. Jumps out of the body are
/// handled instead by `ctx.sync_cleanups`: `render_return_statement`
/// replays the pending cleanup ahead of any `return`, matching the
/// oracle's `early_return.m`. `break`/`continue`/`goto` crossing the
/// boundary stay hard errors (`staticbar::check_synchronized_body`) --
/// unlike `return`, they can leave the block without a value to hand
/// back, and no oracle case needs them.
/// The root-struct field that `@synchronized` locks. One per object, so two
/// threads synchronizing on the same object contend on the same lock -- which
/// a per-block lock on each caller's own stack could never do.
pub(crate) const SYNC_LOCK_FIELD: &str = "oz_sync_lock";

/// The root-struct field recording which thread holds `oz_sync_lock`, so a
/// re-entrant `@synchronized` on the same object skips the acquire instead of
/// deadlocking on a spinlock it already holds. Zero when the lock is free,
/// which is why `oz_current_thread()` must never return NULL.
pub(crate) const SYNC_OWNER_FIELD: &str = "oz_sync_owner";

fn render_synchronized_statement(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let (line, col) = line_col(ctx.src, node.start_byte());
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    let open = children.iter().position(|c| c.kind() == "(");
    let close = children.iter().position(|c| c.kind() == ")");
    let (Some(open), Some(close)) = (open, close) else {
        ctx.err(node, "malformed @synchronized: expected '@synchronized(object) { ... }'");
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    };
    let Some(obj_node) = children.get(open + 1).copied().filter(|_| open + 1 < close) else {
        ctx.err(node, "@synchronized needs an object to lock: '@synchronized(object) { ... }'");
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    };
    let Some(body) = children.get(close + 1).copied() else {
        ctx.err(node, "@synchronized needs a body: '@synchronized(object) { ... }'");
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    };

    let (obj_text, _) = render_expr(obj_node, ctx);
    let root = ctx.program.root_class().unwrap_or("OZObject").to_string();

    ctx.block_counter += 1;
    let suffix = format!("L{}_C{}_{}", line, col, ctx.block_counter);
    let key = format!("_oz_sync_key_{}", suffix);

    // Held across the body so the object can't be deallocated mid-section,
    // mirroring the retain/release the oracle's OZSpinLock does in its
    // -initWithObject:/-dealloc pair.
    // The receiver is bound to a temporary and evaluated exactly once. It
    // has to be: the lock, the retain, the release and the unlock all name
    // it, and `@synchronized([App sharedInstance]) { ... }` must not send
    // the message four times. The previous per-block form evaluated it
    // twice, which was already one too many.
    let obj_var = format!("_oz_sync_obj_{}", suffix);
    let held = format!("_oz_sync_held_{}", suffix);
    let bind = format!("struct {} *{} = (struct {} *)({});", root, obj_var, root, obj_text);
    let retain = format!("oz_static_retain({});", obj_var);
    // Only the block that actually acquired the lock releases it. `held` is a
    // per-block local, so nesting to any depth unwinds correctly without a
    // counter: the inner blocks never acquired and never unlock.
    let cleanup = format!(
        "oz_static_release({obj});\n\
         \tif ({held}) {{\n\
         \t\t{obj}->{owner_field} = (void *)0;\n\
         \t\toz_spin_unlock(&{obj}->{lock_field}, {key});\n\
         \t}}",
        obj = obj_var,
        held = held,
        owner_field = SYNC_OWNER_FIELD,
        lock_field = SYNC_LOCK_FIELD,
        key = key
    );

    ctx.sync_cleanups.push(cleanup.clone());
    let body_text = if body.kind() == "compound_statement" {
        render_body_with_comments(body, ctx)
    } else {
        let (text, _) = render_expr(body, ctx);
        format!("{{\n\t{}\n\t}}", text)
    };
    ctx.sync_cleanups.pop();

    (
        format!(
            "{{\n\
             \t{bind}\n\
             \tint {held} = ({obj}->{owner_field} != oz_current_thread());\n\
             \toz_spinlock_key_t {key} = oz_spin_key_none();\n\
             \tif ({held}) {{\n\
             \t\t{key} = oz_spin_lock(&{obj}->{lock_field});\n\
             \t\t{obj}->{owner_field} = oz_current_thread();\n\
             \t}}\n\
             \t{retain}\n\
             \t{body}\n\
             \t{cleanup}\n\
             }}",
            bind = bind,
            held = held,
            key = key,
            obj = obj_var,
            owner_field = SYNC_OWNER_FIELD,
            lock_field = SYNC_LOCK_FIELD,
            retain = retain,
            body = body_text,
            cleanup = cleanup
        ),
        "id".to_string(),
    )
}

/// A `return` inside one or more `@synchronized` blocks has to run each
/// pending unlock (innermost first) before leaving. A returned value is
/// evaluated into a temporary first, so the expression still sees the
/// locked state -- `return [self compute];` must run `compute` under the
/// lock, not after it.
///
/// Mirrors the oracle's handling of the same shape, where the OZSpinLock
/// object is released by `emit.py::_emit_scope_releases` ahead of the
/// return (`tests/behavior/cases/synchronized/early_return.m`).
fn render_return_statement(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    // A local being returned hands its ownership to the caller, so it is
    // the one thing a return must not release.
    let mut cursor0 = node.walk();
    let returned_children: Vec<Node> = node.children(&mut cursor0).collect();
    let returned_name = returned_children
        .iter()
        .find(|c| c.kind() == "identifier")
        .map(|c| node_text(*c, ctx.src).to_string());
    let arc_releases = releases_for_all_scopes(ctx, returned_name.as_deref());

    // Outside any @synchronized, behave exactly as the catch-all in
    // `render_expr` would: byte-identical when nothing needs translating.
    if ctx.sync_cleanups.is_empty() && arc_releases.is_empty() {
        if !needs_translation(node) {
            return (node_text(node, ctx.src).to_string(), "id".to_string());
        }
        let rebuilt = rebuild(node, ctx, &mut |child, ctx| {
            if needs_translation(child) {
                Some(render_expr(child, ctx).0)
            } else {
                None
            }
        });
        return (rebuilt, "id".to_string());
    }

    let mut cleanup_lines: Vec<String> = arc_releases;
    cleanup_lines.extend(ctx.sync_cleanups.iter().rev().cloned());
    let cleanups = cleanup_lines.join("\n\t");

    let mut cursor = node.walk();
    let value = node.children(&mut cursor).find(|c| c.kind() != "return" && c.kind() != ";");

    match value {
        None if cleanups.is_empty() => ("return;".to_string(), "id".to_string()),
        None => (format!("{}\n\treturn;", cleanups), "id".to_string()),
        Some(value) => {
            let (value_text, _) = render_expr(value, ctx);
            ctx.block_counter += 1;
            let (line, col) = line_col(ctx.src, node.start_byte());
            let tmp = format!("_oz_sync_ret_L{}_C{}_{}", line, col, ctx.block_counter);
            let ret_ty = ctx.method_return_type.clone();
            (
                format!(
                    "{ty} {tmp} = {value};\n\t{cleanups}\n\treturn {tmp};",
                    ty = ret_ty,
                    tmp = tmp,
                    value = value_text,
                    cleanups = cleanups
                ),
                "id".to_string(),
            )
        }
    }
}

/// Is `node` an `@autoreleasepool { ... }` block? tree-sitter-objc gives
/// it no node kind of its own -- it parses as an ordinary
/// `compound_statement` whose first child is the literal token
/// `@autoreleasepool`, ahead of the usual `{`. This is the one place that
/// distinction is tested; everywhere else a bare `{ ... }` is left alone,
/// so an ordinary nested block is unaffected.
fn is_autoreleasepool_shape(node: Node) -> bool {
    if node.kind() != "compound_statement" {
        return false;
    }
    let mut cursor = node.walk();
    let first_kind = node.children(&mut cursor).next().map(|c| c.kind());
    first_kind == Some("@autoreleasepool")
}

/// `@autoreleasepool { body }` unwrapped to a plain compound statement --
/// no pool object, no drain. Matches the Python pipeline exactly
/// (`emit.py`: accepted syntactically and simply unwrapped to its inner
/// compound statement -- there is no `OZAutoreleasePool` class or
/// `-autorelease` method anywhere in this SDK). oz_static has no ARC
/// either way (#189), so there is nothing here for a real pool to drain;
/// the only thing that has to happen is dropping the `@autoreleasepool`
/// token itself, which is not a real C token and would otherwise fail to
/// compile verbatim.
///
/// Always runs when `is_autoreleasepool_shape` matches, even if nothing
/// inside the body needs translating -- unlike the ordinary "byte-
/// identical when untranslated" shortcut elsewhere, leaving the token in
/// place is never valid, so there is no shortcut to take.
fn render_autoreleasepool_statement(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    // children[0] = "@autoreleasepool", children[1] = "{", last = "}".
    let stmts = &children[2..children.len() - 1];

    // A pool block is an ordinary scope as far as ownership goes, so it does
    // the same ARC bookkeeping as `render_scoped_block` -- see `arc_enter`
    // for what went wrong while it did not.
    arc_enter(ctx, node);
    let mut ended_with_jump = false;
    let mut rendered_stmts: Vec<(String, &str)> = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        let rendered = render_expr(*stmt, ctx).0;
        let combined = if ctx.pre_stmts.is_empty() {
            rendered
        } else {
            let pre = ctx.pre_stmts.join("\n\t");
            ctx.pre_stmts.clear();
            format!("{}\n\t{}", pre, rendered)
        };
        rendered_stmts.push((combined, node_text(*stmt, ctx.src)));
        arc_note(*stmt, ctx);
        ended_with_jump = is_jump_statement(*stmt);
    }
    let releases = arc_exit(ctx, ended_with_jump);

    let mut out = String::from("{\n");
    for (rendered, original) in &rendered_stmts {
        if rendered == original {
            out.push('\t');
            out.push_str(original);
        } else {
            out.push_str("\t/* ");
            out.push_str(&one_line(original));
            out.push_str(" */\n\t");
            out.push_str(rendered);
        }
        out.push('\n');
    }
    for line in &releases {
        out.push('\t');
        out.push_str(line);
        out.push('\n');
    }
    out.push('}');
    (out, "id".to_string())
}

fn render_forin_statement(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let (line, col) = line_col(ctx.src, node.start_byte());
    if !ctx.program.is_dynamically_dispatched("iter", false)
        || !ctx.program.is_dynamically_dispatched("next", false)
    {
        ctx.err(
            node,
            format!(
                "for-in loop at {}:{} needs '-iter'/'-next' to be dispatchable on any collection type, but no protocol in this source declares them (declare an IteratorProtocol-style protocol with both, the same shape as the real Foundation one)",
                line, col
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }

    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let open = children.iter().position(|c| c.kind() == "(").unwrap();
    let in_pos = children.iter().position(|c| c.kind() == "in").unwrap();
    let close = children.iter().position(|c| c.kind() == ")").unwrap();

    let decl_nodes = &children[open + 1..in_pos];
    let declarator = *decl_nodes.last().unwrap();
    let type_nodes = &decl_nodes[..decl_nodes.len() - 1];
    let type_text = type_nodes.iter().map(|n| node_text(*n, ctx.src)).collect::<Vec<_>>().join(" ");
    let (var_name, stars) = declarator_name_and_stars(declarator, ctx.src);
    let known: std::collections::HashSet<String> = ctx.program.classes.keys().cloned().collect();
    let c_type = crate::collect::render_type(&type_text, stars, &known);

    let collection = children[in_pos + 1];
    let (coll_text, _) = render_expr(collection, ctx);
    let body = children[close + 1];

    let root = ctx.program.root_class().unwrap_or("OZObject").to_string();
    ctx.block_counter += 1;
    let iter_tmp = format!("_oz_iter_L{}_C{}_{}", line, col, ctx.block_counter);
    let recv_tmp = format!("_oz_recv_L{}_C{}_{}", line, col, ctx.block_counter);
    let next_call = format!("({})OZ_PROTOCOL_SEND_next({})", c_type, recv_tmp);

    ctx.scope.insert(var_name.clone(), c_type.clone());
    ctx.locals.insert(var_name.clone());

    let body_text = if body.kind() == "compound_statement" {
        render_body_with_comments(body, ctx)
    } else {
        let (text, _) = render_expr(body, ctx);
        format!("{{\n\t{}\n\t}}", text)
    };

    (
        format!(
            "{{\n\
             \tstruct {root} *{iter_tmp} = (struct {root} *)OZ_PROTOCOL_SEND_iter((struct {root} *)({coll_text}));\n\
             \tstruct {root} *{recv_tmp} = {iter_tmp};\n\
             \tfor ({c_type} {var_name} = {next_call}; {var_name} != ((void *)0); {var_name} = {next_call}) {body_text}\n\
             }}",
            root = root,
            iter_tmp = iter_tmp,
            recv_tmp = recv_tmp,
            coll_text = coll_text,
            c_type = c_type,
            var_name = var_name,
            next_call = next_call,
            body_text = body_text,
        ),
        "void".to_string(),
    )
}

/// The `-performSelector:` variants, in the order of the arguments they
/// pass. Must agree with `collect::prescan_reflection`'s own list, which
/// is what decides whether wrappers get generated at all.
const PERFORM_SELECTORS: &[&str] = &[
    "performSelector:",
    "performSelector:withObject:",
    "performSelector:withObject:withObject:",
];

fn render_message(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let parts = parse_message(node, ctx.src);
    let (recv_text, recv_type) = render_expr(parts.receiver, ctx);
    let arg_pairs: Vec<(String, String)> =
        parts.args.iter().map(|a| render_expr(*a, ctx)).collect();
    let arg_texts: Vec<String> = arg_pairs.iter().map(|(t, _)| t.clone()).collect();
    let root = ctx.program.root_class().unwrap_or("OZSRoot").to_string();

    if parts.selector == "retain" && parts.args.is_empty() {
        let cast_back =
            if recv_type == "id" { format!("struct {} *", root) } else { recv_type.clone() };
        return (
            format!("(({})oz_static_retain((struct {} *)({})))", cast_back, root, recv_text),
            recv_type,
        );
    }
    if parts.selector == "release" && parts.args.is_empty() {
        return (
            format!("oz_static_release((struct {} *)({}))", root, recv_text),
            "void".to_string(),
        );
    }
    if parts.selector == "retainCount" && parts.args.is_empty() {
        return (
            format!("oz_static_retain_count((struct {} *)({}))", root, recv_text),
            "int".to_string(),
        );
    }
    if parts.selector == "alloc" && parts.args.is_empty() {
        if let Some(cls) = recv_type.strip_prefix("class:") {
            return (format!("{}_oz_alloc()", cls), format!("struct {} *", cls));
        }
    }
    // `+class` on a literal class name is a compile-time constant, and
    // `-class` on a value is the `class_id` bitfield every object already
    // carries -- so neither needs a class object, and both are free.
    //
    // This has to preempt the ordinary class-method path below. `+class`
    // is declared once on the root class, so `find_defining_class` routes
    // `[Widget class]` to `OZObject_class_cls()`: the receiver's class is
    // dropped (making `[Widget class]` and `[Gadget class]` the same
    // expression) and no such function is ever generated, so the build
    // failed at link time with an undefined symbol rather than at
    // transpile time with a located message. Nothing in the corpus writes
    // `[X class]`, which is why it went unnoticed (#226).
    if parts.selector == "class" && parts.args.is_empty() {
        if let Some(cls) = recv_type.strip_prefix("class:") {
            return (format!("OZ_STATIC_CLASS_{}", cls), "Class".to_string());
        }
        return (format!("oz_class_of({})", recv_text), "Class".to_string());
    }
    // Exact class equality, so it needs no ancestry walk -- unlike
    // `-isKindOfClass:`, which `render_introspection` gates behind
    // `--introspection` because it generates a table. `oz_class_of`
    // yields `Nil` for a null receiver, which no class ever equals, so a
    // message to nil answers NO here without a separate guard.
    if parts.selector == "isMemberOfClass:" && parts.args.len() == 1 {
        return (
            format!("(oz_class_of({}) == ({}))", recv_text, arg_texts[0]),
            "BOOL".to_string(),
        );
    }
    // The two introspection selectors that need a generated table: an
    // ancestry walk over the superclass chain, and a per-protocol
    // conformance bitmap. Both are gated on `--introspection`
    // (`CONFIG_OBJZ_INTROSPECTION`) -- and when it is off they stay hard
    // located errors naming the option, so a build never quietly loses
    // them. Class *identity* above needs no table and so no gate.
    if parts.selector == "isKindOfClass:" && parts.args.len() == 1 {
        if !ctx.program.introspection {
            ctx.err(
                node,
                "'-isKindOfClass:' needs introspection, which is off -- set CONFIG_OBJZ_INTROSPECTION=y (oz2c --introspection). '-isMemberOfClass:' is always available if exact class equality will do",
            );
            return (node_text(node, ctx.src).to_string(), "BOOL".to_string());
        }
        ctx.introspection_used.kind_of = true;
        return (
            format!("oz_is_kind_of(oz_class_of({}), ({}))", recv_text, arg_texts[0]),
            "BOOL".to_string(),
        );
    }
    // `-respondsToSelector:` and the `-performSelector:` family, behind
    // `--reflection` (`CONFIG_OBJZ_REFLECTION`). The argument is an
    // ordinary `SEL` expression -- a `@selector(...)` literal or anything
    // holding one -- so there is nothing to restrict here: C's own type
    // checking covers a non-SEL argument, which is the payoff for `SEL`
    // being a real type rather than a compile-time-only spelling.
    if parts.selector == "respondsToSelector:" && parts.args.len() == 1 {
        if !ctx.program.reflection {
            ctx.err(
                node,
                "'-respondsToSelector:' needs reflection, which is off -- set CONFIG_OBJZ_REFLECTION=y (oz2c --reflection)",
            );
            return (node_text(node, ctx.src).to_string(), "BOOL".to_string());
        }
        return (
            format!("oz_responds({}, oz_class_of({}))", arg_texts[0], recv_text),
            "BOOL".to_string(),
        );
    }
    if PERFORM_SELECTORS.contains(&parts.selector.as_str())
        && parts.args.len() == parts.selector.matches(':').count()
    {
        if !ctx.program.reflection {
            ctx.err(
                node,
                format!(
                    "'-{}' needs reflection, which is off -- set CONFIG_OBJZ_REFLECTION=y (oz2c --reflection)",
                    parts.selector
                ),
            );
            return (node_text(node, ctx.src).to_string(), "id".to_string());
        }
        // Absent arguments are nil, which is what Objective-C passes when
        // a selector takes more than the `-performSelector:` variant
        // supplies. The wrapper drops the ones its selector does not want.
        let nil = "((void *)0)".to_string();
        let a0 = arg_texts.get(1).cloned().unwrap_or_else(|| nil.clone());
        let a1 = arg_texts.get(2).cloned().unwrap_or(nil);
        return (
            format!(
                "oz_perform({}, (struct {} *)({}), (void *)({}), (void *)({}))",
                arg_texts[0], root, recv_text, a0, a1
            ),
            "id".to_string(),
        );
    }
    if parts.selector == "conformsToProtocol:" && parts.args.len() == 1 {
        if !ctx.program.introspection {
            ctx.err(
                node,
                "'-conformsToProtocol:' needs introspection, which is off -- set CONFIG_OBJZ_INTROSPECTION=y (oz2c --introspection)",
            );
            return (node_text(node, ctx.src).to_string(), "BOOL".to_string());
        }
        // `render_protocol_literal` has already resolved the argument to
        // the bitmap's name and recorded the use; a non-literal argument
        // is refused by the static bar, since a protocol has no value
        // representation to pass.
        return (
            format!("oz_conforms(oz_class_of({}), {})", recv_text, arg_texts[0]),
            "BOOL".to_string(),
        );
    }
    // `+allocWithHeap:` is declared once on the root class, but it has to
    // allocate `sizeof(struct {receiver})` and stamp the receiver's own
    // class_id -- so, exactly like `+alloc`, it resolves to the *receiver's*
    // generated allocator rather than to the declaring class's. Dispatching
    // it as an ordinary class method would call
    // `OZObject_allocWithHeap__cls`, which allocates an OZObject-sized
    // block: `samples/heap_alloc` did precisely that, and it linked to
    // nothing at all because no such function is generated.
    if parts.selector == "allocWithHeap:" && parts.args.len() == 1 {
        if let Some(cls) = recv_type.strip_prefix("class:") {
            let cls = cls.to_string();
            if !ctx.program.heap_support {
                ctx.err(
                    node,
                    format!(
                        "'{}' needs heap support, which is off -- pass --heap-support (and build with -DOZ_HEAP_SUPPORT) to enable '+allocWithHeap:'",
                        one_line(node_text(node, ctx.src))
                    ),
                );
                return (node_text(node, ctx.src).to_string(), format!("struct {} *", cls));
            }
            return (
                format!(
                    "{cls}_oz_alloc_with_heap((struct {root} *)({heap}))",
                    cls = cls,
                    root = root,
                    heap = arg_texts[0]
                ),
                format!("struct {} *", cls),
            );
        }
    }

    if let Some(target) = recv_type.strip_prefix("class:") {
        let target = target.to_string();
        return match find_defining_class(ctx.program, &target, &parts.selector, true) {
            Some(defining) => {
                let (ret_ty, returns_instancetype) =
                    method_return_type(ctx.program, &defining, &parts.selector, true)
                        .unwrap_or_else(|| ("void".to_string(), false));
                let call = format!(
                    "{}({})",
                    method_fn_name(&defining, &parts.selector, true),
                    arg_texts.join(", ")
                );
                // `instancetype` covaries with the receiver, not with
                // whichever ancestor actually defines the method -- the
                // underlying C function still returns `defining`'s own
                // pointer type (one function serves every subclass), so
                // the call site casts it back up to `target`'s.
                if returns_instancetype && defining != target {
                    (format!("(struct {} *)({})", target, call), format!("struct {} *", target))
                } else {
                    (call, ret_ty)
                }
            }
            None => {
                ctx.err(
                    node,
                    format!("class '{}' has no class method matching '{}'", target, parts.selector),
                );
                ("0".to_string(), "int".to_string())
            }
        };
    }

    match class_name_from_type(&recv_type) {
        None if ctx.program.is_dynamically_dispatched(&parts.selector, false) => {
            // A bare `id` (or otherwise unresolvable) receiver -- e.g. a
            // container's own element storage, typed `id` because it can
            // hold any class -- calling a selector that isn't resolved
            // to one direct function call at compile time anyway (see
            // `Program::is_dynamically_dispatched`). No static type to
            // even attempt a direct call against, so this is the only
            // route available, not a fallback from a failed lookup.
            dynamic_dispatch_call(ctx.program, &root, &parts.selector, &recv_text, &arg_texts)
        }
        None => {
            ctx.err(
                node,
                format!(
                    "cannot statically resolve the receiver type for selector '{}' (receiver type is '{}'); the static subset requires a known declared type",
                    parts.selector, recv_type
                ),
            );
            ("0".to_string(), "int".to_string())
        }
        Some(target) => match find_defining_class(ctx.program, &target, &parts.selector, false) {
            // A `super` send names one specific implementation by
            // definition -- it must stay a direct call, never route back
            // through the receiver's own class_id (which would re-enter
            // the override that issued the send).
            Some(_)
                if !is_super_receiver(&parts, ctx)
                    && ctx.program.has_overriding_subclass(&target, &parts.selector) =>
            {
                // The receiver's *declared* type implements this selector,
                // but a subclass overrides it -- and a declared type is
                // only an upper bound on the real class (`Base *b =
                // (Base *)[Sub alloc];`). Calling the declared type's
                // implementation directly would silently run the wrong
                // one, so this needs the runtime class_id switch. Where
                // no subclass overrides, the direct call below is exact
                // and stays (see `Program::has_overriding_subclass`).
                dynamic_dispatch_call(ctx.program, &root, &parts.selector, &recv_text, &arg_texts)
            }
            Some(defining) => {
                let (ret_ty, returns_instancetype) =
                    method_return_type(ctx.program, &defining, &parts.selector, false)
                        .unwrap_or_else(|| ("void".to_string(), false));
                let mut call_args = vec![format!("(struct {} *)({})", defining, recv_text)];
                call_args.extend(arg_texts);
                let call = format!(
                    "{}({})",
                    method_fn_name(&defining, &parts.selector, false),
                    call_args.join(", ")
                );
                // `[super init]`-style sends: `render_expr`'s "super"
                // case reports `recv_type` as the *superclass*'s own
                // pointer type (needed so the call argument above casts
                // correctly) -- but the real, dynamic receiver is still
                // `self`, i.e. `ctx.class_name`'s own type, not
                // `target`'s. An `instancetype` result covaries with
                // that real receiver, so it needs casting up to
                // `ctx.class_name` here, not to `target` (which for a
                // super-send just *is* the defining class already,
                // masking the mismatch the class-message/plain-receiver
                // branch above catches via `defining != target`).
                let is_super = is_super_receiver(&parts, ctx);
                let covariant_target = if is_super { ctx.class_name.clone() } else { target.clone() };
                if returns_instancetype && defining != covariant_target {
                    (
                        format!("(struct {} *)({})", covariant_target, call),
                        format!("struct {} *", covariant_target),
                    )
                } else {
                    (call, ret_ty)
                }
            }
            None if ctx.program.is_dynamically_dispatched(&parts.selector, false) => {
                // `target` (or its superclass chain) doesn't implement
                // this selector itself, but it's dynamically dispatched
                // and some class in the program does implement it -- the
                // receiver's *static* type isn't precise enough to know
                // which one at compile time (e.g. it's typed as the root
                // class, standing in for "any conforming object"), so
                // this is the one place besides dealloc that needs a
                // runtime switch instead of a direct call.
                dynamic_dispatch_call(ctx.program, &root, &parts.selector, &recv_text, &arg_texts)
            }
            None => {
                ctx.err(node, format!("class '{}' has no method matching '{}'", target, parts.selector));
                ("0".to_string(), "int".to_string())
            }
        },
    }
}

/// Builds a `OZ_PROTOCOL_SEND_{selector}(...)` call (see
/// `companion::render_protocol_dispatch`) routing a message send
/// through the `_meta.class_id` switch -- used whenever the receiver's
/// static type doesn't pin down which class's implementation to call
/// directly, whether because it's genuinely unresolvable (a bare `id`)
/// or because it's typed as the root/a protocol, standing in for "any
/// conforming object."
fn dynamic_dispatch_call(
    program: &Program,
    root: &str,
    selector: &str,
    recv_text: &str,
    arg_texts: &[String],
) -> (String, String) {
    let selc = selector_to_c(selector);
    let mut call_args = vec![format!("(struct {} *)({})", root, recv_text)];
    call_args.extend(arg_texts.iter().cloned());
    let ret_ty = program
        .dynamic_dispatch_methods()
        .into_iter()
        .find(|m| m.selector == selector && !m.is_class_method)
        // Must agree with `companion::render_protocol_dispatch`'s own
        // choice of this function's real C return type: an
        // `instancetype` selector routes to several classes each
        // returning their *own* struct pointer, so the shared function
        // (and this call expression) can only be typed `void *`, not
        // whichever implementor's type happened to be found first.
        .map(|m| if m.returns_instancetype { "void *".to_string() } else { m.return_type })
        .unwrap_or_else(|| "void".to_string());
    (format!("OZ_PROTOCOL_SEND_{}({})", selc, call_args.join(", ")), ret_ty)
}

/// Infer a hoisted block's C return type by scanning its body for a
/// `return_statement` carrying a value. This spike has no general
/// expression-type inference (an arithmetic expression elsewhere always
/// resolves to the opaque "id" static type -- see `render_expr`'s
/// catch-all), so any returned value is assumed `int`, true of every block
/// in the current static-subset test suite. No return-with-value anywhere
/// in the body -> `void`. Does not descend into a nested `block_literal`
/// (a separate scope/function of its own).
fn infer_block_return_type(body: Node) -> &'static str {
    fn scan(node: Node) -> bool {
        if node.kind() == "block_literal" {
            return false;
        }
        if node.kind() == "return_statement" && node.named_child_count() > 0 {
            return true;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        children.into_iter().any(scan)
    }
    if scan(body) {
        "int"
    } else {
        "void"
    }
}

/// A cast expression, which needs handling for two separate reasons:
///
///   - an ARC bridging qualifier (`__bridge`, `__bridge_transfer`,
///     `__bridge_retained`) has to be dropped. It means nothing without
///     ARC and is not a C keyword, so left in place it is a compile error
///     (`use of undeclared identifier '__bridge'`). The real
///     `src/OZTimer.m` casts this way, so without this OZTimer cannot be
///     transpiled at all. Rebuilding the type through
///     `collect::extract_type_and_stars` drops it for free -- that
///     function collects only type specifiers, never a `type_qualifier`.
///     The oracle drops it too: its committed
///     `tests/zephyr/generated/OZTimer_ozm.c:28` renders that same cast
///     as plain `(void *)expBlock`.
///   - the cast's target type has to be *reported*, so a send against a
///     cast receiver resolves. `[((OZQ31 *)obj) int32Value]` otherwise
///     fails with "cannot statically resolve the receiver type ...
///     (receiver type is 'id')", since every expression not specifically
///     handled reports the opaque `id`. The oracle gets this for free from
///     Clang's own types; here the declared type is right there in the
///     cast, which is the one place a bare `id` can be narrowed back to a
///     class without inference.
///
/// A known class name in the cast is rendered `struct Name *`, matching
/// how the same name is rendered in every other type position
/// (`collect::render_type`).
fn render_cast_expression(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let descriptor = children.iter().find(|c| c.kind() == "type_descriptor").copied();
    let value = children
        .iter()
        .rev()
        .find(|c| c.kind() != ")" && c.kind() != "(" && c.kind() != "type_descriptor")
        .copied();

    let (Some(descriptor), Some(value)) = (descriptor, value) else {
        // Not the shape this handles (e.g. a compound literal); fall back
        // to the generic rebuild the catch-all would have done.
        let rebuilt = rebuild(node, ctx, &mut |child, ctx| {
            if needs_translation(child) {
                Some(render_expr(child, ctx).0)
            } else {
                None
            }
        });
        return (rebuilt, "id".to_string());
    };

    let (type_text, stars) = crate::collect::extract_type_and_stars(descriptor, ctx.src);
    let known: HashSet<String> = ctx.program.classes.keys().cloned().collect();
    let c_type = crate::collect::render_type(&type_text, stars, &known);

    let (value_text, _) = render_expr(value, ctx);
    (format!("({})({})", c_type.trim(), value_text), c_type)
}

/// Non-capturing block literal -> hoisted static C function; the block
/// expression itself is replaced with a reference to that function.
/// (Capturing blocks were already rejected by the static-bar scan.)
fn render_block(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let (line, col) = line_col(ctx.src, node.start_byte());
    ctx.block_counter += 1;
    let name = format!("oz_block_L{}_C{}_{}", line, col, ctx.block_counter);

    let mut cursor = node.walk();
    let found_plist = node.children(&mut cursor).find(|c| c.kind() == "parameter_list");
    let params = match found_plist {
        // An `id` parameter is spelled as the root class pointer, matching
        // the function-pointer *type* this block will be assigned or passed
        // to (see `collect_ivar_lowering_edits` and `render_param`). The
        // three have to agree: `samples/transpiled_generics` passes
        // `^(id obj, unsigned int idx, BOOL *stop) { ... }` to
        // `-enumerateObjectsUsingBlock:`, and with only the parameter type
        // lowered the call stopped compiling on the function pointer's type.
        Some(plist) => {
            let text = node_text(plist, ctx.src).to_string();
            match ctx.program.root_class() {
                Some(root) => replace_bare_id(&text, &format!("struct {} *", root)),
                None => text,
            }
        }
        None => "(void)".to_string(),
    };

    let mut cursor2 = node.walk();
    let body = node.children(&mut cursor2).find(|c| c.kind() == "compound_statement");
    let ret_ty = body.map(infer_block_return_type).unwrap_or("void");
    let body_text = match body {
        Some(body) => {
            // Block bodies use the same flat scope as their enclosing
            // method/function (a known spike simplification).
            collect_local_decls(body, ctx);
            render_body_with_comments(body, ctx)
        }
        None => "{\n}".to_string(),
    };

    // `(void)param;` for the block's own unused parameters. This function and
    // its signature are both synthesized here, so unlike a plain C function's
    // body there is no author's text being edited.
    let body_text = match found_plist {
        Some(plist) => {
            let names = parameter_list_names(plist, ctx.src);
            let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            let acks = acks_for_names(&body_text, &refs);
            splice_after_open_brace(&body_text, &acks)
        }
        None => body_text,
    };

    // Hoisted into the *primary* generated source (see `emit()`), not the
    // companion file: a block literal can reference a file-scope
    // static/global declared in the original source (that's not a
    // "capture" -- see staticbar.rs -- so the static bar accepts it), and
    // a `static` variable has internal linkage, invisible from any other
    // translation unit. Putting the hoisted function in the companion .c
    // instead would put it in a different translation unit from that
    // global, and it would no longer compile. A prototype is still needed
    // ahead of every call site (the function's own definition is
    // appended only once, after every class), hence still tracking both.
    let prototype = format!("{} {}{};\n", ret_ty, name, params);
    // The banner says where the literal was, not what enclosed it: since
    // #272 a literal at *file scope* is hoisted too (a block variable's
    // initializer, `static void (^g)(int) = ^(int v){ ... };`), and there is
    // no enclosing method to name. It read "hoisted out of its enclosing
    // method" until then, which was true of every caller at the time and
    // became false for the new one -- the shape of stale claim docs/STATUS.md
    // keeps recording, in generated output this time.
    let definition = format!(
        "/* block at {}:{} -- synthesized function, hoisted from a block literal */\n{} {}{} {}\n",
        line, col, ret_ty, name, params, body_text
    );
    ctx.hoisted_blocks.push((prototype, definition));
    (name.clone(), "id".to_string())
}

/// Render a `compound_statement` body. If nothing inside needed
/// translation, returned byte-identical to the original. Otherwise the
/// whole body is reformatted one-statement-per-line, tab-indented: a
/// translated statement gets its original (collapsed to one line) as a
/// `/* ... */` comment directly above the translated line; an untouched
/// statement is printed as-is. This trades exact preservation of the
/// original body's own formatting (blank lines, inline comments between
/// statements) for consistent, predictable output once a body is already
/// being annotated -- a deliberate simplification, not an oversight.
/// Nested statements (inside an if/for/etc) are still translated by the
/// ordinary recursive mechanism -- they are not re-commented at every
/// nesting level; the comment on the enclosing top-level statement is
/// what points back to source.
/// The object locals a `declaration` statement introduces that this block
/// will own, if any.
///
/// A local is owned only when its initializer is provably +1 (see
/// `arc::is_owning_expr`); a borrowed reference is left alone, because
/// releasing one is a double free. `__unsafe_unretained` opts out
/// explicitly, matching what the qualifier means everywhere else.
fn owned_locals_of(decl: Node, ctx: &EmitCtx) -> Vec<String> {
    owned_locals_of_in(decl, decl.parent(), ctx)
}

/// Is `name` released by hand somewhere under `root`?
///
/// oz_static supports manual retain/release as a feature of its own (see
/// `behavior_memory`), and a variable cannot be managed both ways: adding an
/// automatic release to code that already releases is a double free. So ARC
/// defers to the author wherever the author took control.
///
/// The oracle never has to make this choice -- its sources are compiled with
/// `-fobjc-arc`, under which an explicit `release` is a compile error, and
/// indeed no `.m` under tests/behavior/cases/ contains one. oz_static
/// accepts both styles, so it has to decide, and deferring is the only
/// option that cannot corrupt memory.
///
/// The search covers `root`'s whole subtree, so a release in a nested
/// `if`/loop counts. A release in a *sibling* scope after the declaring
/// block has ended would be missed, but such code cannot be reached anyway
/// -- the variable is out of scope there.
fn released_by_hand(name: &str, root: Node, src: &str) -> bool {
    if root.kind() == "message_expression" {
        let mut cursor = root.walk();
        let parts: Vec<Node> = root
            .children(&mut cursor)
            .filter(|c| c.kind() != "[" && c.kind() != "]")
            .collect();
        if parts.len() == 2
            && &src[parts[1].byte_range()] == "release"
            && &src[parts[0].byte_range()] == name
        {
            return true;
        }
    }
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    children.into_iter().any(|child| released_by_hand(name, child, src))
}

fn owned_locals_of_in(decl: Node, search_root: Option<Node>, ctx: &EmitCtx) -> Vec<String> {
    if node_text(decl, ctx.src).contains("__unsafe_unretained") {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cursor = decl.walk();
    let children: Vec<Node> = decl.children(&mut cursor).collect();
    for child in children {
        if child.kind() != "pointer_declarator" && child.kind() != "init_declarator" {
            continue;
        }
        // Anything ARC manages as a strong local owns whatever it ends up
        // holding (see `managed_object_locals`), so its final value needs the
        // same scope-exit release an owning initializer gets. Without this the
        // last object assigned in a loop would never be freed --
        // release-on-overwrite only covers the ones that were overwritten.
        //
        // This covers all three declaration forms the managed set admits: a
        // bare `Counter *c;`, an explicit `Counter *c = nil;`, and an owning
        // `Counter *c = [Counter alloc];`. Testing the set first rather than
        // the initializer's shape is what makes the `nil` form work -- its
        // initializer is not owning, so the check below would skip it.
        {
            let name = crate::collect::find_declared_name(child, ctx.src);
            if !name.is_empty() && ctx.arc_managed_locals.contains(&name) {
                out.push(name);
                continue;
            }
        }
        if child.kind() != "init_declarator" {
            continue;
        }
        let mut c2 = child.walk();
        let parts: Vec<Node> = child.children(&mut c2).collect();
        // `init_declarator` is `<declarator> = <value>`; the value is
        // whatever follows the `=`.
        let eq = parts.iter().position(|n| n.kind() == "=");
        let Some(value) = eq.and_then(|i| parts.get(i + 1)).copied() else {
            continue;
        };
        if !crate::arc::is_owning_expr(value, ctx.src, ctx.program, &ctx.program.owning_methods) {
            continue;
        }
        let name = crate::collect::find_declared_name(child, ctx.src);
        if name.is_empty() {
            continue;
        }
        if search_root.is_some_and(|root| released_by_hand(&name, root, ctx.src)) {
            continue;
        }
        out.push(name);
    }
    out
}

/// `oz_static_release` for each name, innermost scope first.
fn release_lines(names: &[String], ctx: &EmitCtx) -> Vec<String> {
    let root = ctx.program.root_class().unwrap_or("OZObject").to_string();
    names
        .iter()
        .map(|name| format!("oz_static_release((struct {} *)({}));", root, name))
        .collect()
}

/// Releases owed by every live scope, innermost first, skipping `keep` --
/// the local being returned, whose ownership passes to the caller.
fn releases_for_all_scopes(ctx: &EmitCtx, keep: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    for scope in ctx.arc_scopes.iter().rev() {
        for name in scope.owned.iter().rev() {
            if Some(name.as_str()) != keep {
                names.push(name.clone());
            }
        }
    }
    release_lines(&names, ctx)
}

/// Releases owed by the scopes a `break`/`continue` leaves: from the
/// innermost out to and including the nearest loop body. Scopes outside the
/// loop survive it, so their locals must not be touched.
fn releases_up_to_loop(ctx: &EmitCtx) -> Vec<String> {
    let mut names = Vec::new();
    for scope in ctx.arc_scopes.iter().rev() {
        for name in scope.owned.iter().rev() {
            names.push(name.clone());
        }
        if scope.is_loop_body {
            break;
        }
    }
    release_lines(&names, ctx)
}

/// Does this block declare an object local it would own? Only such blocks
/// need rewriting; every other one is left byte-identical, so ARC costs no
/// churn in the generated output.
fn declares_owned_local(body: Node, ctx: &EmitCtx) -> bool {
    let mut cursor = body.walk();
    let children: Vec<Node> = body.children(&mut cursor).collect();
    children
        .into_iter()
        .any(|child| child.kind() == "declaration" && !owned_locals_of(child, ctx).is_empty())
}

/// Is this compound statement a loop's body?
fn is_loop_body(body: Node) -> bool {
    body.parent().is_some_and(|parent| {
        matches!(parent.kind(), "for_statement" | "while_statement" | "do_statement")
    })
}

/// `break`/`continue`, preceded by the releases owed by every scope the
/// jump leaves.
///
/// Without this a loop-local allocated each iteration is leaked on the way
/// out, which is exactly what
/// `tests/behavior/cases/arc/break_releases_loop_local.m` detects: it breaks
/// out of a loop holding the only block of a one-block slab, then allocates
/// again and checks the allocation succeeded.
fn render_loop_jump(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let releases = releases_up_to_loop(ctx);
    let keyword = node_text(node, ctx.src);
    if releases.is_empty() {
        return (keyword.to_string(), "void".to_string());
    }
    (format!("{}\n\t{}", releases.join("\n\t"), keyword), "void".to_string())
}

/// A nested block that owns object locals: render its statements, then
/// release what it owns on the way out.
///
/// This is oz_static's ARC. The oracle does the same job by tracking
/// `ctx.scope_vars` across its whole statement emitter
/// (`emit.py::_emit_scope_releases`); here it is attached to the block that
/// actually owns the locals, so a block owning none is untouched.
///
/// A block ending in a jump gets no trailing releases -- the jump already
/// emitted them, and code after it would be unreachable anyway.
/// Enter an ARC scope for the block about to be rendered.
///
/// The three `arc_*` helpers exist so that every block renderer does the
/// same bookkeeping. They were factored out after `@autoreleasepool` was
/// found to do none of it: its arm sits before the ARC one in
/// `render_expr`'s match, so a pool block that declared an owned local got
/// the pool renderer and never the releases. `samples/heap_alloc` leaked
/// every object it allocated that way -- and it says so in its own expected
/// output, which no compile or link could have checked.
fn arc_enter(ctx: &mut EmitCtx, body: Node) {
    ctx.arc_scopes.push(ArcScope { owned: Vec::new(), is_loop_body: is_loop_body(body) });
}

/// Record whatever owned locals `stmt` just declared.
fn arc_note(stmt: Node, ctx: &mut EmitCtx) {
    if stmt.kind() != "declaration" {
        return;
    }
    let owned = owned_locals_of(stmt, ctx);
    if let Some(scope) = ctx.arc_scopes.last_mut() {
        scope.owned.extend(owned);
    }
}

/// Leave the scope, returning the releases it owes -- none when the block
/// ended in a jump, which released on its way out (`render_loop_jump` /
/// `render_return_statement`).
fn arc_exit(ctx: &mut EmitCtx, ended_with_jump: bool) -> Vec<String> {
    let scope = ctx.arc_scopes.pop().unwrap_or_default();
    if ended_with_jump {
        return Vec::new();
    }
    release_lines(&scope.owned.iter().rev().cloned().collect::<Vec<_>>(), ctx)
}

/// Did this statement leave the block by jumping?
fn is_jump_statement(node: Node) -> bool {
    matches!(
        node.kind(),
        "return_statement" | "break_statement" | "continue_statement" | "goto_statement"
    )
}

fn render_scoped_block(body: Node, ctx: &mut EmitCtx) -> (String, String) {
    let mut cursor = body.walk();
    let children: Vec<Node> = body.children(&mut cursor).collect();
    if children.len() < 2 {
        return (node_text(body, ctx.src).to_string(), "void".to_string());
    }
    let stmts = &children[1..children.len() - 1];

    arc_enter(ctx, body);
    let mut out = String::from("{\n");
    let mut ended_with_jump = false;
    for stmt in stmts {
        let rendered = render_expr(*stmt, ctx).0;
        if !ctx.pre_stmts.is_empty() {
            let pre = ctx.pre_stmts.join("\n\t");
            ctx.pre_stmts.clear();
            out.push('\t');
            out.push_str(&pre);
            out.push('\n');
        }
        out.push('\t');
        out.push_str(&rendered);
        out.push('\n');
        arc_note(*stmt, ctx);
        ended_with_jump = is_jump_statement(*stmt);
    }
    for line in arc_exit(ctx, ended_with_jump) {
        out.push('\t');
        out.push_str(&line);
        out.push('\n');
    }
    out.push('}');
    (out, "void".to_string())
}

fn render_body_with_comments(body: Node, ctx: &mut EmitCtx) -> String {
    let mut cursor = body.walk();
    let children: Vec<Node> = body.children(&mut cursor).collect();
    if children.len() < 2 {
        return node_text(body, ctx.src).to_string();
    }
    let stmts = &children[1..children.len() - 1];

    ctx.arc_scopes.push(ArcScope { owned: Vec::new(), is_loop_body: is_loop_body(body) });
    let mut rendered_stmts: Vec<(String, &str)> = Vec::with_capacity(stmts.len());
    let mut ended_with_jump = false;
    for stmt in stmts {
        let rendered = render_expr(*stmt, ctx).0;
        let combined = if ctx.pre_stmts.is_empty() {
            rendered
        } else {
            let pre = ctx.pre_stmts.join("\n\t");
            ctx.pre_stmts.clear();
            format!("{}\n\t{}", pre, rendered)
        };
        rendered_stmts.push((combined, node_text(*stmt, ctx.src)));
        if stmt.kind() == "declaration" {
            let owned = owned_locals_of(*stmt, ctx);
            if let Some(scope) = ctx.arc_scopes.last_mut() {
                scope.owned.extend(owned);
            }
        }
        ended_with_jump = matches!(
            stmt.kind(),
            "return_statement" | "break_statement" | "continue_statement" | "goto_statement"
        );
    }
    let scope = ctx.arc_scopes.pop().unwrap_or_default();
    let trailing: Vec<String> = if ended_with_jump {
        Vec::new()
    } else {
        release_lines(&scope.owned.iter().rev().cloned().collect::<Vec<_>>(), ctx)
    };
    if trailing.is_empty()
        && rendered_stmts.iter().all(|(rendered, original)| rendered == original)
    {
        return node_text(body, ctx.src).to_string();
    }

    let mut out = String::from("{\n");
    for (rendered, original) in &rendered_stmts {
        if rendered == original {
            out.push('\t');
            out.push_str(original);
        } else {
            out.push_str("\t/* ");
            out.push_str(&one_line(original));
            out.push_str(" */\n\t");
            out.push_str(rendered);
        }
        out.push('\n');
    }
    for line in &trailing {
        out.push('\t');
        out.push_str(line);
        out.push('\n');
    }
    out.push('}');
    out
}

/// Render one top-level statement/declaration node: byte-identical if
/// translation changed nothing, otherwise the original (collapsed to one
/// line) as a `/* ... */` comment followed by the translated text on its
/// own line at `indent`.
fn render_stmt_with_comment(node: Node, ctx: &mut EmitCtx, indent: &str) -> String {
    let original = node_text(node, ctx.src);
    let rendered = render_expr(node, ctx).0;
    if rendered == original {
        original.to_string()
    } else {
        format!("/* {} */\n{}{}", one_line(original), indent, rendered)
    }
}

/// One top-level `class_interface` (non-category) block. Emits a banner
/// comment wrapping the original header (name/superclass/ivars) verbatim,
/// the struct definition (root only -- see below), each declared method
/// as a `/* original */`-commented prototype, and a closing banner.
///
/// Only the root class's full struct is hoisted into the companion header
/// (`ctx.hoisted_structs`) -- oz_static_retain/release/the dealloc switch
/// need its tracking fields directly. Every other class's struct (and its
/// alloc/free, which need it for sizeof) stays in-place right here; the
/// companion only forward-declares it.
/// Returns `(header_text, alloc_free_text)` -- see the split point inside
/// for why. `emit()` recombines both into one string; `emit_split()`
/// (OZ-096) keeps them apart, routing them to a per-origin `.h`/`.c`
/// respectively.
/// ARC ownership qualifiers, meaningless without a runtime that honors
/// them, so dropped from a generated ivar. `__weak` is deliberately
/// absent -- it is rejected rather than stripped (see `lower_ivar_decl`).
const STRIPPED_IVAR_QUALIFIERS: &[&str] = &["__strong", "__unsafe_unretained", "__autoreleasing"];

/// An ivar declaration is copied into the generated struct essentially
/// verbatim, but two ObjC-only spellings are not valid C and have to be
/// lowered on the way through:
///
///   - a block-pointer declarator (`void (^_block)(id)`) becomes a plain
///     function pointer (`void (*_block)(id)`) -- the same collapse
///     `collect::detect_block_param_type` already applies to block-typed
///     method parameters, the static subset having no block runtime.
///   - an ARC ownership qualifier is dropped.
///   - a bare class name (`OZHeap *_heap;`) gains its `struct` tag. The
///     generated struct for a class is `struct Name`, never a typedef, so
///     the untagged spelling is `error: must use 'struct' tag to refer to
///     type 'OZHeap'`. Every other type position already routes through
///     `collect::render_type` for this; an ivar declaration is copied
///     through as text, so it has to be done here. `struct OZHeap
///     *_heap;` is left alone -- its name sits under a `struct_specifier`
///     rather than being a direct child, so it never matches.
///
/// `__weak` is a hard error rather than a silent strip: with no runtime
/// to zero the reference it would behave as an unretained strong ivar,
/// which is the exact bug the qualifier exists to prevent. Mirrors
/// `collect::extract_property`'s rejection of `weak` properties.
///
/// Edits are applied back-to-front so earlier byte ranges stay valid.
fn lower_ivar_decl(instance_variable: Node, ctx: &mut EmitCtx) -> String {
    let origin = instance_variable.start_byte();
    let mut text = node_text(instance_variable, ctx.src).to_string();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    collect_ivar_lowering_edits(instance_variable, ctx, origin, &mut edits);
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (range, replacement) in edits {
        text.replace_range(range, &replacement);
    }
    // A stripped qualifier leaves behind the whitespace it sat in.
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace(" ;", ";")
}

/// Seed a plain C function's parameters into `ctx`, the way
/// `render_method_definition` seeds a method's (#250).
///
/// Without this a free function's scope was `file_scope_vars` and nothing
/// else, so `[w n]` on a `Widget *w` parameter was rejected as an `id`
/// receiver while the identical method `- (int)read:(Widget *)w` resolved.
/// Same class of omission as gap Q, where the static bar turned out never to
/// scan a free function at all: the free-function path kept getting a reduced
/// version of what a method body gets.
///
/// Every parameter is inserted, not only the object-typed ones, because that
/// is what a method does and the two paths drifting is what produces this
/// shape of bug (#246, gap R).
///
/// Adding them to `ctx.locals` cannot make ARC release a borrowed parameter:
/// `managed_object_locals` looks for `declaration` nodes *inside the body*,
/// and a parameter is a `parameter_declaration` outside it.
fn collect_function_params(func_node: Node, ctx: &mut EmitCtx) {
    let known: std::collections::HashSet<String> = ctx.program.classes.keys().cloned().collect();
    let mut lists = Vec::new();
    find_parameter_lists(func_node, &mut lists);
    // The first list is the function's own: `find_parameter_lists` stops
    // descending once it matches, so a function-pointer parameter's own
    // parameter list is never mistaken for it.
    let Some(plist) = lists.first() else {
        return;
    };
    let mut cursor = plist.walk();
    for child in plist.children(&mut cursor) {
        if child.kind() != "parameter_declaration" {
            continue;
        }
        let (type_text, stars) = crate::collect::extract_type_and_stars(child, ctx.src);
        let c_type = crate::collect::render_type(&type_text, stars, &known);
        let name = crate::collect::find_declared_name(child, ctx.src);
        if !name.is_empty() {
            ctx.scope.insert(name.clone(), c_type);
            ctx.locals.insert(name);
        }
    }
}

/// Every `parameter_list` under `node`.
fn find_parameter_lists<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    if node.kind() == "parameter_list" {
        out.push(node);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_parameter_lists(child, out);
    }
}

/// Rewrite every bare `id` type name under `node` to `replacement`.
///
/// Both node kinds a bare `id` can appear as are handled: `type_identifier`
/// where the grammar reads it as an ordinary type name, and
/// `typedefed_specifier` where it reads it as a typedef reference. A
/// declarator's own `*` is a separate token and is left alone, so `id *`
/// becomes `struct Root **` as it should.
fn rewrite_id_types(
    node: Node,
    src: &str,
    origin: usize,
    replacement: &str,
    edits: &mut Vec<(Range<usize>, String)>,
) {
    if matches!(node.kind(), "type_identifier" | "typedefed_specifier")
        && node_text(node, src).trim() == "id"
    {
        edits.push((node.start_byte() - origin..node.end_byte() - origin, replacement.to_string()));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        rewrite_id_types(child, src, origin, replacement, edits);
    }
}

fn collect_ivar_lowering_edits(
    node: Node,
    ctx: &mut EmitCtx,
    origin: usize,
    edits: &mut Vec<(Range<usize>, String)>,
) {
    if node.kind() == "struct_declaration" {
        // Only a *direct* type_identifier child is an untagged type name;
        // in `struct OZHeap *x` the name hangs off a `struct_specifier`.
        let mut cursor = node.walk();
        let bare = node
            .children(&mut cursor)
            .find(|c| c.kind() == "type_identifier")
            .map(|c| (c.byte_range(), node_text(c, ctx.src).to_string()));
        if let Some((range, name)) = bare {
            if ctx.program.is_class(&name) {
                edits.push((
                    range.start - origin..range.end - origin,
                    format!("struct {}", name),
                ));
            }
        }

        // A function-pointer ivar's own parameter list: an `id` there is
        // spelled as the root class pointer rather than left to the `id`
        // typedef.
        //
        // The field's type is what external C code has to match when it
        // assigns to the field, so it has to be the honest one. `OZDefer`'s
        // ivar is `void (^_block)(id)`, and with `id` left as a typedef for
        // `void *` the field came out `void (*)(void *)` -- so assigning an
        // ordinary `void (*)(struct OZObject *)` function to it was
        // "incompatible function pointer types", which is exactly what
        // `tests/behavior/cases/foundation/defer_block_ivar`'s driver does.
        // The Python backend's field type is `void (*)(struct OZObject *)`
        // too, since its own `id` typedef is `struct OZObject *`.
        //
        // Not in `collect::render_type`, which keeps resolving a *method's*
        // `id` to `void *`: a method's arguments pass through oz_static's own
        // casts at every call site, and `void *` is what lets a concrete
        // class pointer reach an `id` parameter without one. Making `id`
        // itself the root pointer everywhere was tried and is worse -- it
        // turns the ordinary Objective-C idiom of passing `Foo *` where `id`
        // is expected into a warning, in code that has no call site to cast
        // at either.
        //
        // The parameter list hangs off this declaration, not off the
        // `block_pointer_declarator`, whose only children are the `^` and
        // the field name.
        if let Some(root) = ctx.program.root_class() {
            let replacement = format!("struct {} *", root);
            let mut lists = Vec::new();
            find_parameter_lists(node, &mut lists);
            for list in lists {
                rewrite_id_types(list, ctx.src, origin, &replacement, edits);
            }
        }
    }
    match node.kind() {
        "type_qualifier" => {
            let text = node_text(node, ctx.src).trim();
            if text == "__weak" {
                ctx.err(
                    node,
                    "'__weak' ivars are not supported (nothing zeroes a weak reference without a \
                     runtime, so it would silently behave as an unretained strong ivar) -- use \
                     '__unsafe_unretained' and clear it explicitly",
                );
            } else if STRIPPED_IVAR_QUALIFIERS.contains(&text) {
                edits.push((node.start_byte() - origin..node.end_byte() - origin, String::new()));
            }
            return;
        }
        "block_pointer_declarator" => {
            let mut c = node.walk();
            let caret = node.children(&mut c).find(|n| n.kind() == "^").map(|n| n.byte_range());
            if let Some(caret) = caret {
                edits.push((caret.start - origin..caret.end - origin, "*".to_string()));
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ivar_lowering_edits(child, ctx, origin, edits);
    }
}

fn render_interface(node: Node, ctx: &mut EmitCtx, program: &Program) -> (String, String) {
    let name = ctx.class_name.clone();
    let info = &program.classes[&name];

    for protocol in &info.conforms {
        for required in program.protocol_methods(protocol) {
            let implemented = info.methods.iter().any(|m| {
                m.selector == required.selector && m.is_class_method == required.is_class_method
            });
            if !implemented {
                ctx.err(
                    node,
                    format!(
                        "'{}' declares conformance to '{}' but doesn't implement '{}'",
                        name, protocol, required.selector
                    ),
                );
            }
        }
    }
    let base_field = match &info.superclass {
        Some(sup) => format!("\tstruct {sup} base; /* synthesized: inherited from {sup} */\n", sup = sup),
        // Root class: synthesize the tracking fields every object needs.
        None => {
            // `struct oz_metadata` is the PAL's own type
            // (`platform/oz_platform_types.h`): a packed bitfield holding
            // class_id, heap_allocated, deallocating and immortal. Using it
            // rather than a hand-rolled set of `uint8_t` siblings costs
            // nothing to adopt, is what the Python backend's root struct
            // already does, and folds four flags into the four bytes one of
            // them used to take on its own.
            //
            // It also settles a naming question the two backends had
            // answered differently for no reason: three of the behavior
            // corpus's drivers assert on `obj->base._meta.class_id`, and no
            // `#define` can rewrite `a._meta.b` into a flat `a.oz_b` -- the
            // names are separate tokens joined by `.`. They were
            // unbuildable purely because of the spelling.
            //
            // `_refcount` stays a sibling, exactly as in the oracle's own
            // root struct: it is `oz_atomic_t`, not a bitfield, and every
            // driver reaches it through `__objc_refcount_get` anyway.
            let mut f = String::from(
                "\tstruct oz_metadata _meta; /* synthesized: class_id, and the deallocating/heap/immortal flags */\n\
                 \toz_atomic_t oz_refcount; /* synthesized: retain count */\n",
            );
            // Shared lock for every atomic property in the program --
            // reached from any class via `Program::ivar_access_path`'s
            // ordinary "base." hop-chain, same as any inherited ivar.
            if program.has_atomic_property() {
                f.push_str(
                    "\toz_spinlock_t oz_prop_lock; /* synthesized: guards atomic property access */\n",
                );
            }
            // One lock per object, so `@synchronized(obj)` excludes on `obj`
            // rather than on a lock the caller happened to have on its own
            // stack -- which excluded nothing between cores. Zero-initialized
            // for free: `{Class}_oz_alloc` memsets the whole object, a static
            // boxed literal is zero-initialized by C, and `oz_spin_init` is
            // itself a memset. Costs nothing on a single-core target, where
            // `struct k_spinlock` has no members at all.
            if program.uses_synchronized {
                f.push_str(
                    "\toz_spinlock_t oz_sync_lock; /* synthesized: guards @synchronized(self) */\n\
                     \tvoid *oz_sync_owner; /* synthesized: thread holding oz_sync_lock, 0 when free */\n",
                );
            }
            f
        }
    };

    let mut ivars_text = String::new();
    let mut cursor = node.walk();
    if let Some(vars_node) = node.children(&mut cursor).find(|c| c.kind() == "instance_variables") {
        let mut c2 = vars_node.walk();
        for child in vars_node.children(&mut c2) {
            if child.kind() == "instance_variable" {
                // `@public`/`@private`/`@protected`/`@package` each get their
                // own `instance_variable` wrapper holding nothing but a
                // `visibility_specification`. They are ObjC access control
                // with no C equivalent, and copied through they are a syntax
                // error in the generated struct -- "type name requires a
                // specifier or qualifier", which samples/hello_category's Car
                // hit. Dropping them leaves every field reachable, which the
                // generated C already was: nothing enforced visibility once
                // the struct became plain C.
                if child_by_kind_local(child, "visibility_specification").is_some() {
                    continue;
                }
                ivars_text.push('\t');
                let lowered = lower_ivar_decl(child, ctx);
                ivars_text.push_str(&lowered);
                ivars_text.push('\n');
            }
        }
    }
    // A property's backing ivar usually is one of the ones just copied
    // above (both real Foundation classes declare theirs explicitly) --
    // but if a property's ivar isn't declared anywhere in source (fully
    // implicit synthesis), the struct still needs a field for it.
    let known: std::collections::HashSet<String> = program.classes.keys().cloned().collect();
    let raw_ivar_names: std::collections::HashSet<String> =
        crate::collect::extract_ivars(node, ctx.src, &known).into_iter().map(|(n, _)| n).collect();
    let mut emitted: std::collections::HashSet<String> = raw_ivar_names.clone();
    for prop in &info.properties {
        if let Some(ivar) = &prop.ivar_name {
            if !raw_ivar_names.contains(ivar) {
                ivars_text.push_str(&format!(
                    "\t{} {}; /* synthesized: backs property '{}' */\n",
                    prop.c_type, ivar, prop.name
                ));
                emitted.insert(ivar.clone());
            }
        }
    }
    // An ivar declared in the `@implementation` block rather than the
    // `@interface` (valid modern Objective-C, and what
    // `samples/hello_category`'s Car does) was collected onto the class but
    // is not in *this* node's text, since that text is the interface. Add
    // whatever the class owns that has not been emitted yet, or the struct
    // silently lacks the field and every use is "use of undeclared
    // identifier".
    for (ivar, c_type) in &info.own_ivars {
        if emitted.contains(ivar) {
            continue;
        }
        // `oz_prop_lock` and friends are synthesized onto the root class by
        // `collect::resolve_properties` and already emitted above as part of
        // its tracking fields; re-emitting one here is a duplicate member.
        // User ivars are `_`-prefixed by convention, so the `oz_` namespace
        // is unambiguous.
        if ivar.starts_with("oz_") {
            continue;
        }
        ivars_text.push_str(&format!(
            "\t{} {}; /* from the @implementation block */\n",
            c_type, ivar
        ));
        emitted.insert(ivar.clone());
    }

    let struct_text =
        format!("struct {name} {{\n{base}{ivars}}};\n", name = name, base = base_field, ivars = ivars_text);

    let open_banner = banner_box(&header_text(node, ctx.src, &["method_declaration"]), '=');
    let close_banner = banner_rule(&format!("end interface: {}", name), '=');

    // Each declared method: its own line(s) as a comment, then the
    // prototype. Any method known to the class but NOT declared in this
    // @interface (e.g. only ever defined in @implementation) still gets
    // a plain prototype -- just without a "from source" comment, since
    // there's no interface declaration to show.
    let mut declared: std::collections::HashSet<(String, bool)> = std::collections::HashSet::new();
    let mut decls = String::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "method_declaration" {
            let known: std::collections::HashSet<String> = program.classes.keys().cloned().collect();
            let sig = crate::collect::extract_method_sig(child, ctx.src, &name, &known);
            declared.insert((sig.selector.clone(), sig.is_class_method));
            decls.push_str(&format!("/* {} */\n", one_line(node_text(child, ctx.src))));
            decls.push_str(&render_prototype(&name, &sig, ctx.program.root_class()));
        }
    }
    for m in &info.methods {
        if !declared.contains(&(m.selector.clone(), m.is_class_method)) {
            decls.push_str(&render_prototype(&name, m, ctx.program.root_class()));
        }
    }

    // Split in two so a per-origin `.h` (OZ-096) can take just the
    // struct-and-prototypes half -- true header content -- without the
    // alloc/free *function bodies* the non-root branch also generates
    // here (an existing quirk of this spike: root's own alloc/free
    // lives in the shared companion.c, via `companion::render`, but
    // every other class's is generated in-place, right where its own
    // struct is visible). `emit()` (single combined file, unchanged
    // behavior) just concatenates both parts back together; no test
    // checks `source_c`'s exact text, only that it compiles and runs.
    if info.superclass.is_none() {
        // Root: full struct hoisted to the companion; only the banner +
        // method prototypes stay in-place.
        ctx.hoisted_structs.push((name.clone(), struct_text));
        (format!("{}{}{}", open_banner, decls, close_banner), String::new())
    } else {
        let root = program.root_class().unwrap_or(&name).to_string();
        // `{name}_oz_alloc`/`_oz_free` already get a prototype from the
        // shared companion header (every class does) -- but OZArray's/
        // OZDictionary's *extra* boxed-literal builder has no prototype
        // anywhere. That was fine when everything landed in one
        // translation unit (define-before-use), but a caller in a
        // different file (e.g. `main.c`'s own `@[...]` literal) needs
        // an explicit declaration once each class gets its own file.
        let slots = ctx.pools.for_class(&name);
        let owned_ivars = ctx.program.owned_object_ivars(&name);
        let (alloc_free, extra_proto) = if name == "OZArray" {
            (
                crate::companion::render_array_support(
                    &name,
                    &root,
                    slots,
                    &owned_ivars,
                    ctx.program.heap_support,
                    ctx.pools.item_slots(),
                ),
                format!("struct {name} *{name}_oz_initWithItems(void **src, unsigned int count);\n", name = name),
            )
        } else if name == "OZDictionary" {
            (
                crate::companion::render_dict_support(
                    &name,
                    &root,
                    slots,
                    &owned_ivars,
                    ctx.program.heap_support,
                    ctx.pools.item_slots(),
                ),
                format!(
                    "struct {name} *{name}_oz_initWithKeysValues(void **keys, void **values, unsigned int count);\n",
                    name = name
                ),
            )
        } else {
            (
                crate::companion::render_alloc_free(
                    &name,
                    &root,
                    slots,
                    &owned_ivars,
                    ctx.program.heap_support,
                    ctx.program
                        .class_conforms_to(&name, crate::companion::SINGLETON_PROTOCOL),
                ),
                String::new(),
            )
        };
        (format!("{}{}\n{}{}{}", open_banner, struct_text, extra_proto, decls, close_banner), alloc_free)
    }
}

pub(crate) fn render_prototype(
    class_name: &str,
    m: &crate::model::MethodSig,
    root: Option<&str>,
) -> String {
    // Answered at the call site, never emitted as a function -- so a
    // prototype here would declare a symbol that is defined nowhere. That
    // is precisely how `+class` used to fail: declared by `OZObject.h`,
    // called as `OZObject_class_cls()`, defined by nothing, undefined at
    // link time (#226).
    if crate::staticbar::INTRINSIC_SELECTORS.contains(&m.selector.as_str()) {
        return String::new();
    }
    let mut params = String::new();
    if !m.is_class_method {
        params.push_str(&format!("struct {} *self", class_name));
    }
    for (pname, ptype) in &m.params {
        if !params.is_empty() {
            params.push_str(", ");
        }
        params.push_str(&render_param(ptype, pname, root));
    }
    if params.is_empty() {
        params = "void".to_string();
    }
    let fn_name = method_fn_name(class_name, &m.selector, m.is_class_method);
    format!("{} {}({});\n", m.return_type, fn_name, params)
}

/// One category `class_interface (Category)` block -> banner + each
/// declared method as a `/* original */`-commented prototype.
fn render_category_interface(node: Node, src: &str, name: &str, program: &Program) -> String {
    let info = &program.classes[name];
    let open_banner = banner_box(&header_text(node, src, &["method_declaration"]), '=');
    let close_banner = banner_rule(&format!("end interface: {} (category)", name), '=');
    let mut decls = String::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "method_declaration" {
            let known: std::collections::HashSet<String> = program.classes.keys().cloned().collect();
            let sig = crate::collect::extract_method_sig(child, src, name, &known);
            decls.push_str(&format!("/* {} */\n", one_line(node_text(child, src))));
            decls.push_str(&render_prototype(name, &sig, program.root_class()));
        }
    }
    let _ = info; // reserved: category-only method filtering could go here
    format!("{}{}{}", open_banner, decls, close_banner)
}

/// A synthesized property getter (`is_getter`) or setter -- ported 1:1
/// from the Python pipeline's `emit.py::_emit_synthesized_accessor`:
/// atomic (the default unless `nonatomic`) wraps the ivar access in
/// `OZ_SPINLOCK` on the shared root `oz_prop_lock` field (a real
/// spinlock on Zephyr, a no-op `if` on host -- see
/// `platform/oz_platform_{zephyr,host}.h`); a strong object setter also
/// retains the incoming value and releases the old one, via this
/// codebase's own `oz_static_retain`/`oz_static_release` (not Python's
/// `{root}_retain`, which doesn't exist here -- see `render_message`'s
/// `-retain`/`-release` translation for the same pattern).
fn render_synthesized_accessor(
    class_name: &str,
    prop: &crate::model::PropertyInfo,
    is_getter: bool,
    program: &Program,
) -> String {
    let ivar = prop.ivar_name.as_deref().unwrap_or(&prop.name);
    let ivar_path = program.ivar_access_path(class_name, ivar).unwrap_or_else(|| ivar.to_string());
    let is_atomic = !prop.is_nonatomic;
    let lock_path =
        if is_atomic { program.ivar_access_path(class_name, "oz_prop_lock") } else { None }
            .map(|p| format!("self->{}", p));
    let c_type = &prop.c_type;

    let (selector, ret_ty, params_decl) = if is_getter {
        let sel = prop.getter_sel.clone().unwrap_or_else(|| prop.name.clone());
        (sel, c_type.clone(), format!("struct {} *self", class_name))
    } else {
        let sel = prop.setter_sel.clone().unwrap_or_else(|| crate::collect::default_setter_sel(&prop.name));
        (sel, "void".to_string(), format!("struct {} *self, {}", class_name, render_param(c_type, &prop.name, program.root_class())))
    };
    let fn_name = method_fn_name(class_name, &selector, false);

    let mut body = String::from("{\n");
    if is_getter {
        if let Some(lock) = &lock_path {
            body.push_str(&format!(
                "\t{ty} val = {{0}};\n\tOZ_SPINLOCK(&{lock}) {{\n\t\tval = self->{ivar};\n\t}}\n\treturn val;\n",
                ty = c_type,
                lock = lock,
                ivar = ivar_path
            ));
        } else {
            body.push_str(&format!("\treturn self->{};\n", ivar_path));
        }
    } else {
        let param_name = &prop.name;
        let is_strong_obj = prop.is_object && prop.ownership == crate::model::Ownership::Strong;
        let root = program.root_class().unwrap_or("OZSRoot").to_string();
        if is_strong_obj {
            if let Some(lock) = &lock_path {
                body.push_str(&format!(
                    "\t{ty} old = {{0}};\n\toz_static_retain((struct {root} *){param});\n\tOZ_SPINLOCK(&{lock}) {{\n\t\told = self->{ivar};\n\t\tself->{ivar} = {param};\n\t}}\n\toz_static_release((struct {root} *)old);\n",
                    ty = c_type,
                    root = root,
                    param = param_name,
                    lock = lock,
                    ivar = ivar_path
                ));
            } else {
                body.push_str(&format!(
                    "\t{ty} old = self->{ivar};\n\tself->{ivar} = {param};\n\toz_static_retain((struct {root} *){param});\n\toz_static_release((struct {root} *)old);\n",
                    ty = c_type,
                    ivar = ivar_path,
                    param = param_name,
                    root = root
                ));
            }
        } else if let Some(lock) = &lock_path {
            body.push_str(&format!(
                "\tOZ_SPINLOCK(&{lock}) {{\n\t\tself->{ivar} = {param};\n\t}}\n",
                lock = lock,
                ivar = ivar_path,
                param = param_name
            ));
        } else {
            body.push_str(&format!("\tself->{} = {};\n", ivar_path, param_name));
        }
    }
    body.push('}');

    format!(
        "/* synthesized {} for property '{}' */\n{} {}({})\n{}\n",
        if is_getter { "getter" } else { "setter" },
        prop.name,
        ret_ty,
        fn_name,
        params_decl,
        body
    )
}

fn render_method_definition(
    node: Node,
    ctx: &mut EmitCtx,
    class_name: &str,
    ivars_scope: &HashMap<String, String>,
) -> String {
    let known: std::collections::HashSet<String> = ctx.program.classes.keys().cloned().collect();
    let sig = crate::collect::extract_method_sig(node, ctx.src, class_name, &known);

    ctx.scope = ivars_scope.clone();
    ctx.locals.clear();
    for (pname, ptype) in &sig.params {
        ctx.scope.insert(pname.clone(), ptype.clone());
        ctx.locals.insert(pname.clone());
    }

    let defining = find_defining_class(ctx.program, class_name, &sig.selector, sig.is_class_method)
        .unwrap_or_else(|| class_name.to_string());
    let ret_ty = method_return_type(ctx.program, &defining, &sig.selector, sig.is_class_method)
        .map(|(t, _)| t)
        .unwrap_or_else(|| sig.return_type.clone());

    let mut sig_params = String::new();
    if !sig.is_class_method {
        sig_params.push_str(&format!("struct {} *self", class_name));
    }
    for (pname, ptype) in &sig.params {
        if !sig_params.is_empty() {
            sig_params.push_str(", ");
        }
        sig_params.push_str(&render_param(ptype, pname, ctx.program.root_class()));
    }
    if sig_params.is_empty() {
        sig_params = "void".to_string();
    }
    let fn_name = method_fn_name(class_name, &sig.selector, sig.is_class_method);

    let mut cursor = node.walk();
    let body = node.children(&mut cursor).find(|c| c.kind() == "compound_statement");

    // The header comment covers just the original signature (through
    // the last param), not the body -- the body gets its own
    // per-statement comments below.
    let header = header_text(node, ctx.src, &["compound_statement"]);

    // Needed by `render_return_statement` to type the temporary a
    // `return` inside `@synchronized` evaluates into.
    ctx.method_return_type = ret_ty.clone();

    // Whether the body was really translated. A body the static bar rejected
    // is passed through as its original text and the whole transpile is going
    // to fail, so there is nothing to tidy and no output anyone will compile.
    let mut translated = body.is_none();
    let body_text = match body {
        Some(body) => {
            let class_info = ctx.program.classes[class_name].clone();
            let reject_diags = crate::staticbar::check_method_body(
                body, ctx.src, ctx.program, &class_info, &sig.params, &sig.selector,
            );
            if !reject_diags.is_empty() {
                ctx.diags.extend(reject_diags);
                node_text(body, ctx.src).to_string()
            } else {
                translated = true;
                collect_local_decls(body, ctx);
                render_body_with_comments(body, ctx)
            }
        }
        None => "{\n}".to_string(),
    };

    // `(void)x;` for each parameter the body never mentions -- see
    // `unused_param_acks`. Only for a body oz_static itself produced: a plain C
    // function's body is the author's own text, patched in place, and adding
    // acknowledgements to code someone wrote is not this pass's business.
    let body_text = if translated {
        let acks = unused_param_acks(&body_text, &sig.params, sig.is_class_method);
        splice_after_open_brace(&body_text, &acks)
    } else {
        body_text
    };

    format!("/* {} */\n{} {}({})\n{}\n", one_line(&header), ret_ty, fn_name, sig_params, body_text)
}

pub struct EmitOutput {
    pub source_c: String,
    pub companion_h: String,
    pub companion_c: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Single-translation-unit assembler: the whole program as one `source_c`,
/// plus the shared companion pair. This is the form `transpile()` exposes
/// and the one the Rust suite drives.
///
/// It used to be a top-level walk of its own, which is how it managed to
/// disagree with the shipped one four times (#254). It is now the same
/// `walk_top_level` with a different assembly, so a node kind cannot be
/// handled there and not here.
///
/// One synthetic origin covers the whole text: at this level there are no
/// `#import`s resolved into the source and so no header/implementation
/// provenance to distinguish, which is why nothing is passed for
/// `header_ranges`. Every construct therefore lands under the same stem,
/// and the two buckets become an ordering rather than two files --
/// declarations first, bodies after, which is what C requires of a single
/// translation unit anyway.
pub fn emit(
    source: &str,
    program: &Program,
    pools: &crate::pools::PoolSizes,
    repaired_semicolons: &[usize],
) -> EmitOutput {
    let origins = [("main".to_string(), 0..source.len())];
    let walked = walk_top_level(source, program, pools, &origins, &[], repaired_semicolons);

    // One stem in practice, but driven off `stem_order` rather than the
    // maps' own iteration order, which a `HashMap` does not promise.
    let per_stem = |m: &HashMap<String, Vec<(String, String)>>| -> Vec<(String, String)> {
        walked.stem_order.iter().filter_map(|s| m.get(s)).flatten().cloned().collect()
    };
    let statics = per_stem(&walked.hoisted_statics_by_stem);
    let blocks = per_stem(&walked.hoisted_blocks_by_stem);
    let strings = per_stem(&walked.hoisted_strings_by_stem);

    let mut out = String::from(
        "/* Auto-generated by oz_static -- do not edit */\n#include \"oz_static_dispatch.h\"\n\n",
    );

    // A promoted `__block` local is a self-contained
    // `static TYPE name [= init];` line, so unlike the blocks and literals
    // below it needs no prototype/definition split: it only has to precede
    // every reference to it, which living up here guarantees.
    if !statics.is_empty() {
        out.push_str("/* __block-qualified locals, promoted to file scope */\n");
        for (_, decl) in &statics {
            out.push_str(decl);
            out.push('\n');
        }
        out.push('\n');
    }

    // Prototypes ahead of every call site, definitions once at the very
    // end. A hoisted block or boxed literal can be used by a class that
    // appears earlier in the text than the type its own definition needs
    // (`struct OZString` is defined at OZString's `@interface`), so the
    // definition cannot go where the prototype does.
    if !blocks.is_empty() {
        out.push_str("/* non-capturing blocks, hoisted out of their enclosing methods -- prototypes (defined below, after every class) */\n");
        for (prototype, _) in &blocks {
            out.push_str(prototype);
        }
        out.push('\n');
    }
    if !strings.is_empty() {
        out.push_str("/* boxed string literals, hoisted -- extern forward declarations (defined below, after every class) */\n");
        for (prototype, _) in &strings {
            out.push_str(prototype);
        }
        out.push('\n');
    }

    for stem in &walked.stem_order {
        if let Some(sections) = walked.headers.get(stem) {
            out.push_str(&sections.join("\n"));
            out.push('\n');
        }
    }
    for stem in &walked.stem_order {
        if let Some(sections) = walked.bodies.get(stem) {
            out.push_str(&sections.join("\n\n"));
            out.push('\n');
        }
    }

    if !blocks.is_empty() {
        out.push_str("\n/* non-capturing blocks, hoisted out of their enclosing methods */\n");
        for (_, definition) in &blocks {
            out.push_str(definition);
            out.push('\n');
        }
    }
    if !strings.is_empty() {
        out.push_str("\n/* boxed string literals, hoisted -- static struct OZString instances */\n");
        for (_, definition) in &strings {
            out.push_str(definition);
        }
    }

    let (companion_h, companion_c) = crate::companion::render(
        program,
        &walked.hoisted_structs,
        &walked.hoisted_enums,
        &walked.hoisted_forward_decls,
        &walked.hoisted_c_structs,
        pools,
        &crate::imports::collect_system_includes(source),
        &walked.introspection_used,
    );

    EmitOutput { source_c: out, companion_h, companion_c, diagnostics: walked.diags }
}

pub struct EmitSplitOutput {
    /// One `(stem, header_h, source_c)` triple per origin file, in
    /// first-seen (textual) order.
    pub files: Vec<(String, String, String)>,
    pub companion_h: String,
    pub companion_c: String,
    pub diagnostics: Vec<Diagnostic>,
}

fn note_stem(order: &mut Vec<String>, stem: &str) {
    if !order.iter().any(|s| s == stem) {
        order.push(stem.to_string());
    }
}

/// Everything the top-level walk produces, before either assembler has
/// decided where to put it.
///
/// Each construct is bucketed by which origin file it came from and by
/// whether it is interface-shaped (struct + prototypes, no bodies --
/// exactly what `class_interface` renders as) or implementation-shaped
/// (method bodies -- exactly what `class_implementation` renders as). What
/// an assembler then does with a bucket is a placement decision: one file
/// per origin (`emit_split`) or one translation unit (`emit`).
struct TopLevel {
    /// Origin stems in first-seen (textual) order.
    stem_order: Vec<String>,
    headers: HashMap<String, Vec<String>>,
    bodies: HashMap<String, Vec<String>>,
    /// Stems whose generated `.h` must include another stem's `.h`, because
    /// a class there embeds a non-root superclass by value.
    extra_includes: HashMap<String, HashSet<String>>,
    hoisted_blocks_by_stem: HashMap<String, Vec<(String, String)>>,
    hoisted_strings_by_stem: HashMap<String, Vec<(String, String)>>,
    hoisted_statics_by_stem: HashMap<String, Vec<(String, String)>>,
    /// Destined for the shared companion header rather than any one
    /// origin's -- see `companion::render`.
    hoisted_structs: Vec<(String, String)>,
    hoisted_enums: Vec<String>,
    hoisted_forward_decls: Vec<String>,
    hoisted_c_structs: Vec<String>,
    /// Which origin owns each class's declaration.
    class_to_stem: HashMap<String, String>,
    /// See `IntrospectionUse`.
    introspection_used: IntrospectionUse,
    diags: Vec<Diagnostic>,
}

/// The one walk over the top-level nodes, and so the one place a node kind
/// is handled (#254).
///
/// There used to be two: this one and a second inside `emit()`, each with
/// its own match on `node.kind()`. They disagreed about what valid output
/// looks like four separate times -- gap R (#240), #246, #250 and #251 --
/// and none of those was a forgotten case so much as two places answering
/// the same question (*is this a local? is this an object declaration? does
/// this type need a tag?*) with nothing forcing them to answer alike. The
/// asymmetry bit in both directions, so neither walk was simply the more
/// complete one: #246 was `emit()` missing a `declaration` arm outright,
/// while gap C's seventh cause was the split walk *dropping* a top-level
/// struct that `emit()` kept by not touching it.
///
/// Both entry points now call this, and differ only in how they assemble
/// what it returns. Adding a node kind here reaches both by construction,
/// which is the property the four fixes above each restored by hand.
///
/// `origins` is `imports::ResolvedSource::origins`: an ordered list of
/// `(stem, byte_range)` covering every byte of `source` (the same stem
/// may appear more than once, non-contiguously). `emit()` passes a single
/// synthetic origin covering the whole text.
fn walk_top_level(
    source: &str,
    program: &Program,
    pools: &crate::pools::PoolSizes,
    origins: &[(String, Range<usize>)],
    header_ranges: &[Range<usize>],
    repaired_semicolons: &[usize],
) -> TopLevel {
    let tree = crate::parse::parse(source);
    let root = tree.root_node();
    let file_vars = file_scope_vars(root, source, program);

    // Did this byte come from a header? See
    // `imports::ResolvedSource::header_ranges` for why it matters.
    let from_header = |byte: usize| -> bool {
        header_ranges.iter().any(|r| r.contains(&byte))
    };
    let origin_for = |byte: usize| -> String {
        origins.iter().find(|(_, r)| r.contains(&byte)).map(|(s, _)| s.clone()).unwrap_or_else(|| "main".to_string())
    };

    // Pass 1: which stem does each class live in? Needed before pass 2
    // so a subclass's own `.h` can `#include` a same-run superclass's
    // `.h` when that superclass isn't the root (whose full struct is
    // already in the shared companion header) -- `struct {super} base;`
    // is a nested, not pointer, field, so it needs the superclass's
    // *full* struct definition visible, not just a forward declare.
    let mut class_to_stem: HashMap<String, String> = HashMap::new();
    {
        let mut cursor = root.walk();
        for node in root.children(&mut cursor) {
            if node.kind() != "class_interface" && node.kind() != "class_implementation" {
                continue;
            }
            let (name, _, category) = crate::collect::class_header(node, source);
            if category.is_some() {
                continue;
            }
            class_to_stem.entry(name).or_insert_with(|| origin_for(node.start_byte()));
        }
    }

    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut introspection_used = IntrospectionUse::default();
    let mut hoisted_structs: Vec<(String, String)> = Vec::new();
    let mut hoisted_enums: Vec<String> = Vec::new();
    let mut hoisted_forward_decls: Vec<String> = Vec::new();
    let mut hoisted_c_structs: Vec<String> = Vec::new();

    let mut stem_order: Vec<String> = Vec::new();
    let mut headers: HashMap<String, Vec<String>> = HashMap::new();
    let mut bodies: HashMap<String, Vec<String>> = HashMap::new();
    let mut extra_includes: HashMap<String, HashSet<String>> = HashMap::new();
    let mut hoisted_blocks_by_stem: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut hoisted_strings_by_stem: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut hoisted_statics_by_stem: HashMap<String, Vec<(String, String)>> = HashMap::new();

    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        let stem = origin_for(node.start_byte());
        note_stem(&mut stem_order, &stem);
        match node.kind() {
            "compatibility_alias_declaration" => {
                let mut c = node.walk();
                let names: Vec<&str> = node
                    .children(&mut c)
                    .filter(|c| c.kind() == "identifier")
                    .map(|c| node_text(c, source))
                    .collect();
                bodies.entry(stem.clone()).or_default().push(format!(
                    "/* @compatibility_alias {} -- not needed, oz_static resolves classes by their own name only */",
                    names.join(" ")
                ));
            }
            "protocol_declaration" => {
                let (name, _, _) = crate::collect::class_header(node, source);
                bodies.entry(stem.clone()).or_default().push(format!(
                    "/* @protocol {} -- compile-time only, see oz_static_dispatch.h/.c */",
                    name
                ));
            }
            "class_interface" => {
                let (name, _, category) = crate::collect::class_header(node, source);
                if category.is_some() {
                    let text = render_category_interface(node, source, &name, program);
                    headers.entry(stem.clone()).or_default().push(text);
                    continue;
                }
                if let Some(sup) = &program.classes[&name].superclass {
                    let sup_is_root = program.classes.get(sup).map(|s| s.superclass.is_none()).unwrap_or(false);
                    if !sup_is_root {
                        if let Some(sup_stem) = class_to_stem.get(sup) {
                            if sup_stem != &stem {
                                extra_includes.entry(stem.clone()).or_default().insert(sup_stem.clone());
                            }
                        }
                    }
                }
                let scope = base_scope(&name, program);
                let mut ctx = EmitCtx::new(source, program, name.clone(), scope, pools);
                let (header_part, alloc_free_part) = render_interface(node, &mut ctx, program);
                diags.extend(ctx.diags);
                introspection_used.merge(ctx.introspection_used);
                hoisted_structs.extend(ctx.hoisted_structs);
                headers.entry(stem.clone()).or_default().push(header_part);
                if !alloc_free_part.is_empty() {
                    bodies.entry(stem.clone()).or_default().push(alloc_free_part);
                }
            }
            "class_implementation" => {
                let (name, _, category) = crate::collect::class_header(node, source);
                let is_category_impl = category.is_some();
                let mut ivars_scope = base_scope(&name, program);
        // File-scope statics are visible inside every method too, and an
        // ivar of the same name shadows one, so these go in first.
        for (var, ty) in &file_vars {
            ivars_scope.entry(var.clone()).or_insert_with(|| ty.clone());
        }
                let mut ctx =
                    EmitCtx::new(source, program, name.clone(), ivars_scope.clone(), pools);
                let mut out = String::new();
                out.push_str(&banner_box(&header_text(node, source, &["implementation_definition"]), '-'));
                out.push('\n');
                let mut defined_here: HashSet<(String, bool)> = HashSet::new();
                let mut c2 = node.walk();
                for child in node.children(&mut c2) {
                    if child.kind() != "implementation_definition" {
                        continue;
                    }
                    let mut c3 = child.walk();
                    let found_def = child.children(&mut c3).find(|c| c.kind() == "method_definition");
                    match found_def {
                        Some(method_def) => {
                            let known: HashSet<String> = ctx.program.classes.keys().cloned().collect();
                            let sig = crate::collect::extract_method_sig(method_def, source, &name, &known);
                            defined_here.insert((sig.selector, sig.is_class_method));
                            out.push_str(&render_method_definition(method_def, &mut ctx, &name, &ivars_scope));
                            out.push('\n');
                        }
                        None => {
                            let mut c4 = child.walk();
                            let synth = child.children(&mut c4).find(|c| c.kind() == "property_implementation");
                            if synth.is_some() {
                                out.push_str(&format!(
                                    "/* {} -- synthesized accessor(s) emitted below */\n",
                                    one_line(node_text(child, source))
                                ));
                                continue;
                            }
                            ctx.scope = ivars_scope.clone();
                            out.push_str(&render_stmt_with_comment(child, &mut ctx, ""));
                            out.push('\n');
                        }
                    }
                }
                // A category's properties merge into the class it extends,
                // so every @implementation block for that class sees them
                // -- synthesize the accessors only from the primary one,
                // or each block emits its own definition of the same
                // function.
                if let Some(info) = program.classes.get(&name).filter(|_| !is_category_impl) {
                    for prop in &info.properties {
                        let getter_sel = prop.getter_sel.clone().unwrap_or_else(|| prop.name.clone());
                        if !defined_here.contains(&(getter_sel, false)) {
                            out.push_str(&render_synthesized_accessor(&name, prop, true, program));
                            out.push('\n');
                        }
                        if !prop.is_readonly {
                            let setter_sel = prop
                                .setter_sel
                                .clone()
                                .unwrap_or_else(|| crate::collect::default_setter_sel(&prop.name));
                            if !defined_here.contains(&(setter_sel, false)) {
                                out.push_str(&render_synthesized_accessor(&name, prop, false, program));
                                out.push('\n');
                            }
                        }
                    }
                }
                out.push_str(&banner_rule(&format!("end implementation: {}", name), '-'));
                diags.extend(ctx.diags);
                introspection_used.merge(ctx.introspection_used);
                hoisted_structs.extend(ctx.hoisted_structs);
                hoisted_blocks_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_blocks);
                hoisted_strings_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_string_literals);
                hoisted_statics_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_statics);
                bodies.entry(stem.clone()).or_default().push(out);
            }
            "enum_specifier" => {
                let mut c = node.walk();
                let has_body = node.children(&mut c).any(|ch| ch.kind() == "enumerator_list");
                if has_body {
                    hoisted_enums.push(node_text(node, source).to_string());
                    headers.entry(stem.clone()).or_default().push(
                        "/* enum hoisted to the companion header -- needed there before any method prototype references it by value */".to_string(),
                    );
                }
            }
            "struct_specifier" | "union_specifier" => {
                // See the matching arm in `emit()` for why this hoists
                // to the shared companion header rather than staying in
                // this origin's own `.h`: the header a real method
                // prototype needing it actually lands in is
                // `oz_static_dispatch.h`, unconditionally, regardless of
                // which origin's source text this forward-declare itself
                // came from.
                let mut c = node.walk();
                let has_body = node.children(&mut c).any(|ch| ch.kind() == "field_declaration_list");
                if !has_body {
                    hoisted_forward_decls.push(node_text(node, source).to_string());
                    headers.entry(stem.clone()).or_default().push(
                        "/* forward-declared struct hoisted to the companion header -- needed there before any method prototype references it by pointer */".to_string(),
                    );
                } else {
                    // A full `struct Tag { ... };` definition written in
                    // plain C in one of the spliced sources. Output is built
                    // only from what each arm pushes, so until this arm
                    // existed such a definition was dropped outright --
                    // `samples/hello_category`'s `struct color` came out as
                    // nothing but its trailing `;`, and every use of it
                    // failed with "variable has incomplete type 'struct
                    // color'". `emit()` had concealed that by patching the
                    // original text, where anything no arm claimed survived
                    // untouched; since #254 it shares this walk and so
                    // shares this arm.
                    //
                    // It goes to the companion header rather than this
                    // origin's own `.h` because that is the header every
                    // generated file includes, and the type is needed in
                    // more than one of them: the companion's own prototypes
                    // name it (`struct color* Car_color(struct Car *)`),
                    // and another origin's code can build a value of it
                    // (that sample's `main` writes
                    // `&(struct color){255, 255, 0}`).
                    //
                    // Unions share this arm and this one list, so that
                    // source order survives: a struct may have a union
                    // field by value, or the reverse, and the source had
                    // to declare them in a working order already.
                    hoisted_c_structs.push(node_text(node, source).to_string());
                    headers.entry(stem.clone()).or_default().push(format!(
                        "/* {} definition hoisted to the companion header -- named by generated prototypes there, and by other origins' code */",
                        if node.kind() == "union_specifier" { "union" } else { "struct" }
                    ));
                }
            }
            "function_definition" => {
                // No self or ivars here, but a file-scope object variable is
                // in scope for a top-level function just as much as for a
                // method -- `samples/gpio_demo`'s `[led toggle]` sits in
                // `main()`.
                let mut ctx =
                    EmitCtx::new(source, program, String::new(), file_vars.clone(), pools);
                let mut sig_edits = class_tag_edits(node, source, program);
                // A block-typed parameter is lowered to a function pointer
                // here for the same reason its class names are tagged here:
                // this signature is patched text, not rebuilt through
                // `collect::render_type`, so nothing else lowers it and the
                // `^` reached GCC (#272). A method's equivalent parameter
                // has always been lowered.
                sig_edits.extend(block_pointer_edits(node, source));
                let mut text = apply_edits(source, node.start_byte(), node.end_byte(), &sig_edits);
                let mut c2 = node.walk();
                if let Some(body) = node.children(&mut c2).find(|c| c.kind() == "compound_statement") {
                    // The signature is tagged either way; the body is
                    // rendered by the ordinary machinery, which already
                    // resolves types properly.
                    let prefix =
                        apply_edits(source, node.start_byte(), body.start_byte(), &sig_edits);
                    if needs_translation(body) {
                        // Same scan as the single-file arm above; both
                        // `function_definition` paths need it, and an earlier
                        // shape of this change had it in only one.
                        let reject_diags =
                            crate::staticbar::check_function_body(body, source, program);
                        if !reject_diags.is_empty() {
                            ctx.diags.extend(reject_diags);
                            text = format!("{}{}", prefix, node_text(body, source));
                        } else {
                            // Parameters first, so a body declaration of the
                            // same name shadows the parameter rather than the
                            // other way round.
                            collect_function_params(node, &mut ctx);
                            collect_local_decls(body, &mut ctx);
                            let rendered_body = render_body_with_comments(body, &mut ctx);
                            text = format!("{}{}", prefix, rendered_body);
                        }
                    } else {
                        text = format!("{}{}", prefix, node_text(body, source));
                    }
                }
                diags.extend(ctx.diags);
                introspection_used.merge(ctx.introspection_used);
                hoisted_structs.extend(ctx.hoisted_structs);
                hoisted_blocks_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_blocks);
                hoisted_strings_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_string_literals);
                hoisted_statics_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_statics);
                // A `static inline` helper goes to this origin's own
                // header, for the same reason the passthrough arm below
                // already puts macros there: in the single-file design any
                // top-level definition was visible to everything after it
                // merely by sitting in the same text, and once split into
                // real files only that origin's `.h` gives it the same
                // reach. `tests/behavior/cases/regression/
                // issue_090_header_preservation.m` is the case -- its
                // header's `static inline int sensor_scale(int, int)` has
                // to be callable from outside the file it was written in,
                // which is the whole point of the test.
                //
                // `static inline` and nothing else: it is the one form
                // meant to be duplicated per translation unit. A plain
                // `static` function copied into a header would draw
                // "defined but not used" in every file that includes it
                // and break outright if it touched a file-scope static
                // that stayed behind in the body, and a non-static one
                // would be a duplicate symbol at link time.
                if from_header(node.start_byte()) || is_static_inline(node, source) {
                    headers.entry(stem.clone()).or_default().push(text);
                } else {
                    bodies.entry(stem.clone()).or_default().push(text);
                }
            }
            _ => {
                // Passthrough top-level trivia: a stray `#include`,
                // comment, or macro (`preproc_def`/`preproc_ifdef`/...)
                // -- keep it, attached to whichever file its own text
                // physically sits in. Macros specifically (e.g.
                // `OZObject.h`'s own `#define nil ((id)0)`) must land in
                // the *header* bucket, not the body: in the single-file
                // design any top-level `#define` was implicitly visible
                // to every other file (one translation unit) merely by
                // appearing earlier in the same text: split into real
                // per-origin files, only that origin's own `.h` -- which
                // every other file `#include`s when it needs that
                // origin's class -- can still give it the same reach.
                // A plain top-level declaration still needs its class
                // names tagged -- `static OZHeap *sHeap;` is not valid C
                // (see `class_tag_edits`).
                //
                // Two more edits apply to anything that lands here, both
                // #272 and both about blocks reaching C with their `^`
                // intact: a block-pointer declarator is lowered to a
                // function pointer (`block_pointer_edits` -- a file-scope
                // `static void (^g)(int);` is the shape), and a block
                // literal is hoisted to a named function and replaced by
                // that name (`top_level_block_edits` -- which is what makes
                // `ZBUS_LISTENER_DEFINE(n, ^(...){ ... })` compile).
                //
                // Everything else here is trivia and passes through
                // untouched: with no edits, `apply_edits` returns the
                // original text byte for byte.
                // A `#define` body is one opaque `preproc_arg` token, so no
                // arm above descends into it and no edit below can reach it:
                // Objective-C written there is emitted verbatim and fails in
                // the C compiler, naming generated code the user never wrote.
                // Rejected here rather than transpiled -- see
                // `staticbar::check_macro_body` (#238).
                if node.kind() == "preproc_function_def" || node.kind() == "preproc_def" {
                    diags.extend(crate::staticbar::check_macro_body(node, source));
                }

                let mut edits = if node.kind() == "declaration" {
                    class_tag_edits(node, source, program)
                } else {
                    Vec::new()
                };
                // The `;` `parse::repair_bare_macro_statements` wrote over a
                // whitespace byte was for tree-sitter's benefit only. Put
                // the space back, or a macro that terminates its own
                // expansion -- `ZBUS_OBS_DECLARE` -- gets a second `;` and
                // a stray empty declaration at file scope (#288).
                edits.extend(
                    repaired_semicolons
                        .iter()
                        .filter(|o| (node.start_byte()..node.end_byte()).contains(o))
                        .map(|o| (*o..*o + 1, " ".to_string())),
                );
                edits.extend(block_pointer_edits(node, source));
                if contains_block_literal(node) {
                    let mut ctx =
                        EmitCtx::new(source, program, String::new(), file_vars.clone(), pools);
                    edits.extend(top_level_block_edits(node, &mut ctx, program));
                    diags.extend(ctx.diags);
                    introspection_used.merge(ctx.introspection_used);
                    hoisted_structs.extend(ctx.hoisted_structs);
                    hoisted_blocks_by_stem
                        .entry(stem.clone())
                        .or_default()
                        .extend(ctx.hoisted_blocks);
                    hoisted_strings_by_stem
                        .entry(stem.clone())
                        .or_default()
                        .extend(ctx.hoisted_string_literals);
                    hoisted_statics_by_stem
                        .entry(stem.clone())
                        .or_default()
                        .extend(ctx.hoisted_statics);
                }
                let owned_text =
                    apply_edits(source, node.start_byte(), node.end_byte(), &edits);
                let text = owned_text.trim();
                if text.is_empty() {
                    continue;
                }
                // A lone `;` is dropped rather than copied. Several arms
                // above consume a specifier node whose grammar span stops
                // short of the trailing semicolon -- `@compatibility_alias
                // NSObject OZObject;` in `include/oz_sdk/Foundation/
                // OZObject.h` is the one that reaches every generated
                // program -- so the semicolon arrives here as a top-level
                // node of its own, and passing it through left a bare `;`
                // at file scope in 51 of the samples' generated files and
                // 146 of the corpus's.
                //
                // An empty declaration at file scope is not valid ISO C. For
                // the life of this backend it failed no build, because
                // diagnosing it needs `-Wpedantic`, which Zephyr does not
                // pass and neither did the `-Wall -Wextra` sweep behind
                // gap S -- it was found by diffing generated bytes. Since
                // #266 it *does* fail a build: `corpus_parity.rs` compiles
                // every case with `-std=c17 -pedantic-errors`.
                //
                // Handled here rather than in each arm that leaves one: this
                // is the one place every unclaimed node passes through, so a
                // new arm gets the same treatment without knowing to ask for
                // it, and nothing meaningful is lost -- a top-level `;`
                // carries no information in any C dialect.
                if text == ";" {
                    continue;
                }
                // Provenance first: anything a *header* contributed belongs in
                // the generated header, because that is what a header is for
                // -- every file including it should see it. A bare top-level
                // macro invocation is the shape that forced this
                // (`ZBUS_CHAN_DECLARE` in `samples/zbus_service`'s header,
                // which is neither a `preproc` node nor a declaration, so it
                // fell to the body and no other origin could see it).
                //
                // The `preproc` test stays as a fallback for a macro defined
                // in an implementation file, which the single-file design
                // made implicitly visible to everything after it.
                if from_header(node.start_byte()) || node.kind().starts_with("preproc") {
                    headers.entry(stem.clone()).or_default().push(text.to_string());
                } else {
                    bodies.entry(stem.clone()).or_default().push(text.to_string());
                }
            }
        }
    }

    TopLevel {
        stem_order,
        headers,
        bodies,
        extra_includes,
        hoisted_blocks_by_stem,
        hoisted_strings_by_stem,
        hoisted_statics_by_stem,
        hoisted_structs,
        hoisted_enums,
        hoisted_forward_decls,
        hoisted_c_structs,
        class_to_stem,
        introspection_used,
        diags,
    }
}

/// Origin-aware assembler (OZ-096): one `.h`/`.c` pair per origin file,
/// which is what the CLI -- and therefore every real build -- emits.
///
/// The walk is shared with `emit()`; everything here is placement. What
/// makes the two differ at all is that a split program has real
/// translation-unit boundaries, so anything one origin declares and
/// another uses needs an explicit `#include` where the single-file design
/// got the same reach from textual order alone.
pub fn emit_split(
    source: &str,
    program: &Program,
    origins: &[(String, Range<usize>)],
    pools: &crate::pools::PoolSizes,
    header_ranges: &[Range<usize>],
    repaired_semicolons: &[usize],
) -> EmitSplitOutput {
    let TopLevel {
        stem_order,
        headers,
        bodies,
        extra_includes,
        hoisted_blocks_by_stem,
        hoisted_strings_by_stem,
        hoisted_statics_by_stem,
        hoisted_structs,
        hoisted_enums,
        hoisted_forward_decls,
        hoisted_c_structs,
        class_to_stem,
        introspection_used,
        diags,
    } = walk_top_level(source, program, pools, origins, header_ranges, repaired_semicolons);

    // The root class's own header may carry file-scope macros (e.g.
    // `OZObject.h`'s `#define nil ((id)0)`) that every class implicitly
    // saw in the old single-file design, just by textual order -- once
    // split into real files, only an explicit `#include` still gives
    // every other origin the same reach, regardless of whether it
    // actually subclasses anything (plain top-level code, like `main`'s
    // own `main()`, can use `nil` directly too).
    // Same reasoning for OZArray's/OZDictionary's boxed-literal helper
    // (`OZArray_oz_initWithItems`/`OZDictionary_oz_initWithKeysValues`):
    // its prototype lives only in that one class's own `.h` (see
    // `extra_proto` above), not the shared companion header -- but a
    // `@[...]`/`@{...}` literal can appear in *any* file's plain
    // top-level code (e.g. `main()`), not just inside another class's
    // method body, so there's no single "subclass of" edge to hang the
    // dependency on the way there is for a nested struct field.
    //
    // OZString is in the list for the same reason, one step further: a
    // `@"..."` literal emits a *definition* of a `struct OZString` into
    // whichever file used it (see `render_boxed_string_literal`), and
    // defining a variable needs the complete type, not just a
    // declaration. Without this the file gets `error: variable has
    // incomplete type 'struct OZString'` -- which is exactly what five of
    // the cases under tests/behavior/cases/ hit.
    // Carried as (class, stem) rather than just the stem, because whether
    // an edge is safe depends on where the *class* sits in the hierarchy
    // -- see the ancestry check below.
    let mut always_visible: Vec<(String, String)> = Vec::new();
    if let Some(root) = program.root_class() {
        if let Some(stem) = class_to_stem.get(root) {
            always_visible.push((root.to_string(), stem.clone()));
        }
    }
    for helper_class in ["OZArray", "OZDictionary", "OZString"] {
        if let Some(stem) = class_to_stem.get(helper_class) {
            always_visible.push((helper_class.to_string(), stem.clone()));
        }
    }
    // These go into each `.c`, never into a `.h`. They exist so *code* can
    // reach the root class's macros and the boxed-literal helpers, and code
    // lives in the body file. Putting them in headers caused two distinct
    // failures: `main.h` declares nothing, so an earlier attempt to skip
    // declaration-free headers left `main.c` unable to see
    // `OZArray_oz_initWithItems`; and the generated `assert.h` (a shim whose
    // only purpose is keeping `oz_assert` calls in Clang's AST) sits on the
    // include path where the PAL's own `#include <assert.h>` finds it, so
    // pulling the class graph in there re-entered the class headers from
    // inside the companion header, before the root struct existed. A body
    // file is reached by neither path.
    let mut body_includes: HashMap<String, std::collections::BTreeSet<String>> = HashMap::new();
    for (class, target_stem) in &always_visible {
        for stem in &stem_order {
            if stem == target_stem {
                continue;
            }
            // Never point a stem at a *descendant* of a class it owns. A
            // subclass's struct embeds its superclass's by value, so the
            // subclass header must include the superclass header -- and
            // the reverse edge closes a cycle that `#pragma once` then
            // breaks by leaving one of the two structs incomplete
            // (`field has incomplete type 'struct OZObject'`), depending
            // only on which header the compiler happened to enter first.
            // Every class here is a descendant of the root, so without
            // this the root's own header would include all of them.
            let owns_ancestor = program.class_order.iter().any(|owned| {
                class_to_stem.get(owned).is_some_and(|s| s == stem)
                    && program.is_descendant_of(class, owned)
            });
            if owns_ancestor {
                continue;
            }
            body_includes.entry(stem.clone()).or_default().insert(target_stem.clone());
        }
    }

    // A stem that names a class living in another stem needs that stem's
    // header, or the class's struct is incomplete wherever it is used.
    // `samples/hello_category` splits `Car` (its own header, its own
    // origin) from the `main` that does `myCar->_plate = 0xAABBCC`, and
    // without this edge that line is "incomplete definition of type
    // 'struct Car'" -- the companion header carries every class's method
    // prototypes, but only a forward declaration of any non-root struct.
    //
    // Textual mention of the class name is the test. It over-approximates
    // (a comment or an unrelated identifier of the same name counts), but
    // an unnecessary `#include` of a `#pragma once` header costs nothing,
    // while a missing one is a compile error -- so erring towards including
    // is the safe direction. These are body includes, so no header cycle
    // can come of it.
    let mut stem_text: HashMap<&str, String> = HashMap::new();
    for (stem, range) in origins {
        stem_text.entry(stem.as_str()).or_default().push_str(&source[range.clone()]);
    }
    for stem in &stem_order {
        let Some(text) = stem_text.get(stem.as_str()) else {
            continue;
        };
        for (class, owner_stem) in &class_to_stem {
            if owner_stem == stem {
                continue;
            }
            if mentions_identifier(text, class) {
                body_includes.entry(stem.clone()).or_default().insert(owner_stem.clone());
            }
        }
    }

    let mut files = Vec::with_capacity(stem_order.len());
    for stem in &stem_order {
        let mut h = String::from(
            "/* Auto-generated by oz_static -- do not edit */\n#pragma once\n#include \"oz_static_dispatch.h\"\n",
        );
        if let Some(deps) = extra_includes.get(stem) {
            let mut deps: Vec<&String> = deps.iter().collect();
            deps.sort();
            for dep in deps {
                h.push_str(&format!("#include \"{}.h\"\n", dep));
            }
        }
        h.push('\n');
        if let Some(sections) = headers.get(stem) {
            h.push_str(&sections.join("\n"));
            h.push('\n');
        }

        let mut c = format!(
            "/* Auto-generated by oz_static -- do not edit */\n#include \"oz_static_dispatch.h\"\n#include \"{}.h\"\n",
            stem
        );
        if let Some(deps) = body_includes.get(stem) {
            for dep in deps {
                c.push_str(&format!("#include \"{}.h\"\n", dep));
            }
        }
        c.push('\n');
        if let Some(statics) = hoisted_statics_by_stem.get(stem) {
            if !statics.is_empty() {
                c.push_str("/* __block-qualified locals, promoted to file scope */\n");
                for (_, decl) in statics {
                    c.push_str(decl);
                    c.push('\n');
                }
                c.push('\n');
            }
        }
        if let Some(blocks) = hoisted_blocks_by_stem.get(stem) {
            if !blocks.is_empty() {
                c.push_str("/* non-capturing blocks, hoisted out of their enclosing methods -- prototypes (defined below) */\n");
                for (prototype, _) in blocks {
                    c.push_str(prototype);
                }
                c.push('\n');
            }
        }
        if let Some(strs) = hoisted_strings_by_stem.get(stem) {
            if !strs.is_empty() {
                c.push_str("/* boxed string literals, hoisted -- extern forward declarations (defined below) */\n");
                for (prototype, _) in strs {
                    c.push_str(prototype);
                }
                c.push('\n');
            }
        }
        if let Some(sections) = bodies.get(stem) {
            c.push_str(&sections.join("\n\n"));
            c.push('\n');
        }
        if let Some(blocks) = hoisted_blocks_by_stem.get(stem) {
            if !blocks.is_empty() {
                c.push_str("\n/* non-capturing blocks, hoisted out of their enclosing methods */\n");
                for (_, definition) in blocks {
                    c.push_str(definition);
                    c.push('\n');
                }
            }
        }
        if let Some(strs) = hoisted_strings_by_stem.get(stem) {
            if !strs.is_empty() {
                c.push_str("\n/* boxed string literals, hoisted -- static struct OZString instances */\n");
                for (_, definition) in strs {
                    c.push_str(definition);
                }
            }
        }

        files.push((stem.clone(), h, c));
    }

    let (companion_h, companion_c) =
        crate::companion::render(
            program,
            &hoisted_structs,
            &hoisted_enums,
            &hoisted_forward_decls,
            &hoisted_c_structs,
            pools,
            &crate::imports::collect_system_includes(source),
            &introspection_used,
        );

    EmitSplitOutput { files, companion_h, companion_c, diagnostics: diags }
}

/// Byte-range edits that give every bare class name in `node` its `struct`
/// tag, as absolute offsets into the source.
///
/// A class generates `struct Name`, never a typedef, so any type position
/// that keeps the ObjC spelling is invalid C: `error: must use 'struct' tag
/// to refer to type 'Sensor'`. Method signatures, ivars, locals and casts
/// all route through `collect::render_type` already; the two positions that
/// did not were a plain top-level declaration (`samples/heap_alloc`'s
/// `static OZHeap *sHeap;`) and a free function's own signature
/// (`samples/arc_demo`'s `static Sensor *createSensor(int v)`), because both
/// were copied through verbatim.
///
/// A name already under a `struct_specifier` is skipped, so an
/// already-tagged `struct OZHeap *` is left alone rather than becoming
/// `struct struct OZHeap *`.
fn class_tag_edits(node: Node, src: &str, program: &Program) -> Vec<(Range<usize>, String)> {
    fn walk(
        node: Node,
        src: &str,
        program: &Program,
        out: &mut Vec<(Range<usize>, String)>,
    ) {
        // Inside a struct_specifier the tag is already present, and a
        // generic_specifier's arguments are erased rather than tagged.
        if matches!(node.kind(), "struct_specifier" | "generic_specifier") {
            return;
        }
        if node.kind() == "type_identifier" {
            let name = &src[node.byte_range()];
            if program.is_class(name) {
                out.push((node.byte_range(), format!("struct {}", name)));
                return;
            }
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, src, program, out);
        }
    }
    let mut out = Vec::new();
    walk(node, src, program, &mut out);
    out
}

/// Byte-range edits that lower every block-pointer declarator in `node` to a
/// plain function pointer -- `void (^cb)(int)` becomes `void (*cb)(int)` --
/// as absolute offsets into the source.
///
/// A block *is* a function pointer in generated C, and every position that
/// routes through `collect::render_type` already says so: an ivar becomes
/// `void (*_ivarBlk)(int)`, a method parameter becomes
/// `void (*b)(struct k_timer *)`, a local becomes `void (*local)(int)`. The
/// three that did not are the ones assembled by patching the original text,
/// where no edit lowered a block type (#272):
///
///   - a free function's signature, both its prototype and its definition
///     (`static void take_cb(void (^cb)(int));`)
///   - a file-scope block variable (`static void (^g_blk)(int);`)
///
/// Both reached the C compiler with the `^` intact. Blocks are a Clang
/// extension rather than ISO C, so this is not a weaker type but text no GCC
/// target can parse at all: `error: expected ')' before '^' token`.
///
/// Nothing in the repository writes either shape, which is why they went
/// unnoticed -- the same reason gaps Q, V and R went unnoticed, and the same
/// family: the top-level path getting a reduced version of what a method
/// body gets.
///
/// A `block_literal` subtree is skipped, because `render_block` synthesizes
/// that function's signature outright rather than patching it, and
/// `top_level_block_edits` replaces the whole literal anyway -- an edit
/// inside it would be discarded or would collide.
fn block_pointer_edits(node: Node, src: &str) -> Vec<(Range<usize>, String)> {
    fn caret(node: Node, src: &str) -> Option<Range<usize>> {
        let mut cursor = node.walk();
        if let Some(tok) = node.children(&mut cursor).find(|c| c.kind() == "^") {
            return Some(tok.byte_range());
        }
        // The grammar names the token, but fall back to the text rather than
        // silently emitting nothing: leaving a `^` behind does not degrade
        // the output, it makes it uncompilable.
        let start = node.start_byte();
        src[node.byte_range()].find('^').map(|off| (start + off)..(start + off + 1))
    }
    fn walk(node: Node, src: &str, out: &mut Vec<(Range<usize>, String)>) {
        if node.kind() == "block_literal" {
            return;
        }
        if matches!(node.kind(), "block_pointer_declarator" | "abstract_block_pointer_declarator")
        {
            if let Some(range) = caret(node, src) {
                out.push((range, "*".to_string()));
            }
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, src, out);
        }
    }
    let mut out = Vec::new();
    walk(node, src, &mut out);
    out
}

/// Hoist every `block_literal` under a top-level node that no other arm
/// claimed, returning the edits that replace each literal with the name of
/// the function `render_block` synthesized for it.
///
/// Together with `OZM` -- whose two halves are pure preprocessor, in
/// `oz_sdk/Foundation/OZMacro.h` and `platform/oz_platform.h` -- this is
/// what makes a target definition macro writable with an inline block
/// (#272). The hoisting is the transpiler's whole contribution: it turns
/// the literal into a function name, and the preprocessor does the rest.
///
///
/// ```objc
/// OZM(ZBUS_LISTENER_DEFINE, lis_print_temp, ^(const struct zbus_channel *chan) {
///         ...
/// });
/// ```
///
/// The same literal inside a method or free-function *body* has always
/// hoisted -- `walk_top_level`'s passthrough arm copies text, so the literal
/// was simply never reached by `render_block` and arrived at GCC with its
/// `^`. Handled here, at the one place every unclaimed node passes through,
/// rather than per node kind: that is what gap X's bare-`;` fix chose and
/// for the same reason, since it means a future arm gets the same treatment
/// without knowing to ask, and oz_static needs to know no macro's name --
/// `ZBUS_LISTENER_DEFINE`, `K_TIMER_DEFINE` and any other shape are all just
/// unclaimed text with a literal in it.
///
/// The static bar is run over each body, which this position had no scan of
/// at all -- the top-level twin of the free-function scan gap Q added. A
/// rejected block is left exactly as written, so the diagnostic is what the
/// user sees rather than generated code they never wrote.
///
/// A literal is not descended into: `render_block` renders that whole
/// subtree, nested literals included.
fn top_level_block_edits(
    node: Node,
    ctx: &mut EmitCtx,
    program: &Program,
) -> Vec<(Range<usize>, String)> {
    fn walk(
        node: Node,
        ctx: &mut EmitCtx,
        program: &Program,
        out: &mut Vec<(Range<usize>, String)>,
    ) {
        if node.kind() == "block_literal" {
            let mut cursor = node.walk();
            let body = node.children(&mut cursor).find(|c| c.kind() == "compound_statement");
            if let Some(body) = body {
                let reject = crate::staticbar::check_function_body(body, ctx.src, program);
                if !reject.is_empty() {
                    ctx.diags.extend(reject);
                    return;
                }
            }
            let (name, _) = render_block(node, ctx);
            out.push((node.byte_range(), name));
            return;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, ctx, program, out);
        }
    }
    let mut out = Vec::new();
    walk(node, ctx, program, &mut out);
    out
}

/// Does `node` contain a `block_literal` anywhere?
///
/// Cheap guard so the passthrough arm builds an `EmitCtx` only for the nodes
/// that need one, rather than for every comment and `#include`.
fn contains_block_literal(node: Node) -> bool {
    if node.kind() == "block_literal" {
        return true;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().any(contains_block_literal)
}

/// Apply `edits` (absolute source offsets) to the text of `start..end`.
fn apply_edits(src: &str, start: usize, end: usize, edits: &[(Range<usize>, String)]) -> String {
    let mut text = src[start..end].to_string();
    let mut relevant: Vec<&(Range<usize>, String)> =
        edits.iter().filter(|(r, _)| r.start >= start && r.end <= end).collect();
    // Back to front, so earlier offsets stay valid.
    relevant.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (range, replacement) in relevant {
        text.replace_range(range.start - start..range.end - start, replacement);
    }
    text
}

/// Is `node` a `static inline` function definition?
///
/// Read off the text ahead of the declarator rather than the child nodes,
/// because the two keywords can appear in either order and with any
/// qualifiers or attributes between them.
fn is_static_inline(node: Node, source: &str) -> bool {
    let text = node_text(node, source);
    let prefix = match text.find('(') {
        Some(paren) => &text[..paren],
        None => text,
    };
    let has = |word: &str| {
        prefix.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').any(|t| t == word)
    };
    has("static") && (has("inline") || has("__inline") || has("__inline__"))
}

/// Does `name` appear in `text` as a whole identifier?
///
/// A substring test would match `Car` inside `Carriage`, and a real parse
/// is more than this needs: the caller only wants to know whether a file
/// might refer to a class, and answering "yes" too often merely adds an
/// `#include` that a `#pragma once` header makes free.
fn mentions_identifier(text: &str, name: &str) -> bool {
    let is_ident_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let (haystack, needle) = (text.as_bytes(), name.as_bytes());
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(offset) = text[from..].find(name) {
        let start = from + offset;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_ident_byte(haystack[start - 1]);
        let after_ok = end == haystack.len() || !is_ident_byte(haystack[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// First child of `node` with the given kind.
fn child_by_kind_local<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    children.into_iter().find(|c| c.kind() == kind)
}

fn base_scope(class_name: &str, program: &Program) -> HashMap<String, String> {
    program.all_ivars(class_name).into_iter().collect()
}

/// File-scope object variables, as `name -> C type`.
///
/// A `static Widget *g_widget;` at translation-unit scope is visible to every
/// method body and to plain top-level functions, but nothing collected it, so
/// a send to it reported the receiver type as `id` and was rejected:
/// "cannot statically resolve the receiver type for selector 'toggle'".
/// `samples/gpio_demo` (`static GPIOOutput *led;`) and `samples/heap_alloc`
/// (`static OZHeap *sHeap;`) are both that shape, and the oracle collects
/// file-scope statics for the same reason (`collect.py`).
///
/// Only declarations at the top level are considered; anything nested is a
/// local and is already handled by `collect_local_decls`.
fn file_scope_vars(root: Node, ctx_src: &str, program: &Program) -> HashMap<String, String> {
    let known: HashSet<String> = program.classes.keys().cloned().collect();
    let mut out = HashMap::new();
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    for child in children {
        if child.kind() != "declaration" {
            continue;
        }
        let (type_text, stars) = crate::collect::extract_type_and_stars(child, ctx_src);
        // A class can be written either way at file scope, and both spellings
        // mean the same thing: `static Widget *g;` gives a `type_identifier`,
        // so `type_text` is `Widget`, while `static struct Widget *g;` goes
        // through `extract_type_and_stars`'s `struct_specifier` arm and gives
        // `struct Widget` -- which is not a key in `known`, so the tagged form
        // was silently skipped and a send to it reported an `id` receiver
        // (#251). `collect_local_decls` has no such gate, which is why the
        // identical *local* resolved and only file scope was affected: the two
        // disagreeing about what counts as an object declaration is the same
        // asymmetry gap R and #246 both came down to.
        //
        // The bare name is what `render_type` wants, since it re-adds the tag.
        let class_name = type_text.strip_prefix("struct ").unwrap_or(&type_text);
        if stars == 0 || !known.contains(class_name) {
            continue;
        }
        let c_type = crate::collect::render_type(class_name, stars, &known);
        let mut c2 = child.walk();
        let declarators: Vec<Node> = child.children(&mut c2).collect();
        for declarator in declarators {
            if !matches!(declarator.kind(), "init_declarator" | "identifier" | "pointer_declarator")
            {
                continue;
            }
            let name = crate::collect::find_declared_name(declarator, ctx_src);
            if !name.is_empty() {
                out.insert(name, c_type.clone());
            }
        }
    }
    out
}
