// SPDX-License-Identifier: Apache-2.0
//
// arc.rs - which expressions hand back a reference the caller owns.
//
// Scope-based release (see `emit::render_body_with_comments`) has to know
// what it may release. Releasing a borrowed reference is a double free;
// failing to release an owned one is a leak. So every local this decides to
// release must be provably +1, and everything else is left alone.
//
// Two questions, not one, and they have different answers. Scope-based
// release asks whether a local holding a value may be released when its
// scope ends (`is_owning_expr`); a statement whose value is bound to
// nothing asks whether discarding it abandons a reference nothing else
// accounts for (`discards_ownership`, #322). `-retain` and `-init...` are
// +1 to the first and not to the second, because the reference they hand
// back is one something else is already tracking.
//
// Ported from the oracle's `_is_owning_expr` / `_find_owning_return_methods`
// (tools/oz_transpile/emit.py), with one improvement: the oracle's scan is a
// single pass, so a factory whose returns call *another* factory is not
// recognised. This iterates to a fixed point, which costs one more pass over
// a symbol table and catches that chain.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::model::Program;

/// This module indexes `src` directly elsewhere; a named helper keeps the
/// new code readable.
fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

/// `(class, selector)` for every method whose every return path hands back a
/// +1 reference, so a caller storing the result must not retain it again and
/// *must* release it.
#[derive(Debug, Default, Clone)]
pub struct OwningMethods {
    methods: HashSet<(String, String)>,
    /// Plain top-level C functions whose every return path hands back +1.
    ///
    /// A helper like `samples/arc_demo`'s
    /// `static Sensor *createSensor(int v)` is exactly as owning as a
    /// factory method, and its callers own what it returns. Left out, the
    /// local holding its result was treated as borrowed and never released
    /// -- and the sample's own comment says otherwise ("s is released here
    /// by ARC"). On target that showed up as an MPU fault: the one-slot
    /// Sensor slab stayed occupied, the next allocation returned NULL, and
    /// `-initWithValue:` wrote through it.
    functions: HashSet<String>,
}

impl OwningMethods {
    pub fn contains(&self, class: &str, selector: &str) -> bool {
        self.methods.contains(&(class.to_string(), selector.to_string()))
    }

    /// Does the plain C function `name` return +1?
    pub fn contains_function(&self, name: &str) -> bool {
        self.functions.contains(name)
    }

    pub fn len(&self) -> usize {
        self.methods.len() + self.functions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.functions.is_empty()
    }
}

/// Selectors that are +1 by convention rather than by analysis, matching
/// Objective-C's own naming rule (the "create rule"): these transfer
/// ownership whatever their body does.
fn is_owning_selector(selector: &str) -> bool {
    selector == "alloc"
        // `+allocWithHeap:` is `+alloc` with the storage coming from an
        // OZHeap, so it hands back +1 just the same. Missing from this list,
        // `samples/heap_alloc` leaked every object it allocated: nothing
        // released them, no `-dealloc` ran, and the heap's used-bytes never
        // came back down -- which the sample's own expected output
        // ("app heap after free: 0 bytes used", "Sensor dealloc") states.
        // Compiling and linking cannot catch that; only running it can.
        || selector == "allocWithHeap:"
        || selector == "new"
        || selector == "copy"
        || selector == "mutableCopy"
        || selector == "retain"
        || selector.starts_with("init")
}

/// Find every method that returns +1, iterating until the set stops growing.
pub fn analyze(source: &str, program: &Program) -> OwningMethods {
    let tree = crate::parse::parse(source);
    let mut owning = OwningMethods::default();
    loop {
        let before = owning.methods.len();
        scan_once(tree.root_node(), source, program, &mut owning);
        if owning.methods.len() == before {
            return owning;
        }
    }
}

