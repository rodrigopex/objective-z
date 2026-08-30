// SPDX-License-Identifier: Apache-2.0
//
// staticbar.rs - accept/reject scan for the static subset.
//
// Philosophy carried over from OZ-091 Track A: never silently degrade or
// best-effort a construct outside the static bar. Anything not explicitly
// supported is a named, located hard error.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::model::{ClassInfo, Diagnostic, Program};
use crate::parse::line_col;

const REFLECTION_SELECTORS: &[&str] = &[
    "respondsToSelector:",
    "performSelector:",
    "performSelector:withObject:",
    "performSelector:withObject:withObject:",
    "isKindOfClass:",
    "isMemberOfClass:",
    "conformsToProtocol:",
];

const LOOP_KINDS: &[&str] = &["for_statement", "while_statement", "do_statement"];

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.start_byte()..node.end_byte()]
}

fn err(diags: &mut Vec<Diagnostic>, src: &str, node: Node, message: impl Into<String>) {
    let (line, col) = line_col(src, node.start_byte());
    diags.push(Diagnostic::new(message, line, col));
}

fn message_selector(node: Node, src: &str) -> String {
    // message_expression: [ receiver piece1 : arg1 piece2 : arg2 ... ]
    // Selector pieces are `identifier` children immediately followed by a
    // `:` sibling; the very first identifier is the receiver, so skip it.
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let mut selector = String::new();
    let mut seen_receiver = false;
    let mut i = 0;
    while i < children.len() {
        let c = children[i];
        if c.kind() == "identifier" {
            if !seen_receiver {
                seen_receiver = true;
                i += 1;
                continue;
            }
            // A selector piece is an identifier followed by ':'.
            if children.get(i + 1).map(|n| n.kind()) == Some(":") {
                selector.push_str(node_text(c, src));
                selector.push(':');
                i += 2;
                continue;
            }
            // Bare identifier with no following ':' and no ':' anywhere in
            // this message -> unary selector (only valid as the sole piece).
            if selector.is_empty() {
                selector.push_str(node_text(c, src));
            }
        }
        i += 1;
    }
    selector
}

struct MethodScope<'a> {
    class_ivars: &'a HashSet<String>,
    locals: HashSet<String>,
    /// `__block`-qualified locals (tree-sitter-objc parses `__block` as a
    /// `type_qualifier` child of the `declaration` node -- confirmed
    /// against the vendored grammar, there is no dedicated node kind for
    /// it). Mirrors oz_transpile's BlocksAttr promotion-to-static: these
    /// are exempt from the capture check in `find_capture` below, since
    /// emit.rs hoists them to file-scope statics rather than leaving them
    /// as real stack locals a block would need to close over.
    block_locals: HashSet<String>,
}

/// `@synchronized` lowers to an explicit `oz_spin_lock` / `oz_spin_unlock`
/// pair around the body (see `emit::render_synchronized_statement`), so a
/// jump out of the body would skip the unlock and leave the lock held
/// forever. Rather than silently emitting that deadlock, reject the jump
/// and say how to restructure.
///
/// `return` is exempt: `emit::render_return_statement` replays the pending
/// unlock ahead of it, so an early return works (as it does in the
/// oracle -- `tests/behavior/cases/synchronized/early_return.m`).
/// `break`/`continue` are only a problem when they escape the body; one
/// belonging to a loop or switch *inside* the body is fine, so the walk
/// stops treating them as escaping once it descends into one.
fn check_synchronized_body(sync_node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    fn walk(node: Node, src: &str, in_nested_breakable: bool, diags: &mut Vec<Diagnostic>) {
        let escaping = match node.kind() {
            "goto_statement" => Some("goto"),
            "break_statement" if !in_nested_breakable => Some("break"),
            "continue_statement" if !in_nested_breakable => Some("continue"),
            _ => None,
        };
        if let Some(keyword) = escaping {
            err(
                diags,
                src,
                node,
                format!(
                    "'{}' inside @synchronized would skip the unlock and leave the lock held \
                     (the static subset emits an explicit oz_spin_lock/oz_spin_unlock pair, not a \
                     scope guard) -- move the value out to a local, end the @synchronized block, \
                     then '{}'",
                    keyword, keyword
                ),
            );
            return;
        }
        // A loop or switch inside the body captures its own
        // break/continue, so those no longer escape.
        let captures_break = matches!(
            node.kind(),
            "for_statement" | "while_statement" | "do_statement" | "switch_statement"
        );
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, src, in_nested_breakable || captures_break, diags);
        }
    }

    let mut cursor = sync_node.walk();
    let body =
        sync_node.children(&mut cursor).find(|c| c.kind() == "compound_statement");
    if let Some(body) = body {
        walk(body, src, false, diags);
    }
}

