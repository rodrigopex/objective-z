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
/// `/* ... */`, so any embedded `*/` in the original text (a real inline
/// comment, or even a string/char literal containing those two
/// characters) is neutralized to `* /` here too -- C block comments
/// don't nest, so left as-is it would close the wrapping comment early.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace("*/", "* /")
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
/// parsed as live C. Every `*/` inside `content` is neutralized to `* /`
/// before wrapping, the standard escape for exactly this case; cosmetic
/// only, since this text is documentation either way.
fn banner_box(content: &str, fill: char) -> String {
    let content = content.trim_end().replace("*/", "* /");
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
fn collect_local_decls(node: Node, ctx: &mut EmitCtx) {
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
            if child.kind() == "init_declarator" || child.kind() == "identifier" {
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
        collect_local_decls(child, ctx);
    }
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
        if child.kind() != "init_declarator" && child.kind() != "identifier" {
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
pub(crate) fn render_param(ptype: &str, pname: &str) -> String {
    if ptype.contains(crate::collect::PARAM_NAME_PLACEHOLDER) {
        ptype.replace(crate::collect::PARAM_NAME_PLACEHOLDER, pname)
    } else {
        format!("{} {}", ptype, pname)
    }
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
}

impl<'a> EmitCtx<'a> {
    fn err(&mut self, node: Node, message: impl Into<String>) {
        let (line, col) = line_col(self.src, node.start_byte());
        self.diags.push(Diagnostic::new(message, line, col));
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
        "for_statement" if is_forin_shape(node) => render_forin_statement(node, ctx),
        "synchronized_statement" => render_synchronized_statement(node, ctx),
        "return_statement" => render_return_statement(node, ctx),
        "compound_statement" if is_autoreleasepool_shape(node) => {
            render_autoreleasepool_statement(node, ctx)
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
fn is_boxed_string_literal(node: Node) -> bool {
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
    // `oz_deallocating = 1` from birth is what makes this literal
    // immortal. It lives in static storage, so `free()`-ing it aborts --
    // and something does try: `companion`'s release path runs
    // `{class}_oz_free` once a refcount hits zero, and a literal's
    // refcount does reach zero, because a collection that absorbed it
    // (`@[ @"a" ]`, or a dictionary key) releases its elements when it is
    // itself deallocated. `oz_static_release` checks this flag before the
    // free switch, so setting it up front makes release a no-op at zero
    // instead of a crash, matching the real `OZString.m`'s own `-dealloc`
    // ("compile-time constant, never freed").
    let definition = format!(
        "struct OZString {} = {{ .base = {{ .oz_class_id = OZ_STATIC_CLASS_OZString, .oz_refcount = 1, .oz_deallocating = 1 }}, ._length = {}, ._hash = 0, ._data = {} }};\n",
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

/// Desugars a boxed array literal (`@[e1, e2, ...]`) into a call to the
/// malloc-based `OZArray_oz_initWithItems` builder (see
/// `companion::render_array_support`) -- the same shape as the Python
/// pipeline's `ObjCArrayLiteral` handling, but backed by a stack buffer
/// instead of the item-pool allocator that pipeline has and this
/// malloc-based spike doesn't.
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
/// call to the malloc-based `OZDictionary_oz_initWithKeysValues` builder
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
    let Some(class) = class_name_from_type(&recv_type) else {
        return pass_through(ctx);
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
    send_to_resolved_class(ctx, &class, selector, &recv_text, &[index_text])
}

/// One instance send whose receiver's class is already resolved, routed by
/// the same rule as `render_message`'s resolved-receiver branch: a direct
/// call when no subclass overrides the selector, the `class_id` switch when
/// one does (see `Program::has_overriding_subclass`).
///
/// Used by desugarings that synthesize a send rather than translating a
/// literal `[recv sel:...]` -- subscripting today. Deliberately does not
/// handle `super` receivers or class methods: neither can arise from a
/// desugaring, and both need care `render_message` already takes.
fn send_to_resolved_class(
    ctx: &mut EmitCtx,
    class: &str,
    selector: &str,
    recv_text: &str,
    arg_texts: &[String],
) -> (String, String) {
    let root = ctx.program.root_class().unwrap_or("OZSRoot").to_string();
    if ctx.program.has_overriding_subclass(class, selector) {
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
    if returns_instancetype && defining != class {
        (format!("(struct {} *)({})", class, call), format!("struct {} *", class))
    } else {
        (call, ret_ty)
    }
}

/// `@synchronized(obj) { body }` lowered to a scoped critical section:
///
/// ```c
/// { /* @synchronized(obj) */
///     oz_spinlock_t _oz_sync_lock_... = {0};
///     oz_spinlock_key_t _oz_sync_key_... = oz_spin_lock(&_oz_sync_lock_...);
///     oz_static_retain((struct OZObject *)(obj));
///     ... body ...
///     oz_static_release((struct OZObject *)(obj));
///     oz_spin_unlock(&_oz_sync_lock_..., _oz_sync_key_...);
/// }
/// ```
///
/// The lock is fresh per block rather than per object, matching the
/// Python pipeline, which allocates a new `OZSpinLock` per block and
/// locks that object's own field (`emit.py::_emit_synchronized_stmt` +
/// `_inject_oz_spinlock`). What this buys on Zephyr is an
/// interrupt-disabled critical section (`k_spin_lock`), not mutual
/// exclusion keyed on `obj`; on host it compiles to nothing (see
/// `platform/oz_platform_{zephyr,host}.h`).
///
/// Locking `obj`'s own root-level `oz_prop_lock` instead would be closer
/// to what the source literally says, and was tried -- but real
/// `@synchronized` is recursive, and a `k_spinlock` is not, so
/// `@synchronized(self) { @synchronized(self) { ... } }` (a shape the
/// oracle's own `tests/behavior/cases/synchronized/nested.m` exercises,
/// with two receivers that may alias) would self-deadlock on hardware
/// while passing on host, where the lock is a no-op. A per-block lock
/// cannot deadlock, so it is the safer half of that trade until there is
/// a recursive lock in the PAL to build on.
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
    let lock = format!("_oz_sync_lock_{}", suffix);
    let key = format!("_oz_sync_key_{}", suffix);

    // Held across the body so the object can't be deallocated mid-section,
    // mirroring the retain/release the oracle's OZSpinLock does in its
    // -initWithObject:/-dealloc pair.
    let retain = format!("oz_static_retain((struct {} *)({}));", root, obj_text);
    let cleanup = format!(
        "oz_static_release((struct {root} *)({obj}));\n\toz_spin_unlock(&{lock}, {key});",
        root = root,
        obj = obj_text,
        lock = lock,
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
             \toz_spinlock_t {lock} = {{0}};\n\
             \toz_spinlock_key_t {key} = oz_spin_lock(&{lock});\n\
             \t{retain}\n\
             \t{body}\n\
             \t{cleanup}\n\
             }}",
            lock = lock,
            key = key,
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
    // Outside any @synchronized, behave exactly as the catch-all in
    // `render_expr` would: byte-identical when nothing needs translating.
    if ctx.sync_cleanups.is_empty() {
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

    let cleanups = ctx.sync_cleanups.iter().rev().cloned().collect::<Vec<_>>().join("\n\t");

    let mut cursor = node.walk();
    let value = node.children(&mut cursor).find(|c| c.kind() != "return" && c.kind() != ";");

    match value {
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

    let rendered_stmts: Vec<(String, &str)> = stmts
        .iter()
        .map(|s| {
            let rendered = render_expr(*s, ctx).0;
            let combined = if ctx.pre_stmts.is_empty() {
                rendered
            } else {
                let pre = ctx.pre_stmts.join("\n\t");
                ctx.pre_stmts.clear();
                format!("{}\n\t{}", pre, rendered)
            };
            (combined, node_text(*s, ctx.src))
        })
        .collect();

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
/// through the `oz_class_id` switch -- used whenever the receiver's
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
        Some(plist) => node_text(plist, ctx.src).to_string(),
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
    let definition = format!(
        "/* block at {}:{} -- synthesized function, hoisted out of its enclosing method */\n{} {}{} {}\n",
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
fn render_body_with_comments(body: Node, ctx: &mut EmitCtx) -> String {
    let mut cursor = body.walk();
    let children: Vec<Node> = body.children(&mut cursor).collect();
    if children.len() < 2 {
        return node_text(body, ctx.src).to_string();
    }
    let stmts = &children[1..children.len() - 1];

    let rendered_stmts: Vec<(String, &str)> = stmts
        .iter()
        .map(|s| {
            let rendered = render_expr(*s, ctx).0;
            let combined = if ctx.pre_stmts.is_empty() {
                rendered
            } else {
                let pre = ctx.pre_stmts.join("\n\t");
                ctx.pre_stmts.clear();
                format!("{}\n\t{}", pre, rendered)
            };
            (combined, node_text(*s, ctx.src))
        })
        .collect();
    if rendered_stmts.iter().all(|(rendered, original)| rendered == original) {
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
    let mut edits: Vec<(Range<usize>, &str)> = Vec::new();
    collect_ivar_lowering_edits(instance_variable, ctx, origin, &mut edits);
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (range, replacement) in edits {
        text.replace_range(range, replacement);
    }
    // A stripped qualifier leaves behind the whitespace it sat in.
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace(" ;", ";")
}

fn collect_ivar_lowering_edits(
    node: Node,
    ctx: &mut EmitCtx,
    origin: usize,
    edits: &mut Vec<(Range<usize>, &'static str)>,
) {
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
                edits.push((node.start_byte() - origin..node.end_byte() - origin, ""));
            }
            return;
        }
        "block_pointer_declarator" => {
            let mut c = node.walk();
            let caret = node.children(&mut c).find(|n| n.kind() == "^").map(|n| n.byte_range());
            if let Some(caret) = caret {
                edits.push((caret.start - origin..caret.end - origin, "*"));
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
            let mut f = String::from(
                "\tuint8_t oz_class_id; /* synthesized: which concrete class this is -- indexes the dealloc dispatch switch */\n\
                 \toz_atomic_t oz_refcount; /* synthesized: retain count */\n\
                 \tuint8_t oz_deallocating; /* synthesized: guards against re-entrant dealloc while it runs */\n",
            );
            // Shared lock for every atomic property in the program --
            // reached from any class via `Program::ivar_access_path`'s
            // ordinary "base." hop-chain, same as any inherited ivar.
            if program.has_atomic_property() {
                f.push_str(
                    "\toz_spinlock_t oz_prop_lock; /* synthesized: guards atomic property access */\n",
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
    for prop in &info.properties {
        if let Some(ivar) = &prop.ivar_name {
            if !raw_ivar_names.contains(ivar) {
                ivars_text.push_str(&format!(
                    "\t{} {}; /* synthesized: backs property '{}' */\n",
                    prop.c_type, ivar, prop.name
                ));
            }
        }
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
            decls.push_str(&render_prototype(&name, &sig));
        }
    }
    for m in &info.methods {
        if !declared.contains(&(m.selector.clone(), m.is_class_method)) {
            decls.push_str(&render_prototype(&name, m));
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
        let (alloc_free, extra_proto) = if name == "OZArray" {
            (
                crate::companion::render_array_support(&name, &root),
                format!("struct {name} *{name}_oz_initWithItems(void **src, unsigned int count);\n", name = name),
            )
        } else if name == "OZDictionary" {
            (
                crate::companion::render_dict_support(&name, &root),
                format!(
                    "struct {name} *{name}_oz_initWithKeysValues(void **keys, void **values, unsigned int count);\n",
                    name = name
                ),
            )
        } else {
            (crate::companion::render_alloc_free(&name, &root), String::new())
        };
        (format!("{}{}\n{}{}{}", open_banner, struct_text, extra_proto, decls, close_banner), alloc_free)
    }
}

pub(crate) fn render_prototype(class_name: &str, m: &crate::model::MethodSig) -> String {
    let mut params = String::new();
    if !m.is_class_method {
        params.push_str(&format!("struct {} *self", class_name));
    }
    for (pname, ptype) in &m.params {
        if !params.is_empty() {
            params.push_str(", ");
        }
        params.push_str(&render_param(ptype, pname));
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
            decls.push_str(&render_prototype(name, &sig));
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
        (sel, "void".to_string(), format!("struct {} *self, {}", class_name, render_param(c_type, &prop.name)))
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
        sig_params.push_str(&render_param(ptype, pname));
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

    let body_text = match body {
        Some(body) => {
            let class_info = ctx.program.classes[class_name].clone();
            let reject_diags = crate::staticbar::check_method_body(
                body, ctx.src, ctx.program, &class_info, &sig.params,
            );
            if !reject_diags.is_empty() {
                ctx.diags.extend(reject_diags);
                node_text(body, ctx.src).to_string()
            } else {
                collect_local_decls(body, ctx);
                render_body_with_comments(body, ctx)
            }
        }
        None => "{\n}".to_string(),
    };

    format!("/* {} */\n{} {}({})\n{}\n", one_line(&header), ret_ty, fn_name, sig_params, body_text)
}

pub struct EmitOutput {
    pub source_c: String,
    pub companion_h: String,
    pub companion_c: String,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn emit(source: &str, program: &Program) -> EmitOutput {
    let tree = crate::parse::parse(source);
    let root = tree.root_node();
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut hoisted_blocks: Vec<(String, String)> = Vec::new();
    let mut hoisted_structs: Vec<(String, String)> = Vec::new();
    let mut hoisted_enums: Vec<String> = Vec::new();
    let mut hoisted_forward_decls: Vec<String> = Vec::new();
    let mut hoisted_string_literals: Vec<(String, String)> = Vec::new();
    let mut hoisted_statics: Vec<(String, String)> = Vec::new();

    struct Patch {
        start: usize,
        end: usize,
        text: String,
    }
    let mut patches: Vec<Patch> = Vec::new();

    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        match node.kind() {
            "compatibility_alias_declaration" => {
                // `@compatibility_alias NSObject OZObject;` (real
                // Foundation headers use this so Clang accepts either
                // name) is, like `@protocol`, never valid C -- elided to
                // a comment rather than left to break compilation. The
                // alias itself needs no C-level equivalent: oz_static
                // resolves class names by their own spelling only, so
                // code would have to say `OZObject` either way.
                let mut cursor = node.walk();
                let names: Vec<&str> = node
                    .children(&mut cursor)
                    .filter(|c| c.kind() == "identifier")
                    .map(|c| node_text(c, source))
                    .collect();
                patches.push(Patch {
                    start: node.start_byte(),
                    end: node.end_byte(),
                    text: format!(
                        "/* @compatibility_alias {} -- not needed, oz_static resolves classes by their own name only */",
                        names.join(" ")
                    ),
                });
            }
            "protocol_declaration" => {
                // A protocol is purely a compile-time contract in this
                // design too, same as in real Objective-C -- there's no C
                // runtime representation of it (see `companion.rs`'s
                // `render_protocol_dispatch`, which dispatches by
                // "who implements this selector," not by protocol
                // identity). Its own declaration text is never valid C,
                // so it must be replaced, not left in place.
                let (name, _, _) = crate::collect::class_header(node, source);
                patches.push(Patch {
                    start: node.start_byte(),
                    end: node.end_byte(),
                    text: format!("/* @protocol {} -- compile-time only, see oz_static_dispatch.h/.c */", name),
                });
            }
            "class_interface" => {
                let (name, _, category) = crate::collect::class_header(node, source);
                if category.is_some() {
                    let text = render_category_interface(node, source, &name, program);
                    patches.push(Patch { start: node.start_byte(), end: node.end_byte(), text });
                    continue;
                }
                let scope = base_scope(&name, program);
                let mut ctx = EmitCtx {
                    src: source,
                    program,
                    class_name: name.clone(),
                    scope,
                    locals: std::collections::HashSet::new(),
                    diags: Vec::new(),
                    hoisted_blocks: Vec::new(),
                    hoisted_structs: Vec::new(),
                    hoisted_string_literals: Vec::new(),
                    hoisted_statics: Vec::new(),
                    block_counter: 0,
                    pre_stmts: Vec::new(),
                    sync_cleanups: Vec::new(),
                    method_return_type: "int".to_string(),
                };
                let (header_part, alloc_free_part) = render_interface(node, &mut ctx, program);
                let text = format!("{}\n{}", header_part, alloc_free_part);
                diags.extend(ctx.diags);
                hoisted_structs.extend(ctx.hoisted_structs);
                patches.push(Patch { start: node.start_byte(), end: node.end_byte(), text });
            }
            "class_implementation" => {
                let (name, _, category) = crate::collect::class_header(node, source);
                let is_category_impl = category.is_some();
                let ivars_scope = base_scope(&name, program);
                let mut ctx = EmitCtx {
                    src: source,
                    program,
                    class_name: name.clone(),
                    scope: ivars_scope.clone(),
                    locals: std::collections::HashSet::new(),
                    diags: Vec::new(),
                    hoisted_blocks: Vec::new(),
                    hoisted_structs: Vec::new(),
                    hoisted_string_literals: Vec::new(),
                    hoisted_statics: Vec::new(),
                    block_counter: 0,
                    pre_stmts: Vec::new(),
                    sync_cleanups: Vec::new(),
                    method_return_type: "int".to_string(),
                };
                let mut out = String::new();
                out.push_str(&banner_box(&header_text(node, source, &["implementation_definition"]), '-'));
                out.push('\n');
                // Selectors with a real hand-written body somewhere in
                // *this* @implementation -- a property's getter/setter
                // is only synthesized below if it isn't one of these,
                // regardless of what collect.rs decided when populating
                // `info.methods` (that bookkeeping is for dispatch
                // classification, not for "does this need a body").
                let mut defined_here: std::collections::HashSet<(String, bool)> =
                    std::collections::HashSet::new();
                let mut c2 = node.walk();
                for child in node.children(&mut c2) {
                    if child.kind() != "implementation_definition" {
                        continue;
                    }
                    let mut c3 = child.walk();
                    let found_def = child.children(&mut c3).find(|c| c.kind() == "method_definition");
                    match found_def {
                        Some(method_def) => {
                            let known: std::collections::HashSet<String> =
                                ctx.program.classes.keys().cloned().collect();
                            let sig = crate::collect::extract_method_sig(method_def, source, &name, &known);
                            defined_here.insert((sig.selector, sig.is_class_method));
                            out.push_str(&render_method_definition(
                                method_def, &mut ctx, &name, &ivars_scope,
                            ));
                            out.push('\n');
                        }
                        None => {
                            let mut c4 = child.walk();
                            let synth =
                                child.children(&mut c4).find(|c| c.kind() == "property_implementation");
                            if synth.is_some() {
                                out.push_str(&format!(
                                    "/* {} -- synthesized accessor(s) emitted below */\n",
                                    one_line(node_text(child, source))
                                ));
                                continue;
                            }
                            // Not a method: e.g. a `static Foo *g;` file-scope
                            // declaration written directly inside
                            // @implementation. Copy through (translating any
                            // message send it happens to contain, with a
                            // before-comment if so) instead of silently
                            // dropping it.
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
                out.push('\n');
                diags.extend(ctx.diags);
                hoisted_blocks.extend(ctx.hoisted_blocks);
                hoisted_structs.extend(ctx.hoisted_structs);
                hoisted_string_literals.extend(ctx.hoisted_string_literals);
                hoisted_statics.extend(ctx.hoisted_statics);
                patches.push(Patch { start: node.start_byte(), end: node.end_byte(), text: out });
            }
            "enum_specifier" => {
                // A top-level named `enum Tag { ... };` definition. Method
                // prototypes in the companion header may reference this
                // type by value (an enum param/return, not just a pointer),
                // which -- unlike a class's struct -- C cannot forward-
                // declare: the full definition must be visible before any
                // such prototype. So this moves to the companion header
                // (ahead of the per-class prototype sections) exactly like
                // the root class's struct does, and is elided in-place here
                // to avoid a duplicate-definition error.
                let mut c = node.walk();
                let has_body = node.children(&mut c).any(|ch| ch.kind() == "enumerator_list");
                if has_body {
                    hoisted_enums.push(node_text(node, source).to_string());
                    patches.push(Patch {
                        start: node.start_byte(),
                        end: node.end_byte(),
                        text: "/* enum hoisted to the companion header -- needed there before any method prototype references it by value */".to_string(),
                    });
                }
            }
            "struct_specifier" => {
                // A top-level `struct Tag;` forward-declaration (no
                // `field_declaration_list` body -- a real, full `struct
                // Tag { ... };` definition, if this spike ever needs to
                // support one, isn't this case). Real Foundation headers
                // use this for a type only ever referenced by pointer in
                // a method signature (e.g. `NSFastEnumerationState` in
                // `countByEnumeratingWithState:`), letting Clang parse
                // the AST without the real type -- but the *generated*
                // method prototype needs that forward declare visible
                // too, and not just wherever this text happened to sit
                // in the original source: the shared companion header
                // (OZ-091) unconditionally declares every class's every
                // method prototype, `NSFastEnumerationState` included,
                // regardless of which file's `source_c` this text landed
                // in. Same fix and same hoist-to-companion-header
                // mechanism as `enum_specifier` just above.
                let mut c = node.walk();
                let has_body = node.children(&mut c).any(|ch| ch.kind() == "field_declaration_list");
                if !has_body {
                    hoisted_forward_decls.push(node_text(node, source).to_string());
                    patches.push(Patch {
                        start: node.start_byte(),
                        end: node.end_byte(),
                        text: "/* forward-declared struct hoisted to the companion header -- needed there before any method prototype references it by pointer */".to_string(),
                    });
                }
            }
            "function_definition" => {
                // Plain top-level C function (e.g. main()): may still
                // contain message sends. No self/ivars in scope.
                let mut ctx = EmitCtx {
                    src: source,
                    program,
                    class_name: String::new(),
                    scope: HashMap::new(),
                    locals: std::collections::HashSet::new(),
                    diags: Vec::new(),
                    hoisted_blocks: Vec::new(),
                    hoisted_structs: Vec::new(),
                    hoisted_string_literals: Vec::new(),
                    hoisted_statics: Vec::new(),
                    block_counter: 0,
                    pre_stmts: Vec::new(),
                    sync_cleanups: Vec::new(),
                    method_return_type: "int".to_string(),
                };
                let mut c2 = node.walk();
                if let Some(body) =
                    node.children(&mut c2).find(|c| c.kind() == "compound_statement")
                {
                    if needs_translation(body) {
                        collect_local_decls(body, &mut ctx);
                        let text = render_body_with_comments(body, &mut ctx);
                        if text != node_text(body, source) {
                            patches.push(Patch { start: body.start_byte(), end: body.end_byte(), text });
                        }
                    }
                }
                diags.extend(ctx.diags);
                hoisted_blocks.extend(ctx.hoisted_blocks);
                hoisted_structs.extend(ctx.hoisted_structs);
                hoisted_string_literals.extend(ctx.hoisted_string_literals);
                hoisted_statics.extend(ctx.hoisted_statics);
            }
            _ => {}
        }
    }

    patches.sort_by(|a, b| b.start.cmp(&a.start));
    let mut out = source.to_string();
    for p in &patches {
        out.replace_range(p.start..p.end, &p.text);
    }

    // Hoisted-block prototypes go ahead of every call site (so forward
    // references to a not-yet-defined function still compile); the
    // definitions are appended once, after everything else, so each one
    // still sees whatever file-scope static/global it references,
    // wherever in the original source that was declared.
    let mut prototypes = String::new();
    let mut definitions = String::new();

    // `__block`-qualified locals, promoted to file-scope statics (see
    // `hoist_block_var`) -- fully self-contained (only a simple literal
    // initializer survives the promotion), so unlike blocks/string
    // literals below there's no separate prototype/definition split:
    // the whole `static TYPE name [= init];` line just needs to precede
    // every reference to it, guaranteed by living in `prototypes`, ahead
    // of the class code in `out` and of the hoisted block definitions
    // that may reference it.
    if !hoisted_statics.is_empty() {
        prototypes.push_str("/* __block-qualified locals, promoted to file scope */\n");
        for (_, decl) in &hoisted_statics {
            prototypes.push_str(decl);
            prototypes.push('\n');
        }
        prototypes.push('\n');
    }

    if !hoisted_blocks.is_empty() {
        prototypes.push_str("/* non-capturing blocks, hoisted out of their enclosing methods -- prototypes (defined below, after every class) */\n");
        definitions.push_str("\n/* non-capturing blocks, hoisted out of their enclosing methods */\n");
        for (prototype, definition) in &hoisted_blocks {
            prototypes.push_str(prototype);
            definitions.push_str(definition);
            definitions.push('\n');
        }
        prototypes.push('\n');
    }

    // Boxed string literals (`@"..."`) -- same prototype-ahead /
    // definition-after split as blocks, for the same reason: the real
    // definition needs `struct OZString` (defined inline at OZString's
    // own `@interface`, which may appear later in the source than an
    // earlier class's use of the literal) already visible.
    if !hoisted_string_literals.is_empty() {
        prototypes.push_str("/* boxed string literals, hoisted -- extern forward declarations (defined below, after every class) */\n");
        definitions.push_str("\n/* boxed string literals, hoisted -- static struct OZString instances */\n");
        for (prototype, definition) in &hoisted_string_literals {
            prototypes.push_str(prototype);
            definitions.push_str(definition);
        }
        prototypes.push('\n');
    }

    out = format!(
        "/* Auto-generated by oz_static -- do not edit */\n#include \"oz_static_dispatch.h\"\n\n{}{}{}",
        prototypes, out, definitions
    );

    let (companion_h, companion_c) =
        crate::companion::render(program, &hoisted_structs, &hoisted_enums, &hoisted_forward_decls);

    EmitOutput { source_c: out, companion_h, companion_c, diagnostics: diags }
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

/// Origin-aware sibling of `emit()` (OZ-096): instead of one combined
/// `source_c` covering the whole (possibly multi-file,
/// `#import`-resolved) `source`, buckets each top-level construct's
/// already-rendered text by which `origins` range it falls in, and by
/// whether it's interface-shaped (struct + prototypes, no bodies --
/// exactly what `class_interface` already renders as) or
/// implementation-shaped (method bodies -- exactly what
/// `class_implementation` already renders as). Reuses every render_*
/// helper `emit()` itself uses, completely unchanged; only the outer
/// assembly differs. `emit()` itself is untouched, still used directly
/// by every existing test via `transpile()`, which has no concept of
/// multiple origin files.
///
/// `origins` is `imports::ResolvedSource::origins`: an ordered list of
/// `(stem, byte_range)` covering every byte of `source` (the same stem
/// may appear more than once, non-contiguously).
pub fn emit_split(source: &str, program: &Program, origins: &[(String, Range<usize>)]) -> EmitSplitOutput {
    let tree = crate::parse::parse(source);
    let root = tree.root_node();

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
    let mut hoisted_structs: Vec<(String, String)> = Vec::new();
    let mut hoisted_enums: Vec<String> = Vec::new();
    let mut hoisted_forward_decls: Vec<String> = Vec::new();

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
                let mut ctx = EmitCtx {
                    src: source,
                    program,
                    class_name: name.clone(),
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
                };
                let (header_part, alloc_free_part) = render_interface(node, &mut ctx, program);
                diags.extend(ctx.diags);
                hoisted_structs.extend(ctx.hoisted_structs);
                headers.entry(stem.clone()).or_default().push(header_part);
                if !alloc_free_part.is_empty() {
                    bodies.entry(stem.clone()).or_default().push(alloc_free_part);
                }
            }
            "class_implementation" => {
                let (name, _, category) = crate::collect::class_header(node, source);
                let is_category_impl = category.is_some();
                let ivars_scope = base_scope(&name, program);
                let mut ctx = EmitCtx {
                    src: source,
                    program,
                    class_name: name.clone(),
                    scope: ivars_scope.clone(),
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
                };
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
            "struct_specifier" => {
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
                }
            }
            "function_definition" => {
                let mut ctx = EmitCtx {
                    src: source,
                    program,
                    class_name: String::new(),
                    scope: HashMap::new(),
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
                };
                let mut text = node_text(node, source).to_string();
                let mut c2 = node.walk();
                if let Some(body) = node.children(&mut c2).find(|c| c.kind() == "compound_statement") {
                    if needs_translation(body) {
                        collect_local_decls(body, &mut ctx);
                        let rendered_body = render_body_with_comments(body, &mut ctx);
                        if rendered_body != node_text(body, source) {
                            let prefix = &source[node.start_byte()..body.start_byte()];
                            text = format!("{}{}", prefix, rendered_body);
                        }
                    }
                }
                diags.extend(ctx.diags);
                hoisted_structs.extend(ctx.hoisted_structs);
                hoisted_blocks_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_blocks);
                hoisted_strings_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_string_literals);
                hoisted_statics_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_statics);
                bodies.entry(stem.clone()).or_default().push(text);
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
                let text = node_text(node, source).trim();
                if text.is_empty() {
                    continue;
                }
                if node.kind().starts_with("preproc") {
                    headers.entry(stem.clone()).or_default().push(text.to_string());
                } else {
                    bodies.entry(stem.clone()).or_default().push(text.to_string());
                }
            }
        }
    }

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
    let mut always_visible: Vec<String> = Vec::new();
    if let Some(r) = program.root_class().and_then(|r| class_to_stem.get(r).cloned()) {
        always_visible.push(r);
    }
    for helper_class in ["OZArray", "OZDictionary"] {
        if let Some(s) = class_to_stem.get(helper_class) {
            always_visible.push(s.clone());
        }
    }
    for target_stem in &always_visible {
        for stem in &stem_order {
            if stem != target_stem {
                extra_includes.entry(stem.clone()).or_default().insert(target_stem.clone());
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
            "/* Auto-generated by oz_static -- do not edit */\n#include \"oz_static_dispatch.h\"\n#include \"{}.h\"\n\n",
            stem
        );
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
        crate::companion::render(program, &hoisted_structs, &hoisted_enums, &hoisted_forward_decls);

    EmitSplitOutput { files, companion_h, companion_c, diagnostics: diags }
}

fn base_scope(class_name: &str, program: &Program) -> HashMap<String, String> {
    program.all_ivars(class_name).into_iter().collect()
}
