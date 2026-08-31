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
    /// Object locals ARC manages as strong variables, so that an overwrite
    /// releases what was there (`emit::managed_object_locals`). An
    /// allocation stored into one of these is bounded at a single live
    /// instance however many times the loop runs, which is what lets the
    /// loop rule below tell *reassignment* apart from *accumulation*.
    arc_managed_locals: &'a HashSet<String>,
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

/// Is this allocation's result stored straight into a strong local that ARC
/// manages?
///
/// This is what separates the two shapes the loop rule used to conflate:
///
/// ```objc
/// /* reassignment -- bounded at one live instance */
/// Counter *c;
/// for (...) { c = [Counter alloc]; }
///
/// /* accumulation -- genuinely N live instances */
/// for (...) { [arr addObject:[Counter alloc]]; }
/// ```
///
/// In the first, `emit::render_strong_local_assign` releases the previous
/// object *before* allocating the next, so the slab slot is returned and
/// immediately reusable -- one slot serves the whole loop, and the
/// occurrence count `pools::count_sites` produced is right. In the second,
/// nothing releases anything and the count is a floor the program walks
/// straight through, so it stays a hard error.
///
/// The climb only follows a *receiver* position, never an argument.
/// `[[Counter alloc] init]` keeps the allocation's identity, so the store
/// that matters is the outer send's; `[arr addObject:[Counter alloc]]` does
/// not, and treating its enclosing assignment as the destination would
/// accept exactly the accumulating shape this rule exists for.
fn stored_into_managed_local(node: Node, src: &str, scope: &MethodScope) -> bool {
    let mut cur = node;
    loop {
        let Some(parent) = cur.parent() else {
            return false;
        };
        match parent.kind() {
            "parenthesized_expression" | "cast_expression" => {
                cur = parent;
            }
            "message_expression" => {
                // Only climb when `cur` is the receiver.
                let mut c = parent.walk();
                let parts: Vec<Node> = parent
                    .children(&mut c)
                    .filter(|n| n.kind() != "[" && n.kind() != "]")
                    .collect();
                match parts.first() {
                    Some(receiver) if receiver.id() == cur.id() => {
                        cur = parent;
                    }
                    _ => return false,
                }
            }
            "assignment_expression" => {
                let mut c = parent.walk();
                let parts: Vec<Node> = parent.children(&mut c).collect();
                if parts.len() >= 3
                    && parts[0].kind() == "identifier"
                    && node_text(parts[1], src) == "="
                    && parts.last().map(|n| n.id()) == Some(cur.id())
                {
                    return scope.arc_managed_locals.contains(node_text(parts[0], src));
                }
                return false;
            }
            _ => return false,
        }
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
            if selector == "alloc"
                && in_loop
                && !fresh_decl
                && !stored_into_managed_local(node, src, scope)
            {
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
        // (`@{...}`) are accepted in general -- they desugar to
        // OZArray_oz_initWithItems / OZDictionary_oz_initWithKeysValues
        // calls in emit.rs -- but they allocate, so they are held to the
        // same loop rule as an explicit `alloc` above. Sizing counts one
        // site once however many times it runs (see `pools`), which is
        // sound only when each iteration's instance dies before the next
        // begins; a literal in a loop that is *not* bound to a fresh
        // per-iteration local can accumulate live instances the static
        // count cannot bound, exhausting both the OZArray/OZDictionary
        // slab and the shared element pool. This arm does not return:
        // child nodes (elements; key/value pairs) still get walked by the
        // default descent below, so an unsupported construct nested
        // inside one of them is still caught.
        "array_literal" | "dictionary_literal"
            if in_loop && !fresh_decl && !stored_into_managed_local(node, src, scope) =>
        {
            let what = if node.kind() == "array_literal" {
                "boxed array literal"
            } else {
                "boxed dictionary literal"
            };
            err(diags, src, node, format!(
                "a {} inside a loop escapes the iteration (not a fresh per-iteration local) — the static subset cannot bound how many live instances this may need; hoist it out of the loop or restructure",
                what));
        }
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

/// Reject `[_ivar release]` inside `-dealloc` for an ivar the class already
/// owns, because that release is emitted automatically
/// (`companion::render_release_ivars`) and doing both is a double free.
///
/// This is the one place oz_static deliberately diverges from the oracle
/// rather than following it. `emit.py::_emit_user_dealloc` appends the
/// owned-ivar releases *after* the user's body, so a `-dealloc` written in
/// ordinary manual-retain/release style -- releasing what it owns -- has
/// every one of those ivars released twice, silently. Real ARC does not
/// paper over that: it makes an explicit `release` a compile error, and the
/// safety comes from the rejection. Rejecting is also the only option
/// consistent with never silently degrading.
///
/// Only owned object ivars are rejected. Releasing a local, a parameter, or
/// an `__unsafe_unretained` ivar the author manages by hand is untouched --
/// nothing releases those automatically.
fn check_dealloc_body(
    body: Node,
    src: &str,
    program: &Program,
    class_info: &ClassInfo,
    diags: &mut Vec<Diagnostic>,
) {
    let owned = program.owned_object_ivar_names(&class_info.name);
    if owned.is_empty() {
        return;
    }
    fn walk(
        node: Node,
        src: &str,
        owned: &[String],
        class_name: &str,
        diags: &mut Vec<Diagnostic>,
    ) {
        if node.kind() == "message_expression" {
            let mut cursor = node.walk();
            let parts: Vec<Node> = node
                .children(&mut cursor)
                .filter(|c| c.kind() != "[" && c.kind() != "]")
                .collect();
            if parts.len() == 2 && node_text(parts[1], src) == "release" {
                let receiver = node_text(parts[0], src);
                if owned.iter().any(|ivar| ivar == receiver) {
                    err(
                        diags,
                        src,
                        node,
                        format!(
                            "'{recv}' is released automatically when a {class} is deallocated, so \
                             releasing it here would release it twice -- drop this line (the \
                             generated {class}_oz_release_ivars does it). Declare the ivar \
                             '__unsafe_unretained' if this class does not own it.",
                            recv = receiver,
                            class = class_name
                        ),
                    );
                }
            }
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, src, owned, class_name, diags);
        }
    }
    walk(body, src, &owned, &class_info.name, diags);
}

pub fn check_method_body(
    body: Node,
    src: &str,
    program: &Program,
    class_info: &ClassInfo,
    params: &[(String, String)],
    selector: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if selector == "dealloc" {
        check_dealloc_body(body, src, program, class_info, &mut diags);
    }
    let ivar_names: HashSet<String> =
        program.all_ivars(&class_info.name).into_iter().map(|(n, _)| n).collect();
    let managed = crate::emit::managed_object_locals(body, src, program);
    let mut scope = MethodScope {
        class_ivars: &ivar_names,
        arc_managed_locals: &managed,
        locals: HashSet::new(),
        block_locals: HashSet::new(),
    };
    for (name, _) in params {
        scope.locals.insert(name.clone());
    }
    walk_for_reject(body, src, &mut scope, false, false, &mut diags);
    diags
}

/// The same accept/reject scan, over a plain top-level C function's body.
///
/// A `.m` file's file-scope functions -- `main()` above all -- can contain
/// Objective-C, and `emit` transpiles it there exactly as it does in a
/// method. The bar, however, was entered from one place only: the
/// `@implementation` method-body renderer. So every check was skipped for
/// code in a free function -- not just the allocation rule but `@try`,
/// reflection selectors, `@selector`/`@protocol`, `@synchronized` bodies with
/// an escaping jump, and block captures of stack locals.
///
/// Most of those fail loudly anyway, by reaching `emit` and producing C that
/// does not compile. The allocation rule was the one with a *silent*
/// consequence: pool sizing counts a site once however many times it runs, so
/// an unbounded loop in `main()` was sized as though it allocated once, and
/// that surfaced at run time as an unexpected nil rather than at build time
/// as a diagnostic.
///
/// No `MethodScope` mode is needed for this. `class_ivars` is read in exactly
/// one place -- `find_capture`, which asks whether a name a block closes over
/// is an ivar -- and a free function has none, so the empty set is not a
/// stand-in but the truth. Seeding it from some nearby class instead would
/// invent captures: `samples/gpio_demo`'s `[led toggle]` inside a block in
/// `main` would be flagged the moment any class in that file declared an ivar
/// named `led`. `check_dealloc_body` is likewise inapplicable and is gated on
/// the selector, not called here.
pub fn check_function_body(body: Node, src: &str, program: &Program) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let no_ivars: HashSet<String> = HashSet::new();
    let managed = crate::emit::managed_object_locals(body, src, program);
    let mut scope = MethodScope {
        class_ivars: &no_ivars,
        arc_managed_locals: &managed,
        locals: HashSet::new(),
        block_locals: HashSet::new(),
    };
    walk_for_reject(body, src, &mut scope, false, false, &mut diags);
    diags
}