fn scan_once(root: Node, src: &str, program: &Program, owning: &mut OwningMethods) {
    fn walk(
        node: Node,
        src: &str,
        program: &Program,
        owning: &mut OwningMethods,
        class: Option<&str>,
    ) {
        if node.kind() == "function_definition" {
            consider_function(node, src, program, owning);
            return;
        }
        let class = if node.kind() == "class_implementation" {
            // Only an @implementation has bodies to analyse.
            let (name, _, _) = crate::collect::class_header(node, src);
            if name.is_empty() {
                class
            } else {
                // Leaked into the recursion below via the owned String's
                // lifetime, so it is resolved eagerly here instead.
                return walk_impl(node, src, program, owning, &name);
            }
        } else {
            class
        };
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, src, program, owning, class);
        }
    }

    fn walk_impl(
        node: Node,
        src: &str,
        program: &Program,
        owning: &mut OwningMethods,
        class: &str,
    ) {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            if child.kind() == "implementation_definition" {
                let mut c2 = child.walk();
                let defs: Vec<Node> = child.children(&mut c2).collect();
                for def in defs {
                    if def.kind() == "method_definition" {
                        consider_method(def, src, program, owning, class);
                    }
                }
            }
        }
    }

    /// Same rule as `consider_method`, for a plain C function: an
    /// object-returning function whose every return path is owning is
    /// itself owning.
    fn consider_function(
        function: Node,
        src: &str,
        program: &Program,
        owning: &mut OwningMethods,
    ) {
        let Some(name) = function_name(function, src) else {
            return;
        };
        if owning.functions.contains(&name) {
            return;
        }
        let mut cursor = function.walk();
        let children: Vec<Node> = function.children(&mut cursor).collect();
        // Only a pointer return can carry ownership. Checked on the
        // declared type's own text, ahead of the body, so a function
        // returning a struct by value is skipped.
        let returns_pointer = children.iter().any(|c| {
            matches!(c.kind(), "pointer_declarator" | "function_declarator")
                && node_text(*c, src).contains('*')
        });
        if !returns_pointer {
            return;
        }
        let Some(body) = children.iter().find(|c| c.kind() == "compound_statement") else {
            return;
        };
        let returns = collect_returns(*body);
        if returns.is_empty() {
            return;
        }
        let all_owning = returns
            .iter()
            .all(|ret| return_hands_back_ownership(*ret, *body, src, program, owning));
        if all_owning {
            owning.functions.insert(name);
        }
    }

    fn consider_method(
        method: Node,
        src: &str,
        program: &Program,
        owning: &mut OwningMethods,
        class: &str,
    ) {
        let known: HashSet<String> = program.classes.keys().cloned().collect();
        let sig = crate::collect::extract_method_sig(method, src, class, &known);
        // Only an object-returning method can hand back ownership, and the
        // convention-named ones are already owning without analysis.
        if is_owning_selector(&sig.selector) {
            return;
        }
        if !sig.return_type.contains('*') {
            return;
        }
        let mut cursor = method.walk();
        let body = method.children(&mut cursor).find(|c| c.kind() == "compound_statement");
        let Some(body) = body else {
            return;
        };
        let returns = collect_returns(body);
        if returns.is_empty() {
            return;
        }
        // Every path must be owning. One borrowed return makes the whole
        // method +0, because the caller cannot tell the paths apart.
        // Same shape as the function case above: a factory method that
        // returns a local is just as owning as one that returns the
        // allocation directly.
        let all_owning = returns
            .iter()
            .all(|ret| return_hands_back_ownership(*ret, body, src, program, owning));
        if all_owning {
            owning.methods.insert((class.to_string(), sig.selector));
        }
    }

    walk(root, src, program, owning, None);
}