fn walk_for_reject(
    node: Node,
    src: &str,
    scope: &mut MethodScope,
    in_loop: bool,
    fresh_decl: bool,
    diags: &mut Vec<Diagnostic>,
) {
    match node.kind() {
        "try_statement" => {
            err(diags, src, node, "@try/@catch is not supported in the static subset (exception handling requires runtime unwinding info this backend does not generate)");
            return;
        }
        "synchronized_statement" => {
            check_synchronized_body(node, src, diags);
        }
        "message_expression" => {
            let selector = message_selector(node, src);
            if REFLECTION_SELECTORS.contains(&selector.as_str()) {
                err(
                    diags,
                    src,
                    node,
                    format!(
                        "'{}' is reflection, which the static subset rejects (no runtime type/selector registry is generated)",
                        selector
                    ),
                );
            }
            if selector == "alloc" && in_loop && !fresh_decl {
                let class_name = node_text(node, src)
                    .trim_start_matches('[')
                    .split_whitespace()
                    .next()
                    .unwrap_or("?");
                err(diags, src, node, format!(
                    "allocation of '{}' inside a loop escapes the iteration (not a fresh per-iteration local) — the static subset cannot bound how many live instances this may need; hoist it out of the loop or restructure",
                    class_name));
            }
        }
        "block_literal" => {
            check_block_capture(node, src, scope, diags);
            return; // don't descend further with loop/decl context; block is opaque
        }
        // `array_literal` (`@[...]`) and `dictionary_literal`
        // (`@{...}`) are not rejected -- they desugar to
        // OZArray_oz_initWithItems / OZDictionary_oz_initWithKeysValues
        // calls in emit.rs. Their child nodes (elements; key/value
        // pairs) still get walked normally (falling through to the
        // default descent below), so an unsupported construct nested
        // inside one of them is still caught.
        //
        // `selector_expression` (`@selector(...)`) is a real node kind
        // in tree-sitter-objc 3.0.2 (confirmed against its
        // node-types.json) and is rejected directly here.
        // `protocol_expression` -- unlike `selector_expression` --
        // isn't: `@protocol(Foo)` parses as a generic `at_expression`
        // (see the `at_expression` arm below), the same class of bug
        // already found and fixed for `boxed_expression` in #191. This
        // match arm never fired for it; a dedicated `at_expression`
        // sub-case now gives `@protocol(...)` its own clear message
        // instead of relying on the generic boxed-literal one.
        "selector_expression" => {
            err(
                diags,
                src,
                node,
                "'selector_expression' is not in the static subset's accepted construct set",
            );
            return;
        }
        // tree-sitter-objc 3.0.2 parses every `@`-prefixed boxed literal --
        // `@42`, `@3.14f`, `@(expr)`, `@YES`, `@(call())`, even
        // `@protocol(Foo)` -- as a single generic `at_expression` node
        // (there is no dedicated `boxed_expression` or `protocol_expression`
        // node kind in this grammar version). A numeric/boolean-shaped one
        // (see `emit::is_numeric_boxed_shape`) desugars to an OZQ31 class-
        // method call, handled in `emit.rs`; a `@protocol(Name)`-shaped one
        // (see `emit::is_protocol_literal_shape`) gets its own message
        // below; anything else (a boxed call expression, etc.) has no
        // desugaring and must still be rejected here, or the emitter's
        // catch-all would pass the raw `@(...)` text straight through as
        // bogus C.
        "at_expression" if crate::emit::is_protocol_literal_shape(node, src) => {
            err(
                diags,
                src,
                node,
                "'@protocol(...)' is not in the static subset's accepted construct set",
            );
            return;
        }
        "at_expression" if !crate::emit::is_numeric_boxed_shape(node, src) => {
            err(
                diags,
                src,
                node,
                "this '@'-boxed expression is not in the static subset's accepted construct set (only a numeric/boolean literal like '@42', '@3.5f', or '@YES' desugars to an OZQ31 class-method call)",
            );
            return;
        }
        _ => {}
    }

    let child_in_loop = in_loop || LOOP_KINDS.contains(&node.kind());

    if node.kind() == "declaration" {
        // A declaration's own init_declarator initializer is "fresh" only
        // when the declaration itself sits directly in a loop body.
        let is_block_qualified = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .any(|c| c.kind() == "type_qualifier" && node_text(c, src) == "__block");
            found
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "init_declarator" {
                // record the declared name as a local
                if let Some(name) = find_first_identifier_before_eq(child, src) {
                    scope.locals.insert(name.clone());
                    if is_block_qualified {
                        scope.block_locals.insert(name);
                    }
                }
                let mut c2 = child.walk();
                for gc in child.children(&mut c2) {
                    walk_for_reject(gc, src, scope, child_in_loop, true, diags);
                }
            } else {
                walk_for_reject(child, src, scope, child_in_loop, fresh_decl, diags);
            }
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_reject(child, src, scope, child_in_loop, fresh_decl, diags);
    }
}