/// Does this `return` hand back a reference the caller owns?
///
/// `is_owning_expr` alone is not enough, because the idiomatic factory
/// returns a *variable*:
///
/// ```objc
/// static Sensor *createSensor(int v)
/// {
///         Sensor *s = [[Sensor alloc] init];
///         [s setValue:v];
///         return s;
/// }
/// ```
///
/// `samples/arc_demo` is built on that shape, and with the returned
/// identifier read as borrowed the function looked +0, its callers released
/// nothing, and the one-slot Sensor slab stayed occupied -- an MPU fault on
/// target at the next allocation.
///
/// So a returned identifier is followed back to its declaration, and counts
/// as owning when that declaration's initialiser is. Requiring the name to
/// be assigned nowhere else keeps the usual bias: a variable that is
/// reassigned might hold something borrowed by the time it is returned, and
/// guessing wrong in that direction is a double free, where guessing wrong
/// the other way only leaks.
fn return_hands_back_ownership(
    ret: Node,
    body: Node,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> bool {
    let Some(value) = value_of_return(ret) else {
        return false;
    };
    if is_owning_expr(value, src, program, owning) {
        return true;
    }
    if value.kind() != "identifier" {
        return false;
    }
    let name = node_text(value, src);
    if is_reassigned(body, src, name) {
        return false;
    }
    declared_initializer(body, src, name)
        .is_some_and(|init| is_owning_expr(init, src, program, owning))
}

/// The initialiser of `name`'s declaration inside `node`, if it has one.
fn declared_initializer<'a>(node: Node<'a>, src: &str, name: &str) -> Option<Node<'a>> {
    if node.kind() == "init_declarator" {
        let mut cursor = node.walk();
        let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
        let declares = children.iter().any(|c| {
            matches!(c.kind(), "identifier" | "pointer_declarator")
                && node_text(*c, src).trim_start_matches('*').trim() == name
        });
        if declares {
            return children.into_iter().rev().find(|c| {
                !matches!(c.kind(), "=" | "identifier" | "pointer_declarator")
            });
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = declared_initializer(child, src, name) {
            return Some(found);
        }
    }
    None
}

/// Is `name` the target of an assignment anywhere in `node`?
fn is_reassigned(node: Node, src: &str, name: &str) -> bool {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    if node.kind() == "assignment_expression" {
        if let Some(lhs) = children.first() {
            if lhs.kind() == "identifier" && node_text(*lhs, src) == name {
                return true;
            }
        }
    }
    children.into_iter().any(|child| is_reassigned(child, src, name))
}

/// A `function_definition`'s own name, reached through however many
/// declarator layers its return type needs (`static Sensor *f(int)` nests a
/// `pointer_declarator` around the `function_declarator`).
fn function_name(function: Node, src: &str) -> Option<String> {
    fn find_declarator_identifier<'a>(node: Node<'a>, src: &str) -> Option<String> {
        if node.kind() == "function_declarator" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    return Some(node_text(child, src).to_string());
                }
                if let Some(found) = find_declarator_identifier(child, src) {
                    return Some(found);
                }
            }
            return None;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_declarator_identifier(child, src) {
                return Some(found);
            }
        }
        None
    }
    find_declarator_identifier(function, src)
}

/// Every `return_statement` in `body`, not descending into a nested block
/// literal -- that is a separate function with its own returns.
fn collect_returns<'a>(body: Node<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    fn walk<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
        if node.kind() == "block_literal" {
            return;
        }
        if node.kind() == "return_statement" {
            out.push(node);
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, out);
        }
    }
    walk(body, &mut out);
    out
}

fn value_of_return<'a>(ret: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = ret.walk();
    let children: Vec<Node<'a>> = ret.children(&mut cursor).collect();
    children.into_iter().find(|c| c.kind() != "return" && c.kind() != ";")
}