fn check_block_capture(node: Node, src: &str, scope: &MethodScope, diags: &mut Vec<Diagnostic>) {
    // Anything the block itself declares/binds is not a capture.
    let mut own_names: HashSet<String> = HashSet::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_bound_names(child, src, &mut own_names);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_capture(child, src, scope, &own_names, diags);
    }
}

fn collect_bound_names(node: Node, src: &str, out: &mut HashSet<String>) {
    match node.kind() {
        "parameter_declaration" => {
            if let Some(id) = find_last_identifier(node, src) {
                out.insert(id);
            }
        }
        "init_declarator" | "declaration" => {
            if let Some(id) = find_first_identifier_before_eq(node, src) {
                out.insert(id);
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_bound_names(child, src, out);
    }
}

fn find_last_identifier(node: Node, src: &str) -> Option<String> {
    let mut result = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            result = Some(node_text(child, src).to_string());
        } else {
            result = find_last_identifier(child, src).or(result);
        }
    }
    result
}

fn find_first_identifier_before_eq(node: Node, src: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "=" {
            break;
        }
        if child.kind() == "identifier" {
            return Some(node_text(child, src).to_string());
        }
        if let Some(found) = find_first_identifier_before_eq(child, src) {
            return Some(found);
        }
    }
    None
}

fn find_capture(
    node: Node,
    src: &str,
    scope: &MethodScope,
    own_names: &HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    if node.kind() == "identifier" {
        let name = node_text(node, src);
        if own_names.contains(name) || scope.block_locals.contains(name) {
            return;
        }
        if name == "self" || scope.class_ivars.contains(name) || scope.locals.contains(name) {
            err(
                diags,
                src,
                node,
                format!(
                    "block captures '{}' from the enclosing scope; the static subset only accepts non-capturing blocks",
                    name
                ),
            );
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_capture(child, src, scope, own_names, diags);
    }
}

pub fn check_method_body(
    body: Node,
    src: &str,
    program: &Program,
    class_info: &ClassInfo,
    params: &[(String, String)],
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let ivar_names: HashSet<String> =
        program.all_ivars(&class_info.name).into_iter().map(|(n, _)| n).collect();
    let mut scope =
        MethodScope { class_ivars: &ivar_names, locals: HashSet::new(), block_locals: HashSet::new() };
    for (name, _) in params {
        scope.locals.insert(name.clone());
    }
    walk_for_reject(body, src, &mut scope, false, false, &mut diags);
    diags
}