/// Does this expression hand back a reference its receiver owns?
///
/// Deliberately narrow: anything not recognised is treated as borrowed, so
/// an unrecognised shape leaks rather than double-frees. That asymmetry is
/// the whole point -- a leak is a bug, a double free is memory corruption.
///
/// The question is specifically whether a *local* holding this value may be
/// released when its scope ends. It is the wrong one for a value bound to
/// nothing: `[c retain]` and `[f init]` are +1 here and must be, since a
/// local taking one over needs no retain of its own, but discarding either
/// abandons no reference -- see `discards_ownership`, which #322 added for
/// exactly that difference.
pub fn is_owning_expr(
    node: Node,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> bool {
    match node.kind() {
        // `@42` / `@3.5f` allocate an OZQ31; `@[...]`/`@{...}` allocate the
        // collection. A `@"..."` literal is a static, so releasing it is a
        // guarded no-op (see `emit::render_boxed_string_literal`) -- counting
        // it as owning keeps the rule uniform and costs nothing.
        "at_expression" => crate::emit::is_numeric_boxed_shape(node, src),
        "array_literal" | "dictionary_literal" => true,
        "string_literal" => crate::emit::is_boxed_string_literal(node),
        "parenthesized_expression" => {
            let mut cursor = node.walk();
            let inner = node.children(&mut cursor).find(|c| c.kind() != "(" && c.kind() != ")");
            inner.is_some_and(|inner| is_owning_expr(inner, src, program, owning))
        }
        "message_expression" => {
            let (receiver_class, selector) = message_target(node, src, program);
            if is_owning_selector(&selector) {
                return true;
            }
            receiver_class.is_some_and(|class| owning.contains(&class, &selector))
        }
        // A call to a plain C function that returns +1 -- see
        // `OwningMethods::functions`.
        "call_expression" => {
            let mut cursor = node.walk();
            let callee = node.children(&mut cursor).next();
            callee.is_some_and(|callee| {
                callee.kind() == "identifier" && owning.contains_function(node_text(callee, src))
            })
        }
        // A cast says nothing about ownership, and `__bridge` explicitly
        // means "not mine" -- borrowed either way.
        _ => false,
    }
}

/// Of the convention-named +1 selectors, the ones whose reference is a
/// *newly created* object rather than one the send was handed.
///
/// The distinction only matters when the result is thrown away, and there
/// it is the whole question. Two of `is_owning_selector`'s entries hand
/// back a reference something else is already accounting for:
///
///   - `-retain` returns its own receiver, and a bare `[c retain];` is the
///     manual-retain/release idiom whose balancing `[c release];` is
///     written by hand -- `samples/smp_shared` does exactly that, twice per
///     iteration. Releasing the discarded result would undo the retain and
///     free the object out from under the sample. Real ARC has nothing to
///     match here either: it makes an explicit `retain` a compile error.
///   - `-init...` *consumes* the receiver's +1 and hands it back, so the
///     reference is the receiver's. In `[[Foo alloc] init]` that receiver
///     is a temporary nothing tracks, and the result is genuinely
///     abandoned; in
///
///     ```objc
///     Foo *f = [Foo alloc];
///     [f init];
///     ```
///
///     it is `f`, which scope-based ARC already releases at the end of the
///     block. Releasing the discarded result too would be a double free.
///     So an `init` send is followed back to its receiver rather than
///     trusted on its name.
fn creates_reference(selector: &str) -> bool {
    matches!(selector, "alloc" | "allocWithHeap:" | "new" | "copy" | "mutableCopy")
}

/// Does throwing this expression's value away abandon a +1 reference that
/// nothing else releases?
///
/// `is_owning_expr` answers a different question -- may a *local* holding
/// this value be released at scope exit -- and it is the wrong one for a
/// result bound to nothing: it says yes to `[f init]` and `[c retain]`,
/// where the reference belongs to the receiver. Getting that wrong here is
/// a double free rather than a leak, which is the direction this module
/// exists to avoid, so the two selectors that hand back a reference
/// something else accounts for are separated out (`creates_reference`) and
/// an `init` send is resolved through its receiver.
///
/// Everything else is `is_owning_expr` unchanged: a boxed literal, a
/// collection literal, a call to an owning C function and an
/// analysis-derived owning method all produce a fresh object. That an
/// owning *method* cannot be returning `self` is not an assumption --
/// `consider_method` classifies one only when every return path is
/// `is_owning_expr`, and a bare `self` is not.
pub fn discards_ownership(
    node: Node,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> bool {
    if node.kind() == "parenthesized_expression" {
        let mut cursor = node.walk();
        let inner = node.children(&mut cursor).find(|c| c.kind() != "(" && c.kind() != ")");
        return inner.is_some_and(|inner| discards_ownership(inner, src, program, owning));
    }
    if node.kind() != "message_expression" {
        return is_owning_expr(node, src, program, owning);
    }
    let (receiver_class, selector) = message_target(node, src, program);
    // A `-performSelector:` whose selector resolves statically is the same
    // send by another spelling, so it gets the same answer. When it does
    // not resolve, the selector is a run-time value and no +1 can be seen:
    // borrowed, as ever, is the safe reading.
    if let Some(performed) = statically_performed_selector(node, src) {
        return created_by(&performed, receiver_class.as_deref(), owning);
    }
    if selector.starts_with("init") {
        let parts = crate::emit::parse_message(node, src);
        return discards_ownership(parts.receiver, src, program, owning);
    }
    created_by(&selector, receiver_class.as_deref(), owning)
}

/// Does a send of `selector` to a receiver of `class` create a reference
/// its caller owns and nothing else accounts for?
fn created_by(selector: &str, class: Option<&str>, owning: &OwningMethods) -> bool {
    if creates_reference(selector) {
        return true;
    }
    if selector == "retain" || selector.starts_with("init") {
        return false;
    }
    class.is_some_and(|class| owning.contains(class, selector))
}

/// The selector a `-performSelector:` send performs, when the source says
/// so exactly: a `@selector(...)` literal at the call site, or a local
/// declared once from one and never reassigned.
///
/// `SEL` is a real value type here, so the argument can be anything --
/// a parameter, a field, the result of a call. Only these two spellings
/// are readings of the source rather than guesses, and anything else stays
/// unresolved.
fn statically_performed_selector(node: Node, src: &str) -> Option<String> {
    let parts = crate::emit::parse_message(node, src);
    if !matches!(
        parts.selector.as_str(),
        "performSelector:" | "performSelector:withObject:" | "performSelector:withObject:withObject:"
    ) {
        return None;
    }
    selector_value_of(*parts.args.first()?, src)
}

/// The selector literal `node` evaluates to, or None.
fn selector_value_of(node: Node, src: &str) -> Option<String> {
    if node.kind() == "parenthesized_expression" {
        let mut cursor = node.walk();
        let inner = node.children(&mut cursor).find(|c| c.kind() != "(" && c.kind() != ")")?;
        return selector_value_of(inner, src);
    }
    if node.kind() == "selector_expression" {
        return crate::collect::selector_literal_name(node, src);
    }
    if node.kind() != "identifier" {
        return None;
    }
    // A named local, read out of its own declaration. Declared exactly
    // once, so an inner block shadowing the name cannot be mistaken for
    // the outer one, and assigned nowhere, so what the declaration says is
    // what the send performs. The initializer must be the literal itself:
    // following a chain of identifiers would have to guard against
    // `SEL s = s;`, and nothing writes that.
    let scope = enclosing_scope(node)?;
    let name = node_text(node, src);
    if declaration_count(scope, src, name) != 1 || is_reassigned(scope, src, name) {
        return None;
    }
    let init = declared_initializer(scope, src, name)?;
    if init.kind() != "selector_expression" {
        return None;
    }
    crate::collect::selector_literal_name(init, src)
}

/// The `method_definition` or `function_definition` enclosing `node`.
fn enclosing_scope<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut scope = node.parent();
    while let Some(n) = scope {
        if matches!(n.kind(), "method_definition" | "function_definition") {
            return Some(n);
        }
        scope = n.parent();
    }
    None
}

/// How many declarations or parameters inside `node` introduce `name`.
fn declaration_count(node: Node, src: &str, name: &str) -> usize {
    if matches!(node.kind(), "declaration" | "parameter_declaration")
        && declares_name(node, name, src)
    {
        return 1;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().map(|child| declaration_count(child, src, name)).sum()
}

/// The receiver's class (when statically known) and the selector of a
/// message send, for looking the send up in `OwningMethods`.
fn message_target(node: Node, src: &str, program: &Program) -> (Option<String>, String) {
    let mut cursor = node.walk();
    let children: Vec<Node> =
        node.children(&mut cursor).filter(|c| c.kind() != "[" && c.kind() != "]").collect();
    if children.is_empty() {
        return (None, String::new());
    }
    let selector = selector_of(&children, src);
    let receiver_text = &src[children[0].byte_range()];
    // A class-name receiver is a class-method send; a nested message send is
    // resolved through its own selector's declaring class. Anything else
    // (a variable) is left unresolved: the analysis stays conservative.
    if program.is_class(receiver_text) {
        return (Some(receiver_text.to_string()), selector);
    }
    if children[0].kind() == "message_expression" {
        let (inner_class, _) = message_target(children[0], src, program);
        return (inner_class, selector);
    }
    // A variable receiver, resolved from its *declaration* rather than
    // guessed. Left unresolved, an owning instance method called on a
    // variable hands back +1 that nothing releases: `[a sub:b]` in
    // `foundation/q31_basic` leaked an OZQ31 on every call, because
    // `OZQ31 *a` is not a class name and the send therefore looked
    // borrowed however owning `-sub:` was known to be. Found by running the
    // corpus under LeakSanitizer through this backend for the first time.
    //
    // Both forms below are exact readings of the source, not inferences,
    // which matters more here than usual: the standing bias is that an
    // unrecognised shape must *leak* rather than double-free, so widening
    // what counts as owning is the dangerous direction. `self` is the class
    // whose `@implementation` encloses the send; a named local or parameter
    // is whatever its declaration says. Neither can be wrong about the
    // receiver's static type.
    if receiver_text == "self" {
        if let Some(class) = enclosing_impl_class(node, src) {
            return (Some(class), selector);
        }
    }
    if children[0].kind() == "identifier" {
        if let Some(class) = declared_class_of(receiver_text, node, src, program) {
            return (Some(class), selector);
        }
    }
    (None, selector)
}

/// The class whose `@implementation` encloses `node`, for a `self` receiver.
fn enclosing_impl_class(node: Node, src: &str) -> Option<String> {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if n.kind() == "class_implementation" {
            let (name, _, _) = crate::collect::class_header(n, src);
            if !name.is_empty() {
                return Some(name);
            }
            return None;
        }
        cur = n.parent();
    }
    None
}

/// The class a named receiver was declared as, searching the enclosing
/// method or function: its parameter list first, then the body's
/// declarations. Returns None unless the name is declared exactly once with
/// a type that is a known class, so an ambiguous or unknown spelling stays
/// unresolved rather than being assumed.
fn declared_class_of(
    name: &str,
    node: Node,
    src: &str,
    program: &Program,
) -> Option<String> {
    let mut scope = node.parent();
    while let Some(n) = scope {
        if matches!(n.kind(), "method_definition" | "function_definition") {
            break;
        }
        scope = n.parent();
    }
    let scope = scope?;

    let mut found: Option<String> = None;
    let mut count = 0usize;
    collect_declared_types(scope, name, src, program, &mut found, &mut count);
    if count == 1 {
        found
    } else {
        None
    }
}

/// Walk `node` for declarations and parameters naming `name`, recording the
/// class each says it has and how many such declarations were seen.
fn collect_declared_types(
    node: Node,
    name: &str,
    src: &str,
    program: &Program,
    found: &mut Option<String>,
    count: &mut usize,
) {
    if matches!(node.kind(), "declaration" | "parameter_declaration") && declares_name(node, name, src)
    {
        let (ty, _) = crate::collect::extract_type_and_stars(node, src);
        let bare = ty.trim().trim_start_matches("struct ").trim();
        if program.is_class(bare) {
            *found = Some(bare.to_string());
        }
        *count += 1;
        return;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children {
        collect_declared_types(child, name, src, program, found, count);
    }
}

/// Does this declaration or parameter introduce `name`?
fn declares_name(node: Node, name: &str, src: &str) -> bool {
    fn any_identifier(node: Node, name: &str, src: &str) -> bool {
        if node.kind() == "identifier" && &src[node.byte_range()] == name {
            return true;
        }
        // A declarator's own name only: do not descend into an initialiser,
        // where the same identifier may merely be *read*.
        if node.kind() == "init_declarator" {
            let mut c = node.walk();
            let kids: Vec<Node> = node.children(&mut c).collect();
            return kids.first().is_some_and(|d| any_identifier(*d, name, src));
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        children.into_iter().any(|c| any_identifier(c, name, src))
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().any(|c| {
        matches!(
            c.kind(),
            "init_declarator" | "pointer_declarator" | "identifier" | "function_declarator"
        ) && any_identifier(c, name, src)
    })
}

fn selector_of(children: &[Node], src: &str) -> String {
    if children.len() == 2 {
        return src[children[1].byte_range()].to_string();
    }
    let mut selector = String::new();
    let mut i = 1;
    while i < children.len() {
        if children[i].kind() == "identifier"
            && children.get(i + 1).map(|n| n.kind()) == Some(":")
        {
            selector.push_str(&src[children[i].byte_range()]);
            selector.push(':');
            i += 2;
        } else {
            i += 1;
        }
    }
    selector
}
