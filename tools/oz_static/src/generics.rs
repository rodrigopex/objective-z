// SPDX-License-Identifier: Apache-2.0
//
// generics.rs - `id<Protocol>` and `Container<Arg, ...>` constraint
// checking. Parity item for tools/oz_transpile/resolve.py's
// `_validate_generic_types`/`_satisfies_constraint`/`_class_conforms_to`
// (the Python oracle), which oz_static previously had no counterpart
// for at all -- its OZArray/OZDictionary test fixtures cut the real
// header's `<__covariant ObjectType>` generic parameter outright rather
// than risk it (see `tests/common/mod.rs`'s doc comments before this
// change).
//
// Runs as its own pass over the parse tree, called from `lib.rs` right
// after `collect::collect` succeeds, rather than folding into that
// function -- it needs nothing from collection beyond the finished
// `Program` (class/protocol tables), and a separate re-parse keeps this
// file decoupled from collect()'s own internals. The tree-sitter parse
// is cheap enough that re-parsing once more here is not worth avoiding
// at the cost of coupling.
//
// Deliberately narrower than the oracle's own scope, which itself is
// already partial (Clang erases generics from `qualType`, so it recovers
// them via a *second* tree-sitter pass, `collect.py::extract_source_generics`
// -- a hack oz_static doesn't need, since it parses with tree-sitter
// natively and the generic argument is already sitting in the CST).
// A constrained value's concrete class is resolved only for the two
// shapes real source actually uses:
//
//   - a message send whose receiver is a literal class name (the
//     alloc/factory idiom: `[ClassName alloc]`, `[ClassName foo]`), and
//   - a bare identifier already known, from an earlier plain-typed
//     declaration in the same method body, to hold one of those.
//
// An element/value whose class can't be resolved this way (an arbitrary
// expression, a message send through an `id`-typed receiver, a value
// coming from outside the method body...) is left unchecked rather than
// misreported -- silence on the unresolvable, never a false positive.
// This mirrors the oracle's own `if not elem_type or elem_type == "id":
// continue` in `_validate_array_generics`/`_validate_dict_generics`.
//
// Also out of scope, matching the oracle's own boundary in
// `_walk_generic_validation` (it only checks `VarDecl` and top-level `=`
// assignment): ivars, method parameters (their constraint is caught only
// as a plain-class-type registration for later local resolution, never
// itself checked against a caller), and returned values.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::model::{Diagnostic, Program};
use crate::parse::line_col;

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.start_byte()..node.end_byte()]
}

/// One constrained slot's requirement, extracted from a declared type
/// like `id<Frobbable>` or `OZArray<OZString *>`.
#[derive(Clone, Debug)]
enum Constraint {
    Protocol(String),
    Class(String),
}

impl Constraint {
    /// Does `concrete_class` satisfy this constraint? Mirrors the
    /// oracle's `_satisfies_constraint`: a protocol constraint checks
    /// conformance (including inherited protocols, via
    /// `Program::class_conforms_to`); a class constraint checks
    /// same-or-subclass (via `Program::is_descendant_of`).
    fn satisfied_by(&self, concrete_class: &str, program: &Program) -> bool {
        match self {
            Constraint::Protocol(proto) => program.class_conforms_to(concrete_class, proto),
            Constraint::Class(base) => {
                concrete_class == base || program.is_descendant_of(concrete_class, base)
            }
        }
    }

    fn describe(&self) -> String {
        match self {
            Constraint::Protocol(p) => format!("id<{}>", p),
            Constraint::Class(c) => c.clone(),
        }
    }
}

/// Parses one `generic_specifier` *argument* node into the constraint it
/// names: `id<Proto>` -> `Protocol(Proto)`; `id` alone -> `None`
/// (unconstrained, same as the oracle treating a bare `id` type argument
/// as satisfying anything); `ClassName [*]` -> `Class(ClassName)`;
/// `ClassName<...> [*]` (a nested generic argument) -> `Class(ClassName)`
/// too, the oracle's own `re.sub(r"<.*>$", "", ...)` -- nested
/// element-type validation isn't attempted, only that the outer class
/// matches.
///
/// Only valid for classifying a generic argument, never a whole
/// declared type on its own: a plain `OZArray *arr` is not itself a
/// constraint on what gets assigned to `arr` (it's just `arr`'s type),
/// so `classify_declared_type` must not call this on a bare, non-generic
/// type node -- see the comment there.
fn parse_constraint(type_node: Node, src: &str) -> Option<Constraint> {
    // `id<Proto>` parses as `typedefed_specifier` wrapping `id` plus a
    // `protocol_reference_list` -- see the probe in this change's
    // description, or `tools/oz_static/tests/type_constraints.rs`'s
    // header comment for the confirmed shape.
    if let Some(list) = find_protocol_reference_list(type_node) {
        let mut cursor = list.walk();
        let proto = list.children(&mut cursor).find(|c| c.kind() == "identifier")?;
        return Some(Constraint::Protocol(node_text(proto, src).to_string()));
    }
    let base = find_first_of_kinds(type_node, &["type_identifier"])?;
    let name = node_text(base, src);
    if name == "id" || name == "instancetype" {
        return None;
    }
    Some(Constraint::Class(name.to_string()))
}

fn find_protocol_reference_list(node: Node) -> Option<Node> {
    if node.kind() == "protocol_reference_list" {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_protocol_reference_list(child) {
            return Some(found);
        }
    }
    None
}

fn find_first_of_kinds<'a>(node: Node<'a>, kinds: &[&str]) -> Option<Node<'a>> {
    if kinds.contains(&node.kind()) {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_first_of_kinds(child, kinds) {
            return Some(found);
        }
    }
    None
}

/// What a declared type means for this pass: a constrained slot to
/// check assignments against, or (if it's a plain, unconstrained class
/// type) just a name worth remembering so a *later* bare-identifier
/// reference can resolve back to a concrete class.
enum DeclaredType {
    Constrained(Vec<Constraint>),
    PlainClass(String),
    Other,
}

/// Classifies a declaration's type node. `generic_specifier`
/// (`Container<Arg, ...>`) yields one `Constraint` per angle-bracket
/// argument, in order -- one for `OZArray<T>`, two (key, value) for
/// `OZDictionary<K, V>`. A bare `id<Proto>` yields exactly one. Anything
/// else that names a known class is `PlainClass` (no constraint, but
/// worth tracking); everything else (`id`, a primitive, an unknown type)
/// is `Other` and never touched again.
fn classify_declared_type(type_node: Node, src: &str, program: &Program) -> DeclaredType {
    if type_node.kind() == "generic_specifier" {
        let mut cursor = type_node.walk();
        let args: Vec<Node> = type_node
            .children(&mut cursor)
            .filter(|c| !matches!(c.kind(), "type_identifier" | "<" | ">" | ","))
            .collect();
        let constraints: Vec<Constraint> =
            args.iter().filter_map(|a| parse_constraint(*a, src)).collect();
        if !constraints.is_empty() {
            return DeclaredType::Constrained(constraints);
        }
        // Every argument was itself unconstrained id/instancetype.
        return DeclaredType::Other;
    }
    // A bare `id<Proto>` declared type is itself a constrained slot.
    // Unlike a `generic_specifier` argument, a plain class type here
    // (`OZArray *arr = ...;`, with no `<...>`) is NOT a constraint on
    // whatever gets assigned -- it is just `arr`'s own type -- so this
    // must not fall through to `parse_constraint`'s class-name branch,
    // which exists only for classifying a *generic argument*.
    if let Some(list) = find_protocol_reference_list(type_node) {
        let mut cursor = list.walk();
        let proto = list.children(&mut cursor).find(|c| c.kind() == "identifier");
        if let Some(proto) = proto {
            return DeclaredType::Constrained(vec![Constraint::Protocol(
                node_text(proto, src).to_string(),
            )]);
        }
    }
    if let Some(base) = find_first_of_kinds(type_node, &["type_identifier"]) {
        let name = node_text(base, src);
        if program.is_class(name) {
            return DeclaredType::PlainClass(name.to_string());
        }
    }
    DeclaredType::Other
}

/// One constrained name in scope, remembered so a later plain assignment
/// (`x = ...;`, not just the initializer) is checked too.
struct Constrained {
    constraints: Vec<Constraint>,
    /// The declared container/type spelling, for the diagnostic message
    /// (`"required by 'OZArray<OZQ31 *>'"`, matching the oracle's own
    /// message shape in `_validate_array_generics`).
    declared_spelling: String,
}

struct MethodScope {
    /// name -> concrete class, for a plain (unconstrained) declared type.
    plain: HashMap<String, String>,
    /// name -> its constraint(s) + declared spelling.
    constrained: HashMap<String, Constrained>,
}

pub fn check_program(source: &str, program: &Program) -> Vec<Diagnostic> {
    let tree = crate::parse::parse(source);
    let mut diags = Vec::new();
    walk_for_method_bodies(tree.root_node(), source, program, &mut diags);
    walk_for_owned_array_ivars(tree.root_node(), source, program, &mut diags);
    diags
}

/// Reject an owned array of objects with more than one dimension.
///
/// A one-dimensional one is released element by element, with the count from
/// `sizeof(a) / sizeof(a[0])` (see `companion::render_release_ivars`). At two
/// dimensions that expression counts *rows*, and `a[i]` is a sub-array rather
/// than an object -- so the release would cast array storage to an object
/// pointer and read a refcount out of it. That is the corruption #287's fix
/// removed at one dimension, and it returns unchanged at two or more.
///
/// Flattening the release with a cast to `Element **` would work on every
/// real target and is still the wrong answer: reaching across a
/// multi-dimensional array through a pointer to its first element is not
/// something ISO C defines, and "no undefined behaviour in emitted C" is a
/// standing requirement rather than a preference.
///
/// So it is a located error, which leaves the author the shape that does
/// work: one dimension, indexed arithmetically.
///
/// Scalar arrays are unaffected at any dimensionality -- they own nothing,
/// and `int _v[2][3][4][5]` transpiles and indexes correctly.
fn walk_for_owned_array_ivars(
    node: Node,
    src: &str,
    program: &Program,
    diags: &mut Vec<Diagnostic>,
) {
    if node.kind() == "class_interface" || node.kind() == "class_implementation" {
        let (class_name, _, category) = crate::collect::class_header(node, src);
        if category.is_none() {
            let owned = program.owned_object_ivar_names(&class_name);
            for (ivar, extent) in collect_declared_extents(node, src) {
                if extent.matches('[').count() < 2 {
                    continue;
                }
                if !owned.iter().any(|n| *n == ivar) {
                    continue;
                }
                let (line, col) = crate::parse::line_col(src, node.start_byte());
                diags.push(Diagnostic::new(
                    format!(
                        "'{ivar}' is an owned array of objects with more than one dimension \
                         ('{extent}'), which this backend cannot release: the elements are \
                         released one by one, and at two or more dimensions '{ivar}[i]' is a \
                         sub-array rather than an object. Declare it with a single dimension \
                         and index it arithmetically, or '__unsafe_unretained' if {class} does \
                         not own the elements.",
                        ivar = ivar,
                        extent = extent,
                        class = class_name
                    ),
                    line,
                    col,
                ));
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_owned_array_ivars(child, src, program, diags);
    }
}

/// `(ivar name, extent text)` for every array ivar declared directly on
/// `node`'s `instance_variables` block.
fn collect_declared_extents(node: Node, src: &str) -> Vec<(String, String)> {
    let known = std::collections::HashSet::new();
    let (_, _, extents) = crate::collect::extract_ivars_with_ownership(node, src, &known);
    extents.into_iter().collect()
}

fn walk_for_method_bodies(node: Node, src: &str, program: &Program, diags: &mut Vec<Diagnostic>) {
    if node.kind() == "method_definition" {
        let mut cursor = node.walk();
        if let Some(body) = node.children(&mut cursor).find(|c| c.kind() == "compound_statement") {
            let mut scope = MethodScope { plain: HashMap::new(), constrained: HashMap::new() };
            walk_statements(body, src, program, &mut scope, diags);
        }
        return; // a method body's own nested blocks are walked from here.
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_method_bodies(child, src, program, diags);
    }
}

/// Walks every statement a method body can contain, including nested
/// blocks (`if`/`for`/`while`/`{ }`), tracking `scope` as it goes -- a
/// single linear pass mirrors the oracle's own `_walk_generic_validation`
/// closely enough for the two statement shapes this pass understands
/// (`declaration`, and a top-level `=` assignment) without needing a
/// real nested-scope stack: a name declared in an inner block simply
/// overwrites/adds to the same map, which is only wrong for a shadowing
/// re-declaration in a sibling block -- not a shape any test in this
/// suite (or the oracle's) exercises.
fn walk_statements(node: Node, src: &str, program: &Program, scope: &mut MethodScope, diags: &mut Vec<Diagnostic>) {
    match node.kind() {
        "declaration" => {
            check_declaration(node, src, program, scope, diags);
            return;
        }
        "expression_statement" => {
            let mut cursor = node.walk();
            if let Some(assign) =
                node.children(&mut cursor).find(|c| c.kind() == "assignment_expression")
            {
                check_assignment(assign, src, program, scope, diags);
            }
            return;
        }
        // A block_literal is its own scope, with no access to the
        // enclosing method's locals in the first place (this backend
        // only accepts non-capturing blocks) -- nothing here applies
        // inside one.
        "block_literal" => return,
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_statements(child, src, program, scope, diags);
    }
}

/// `declaration` shape: a type node followed by one or more comma-separated
/// declarators, each of
///
/// - `init_declarator` (`[pointer_declarator] identifier = expr`),
/// - `pointer_declarator` (`* identifier`) -- a *pointer* with no initializer,
/// - a bare `identifier` -- a non-pointer with no initializer.
///
/// The third kind is why the second was missed for so long: this comment used
/// to claim an uninitialized declarator was always a bare `identifier`, which
/// is true only when it has no `*`. `OZArray<Widget *> *a;` produces a
/// `pointer_declarator` and no `init_declarator` anywhere, so it was filtered
/// out below and its constraint went unchecked — silently, since nothing was
/// emitted to complain about. Verified against a CST dump, and pinned by
/// `tests/bare_declarator_checks.rs`.
fn check_declaration(
    node: Node,
    src: &str,
    program: &Program,
    scope: &mut MethodScope,
    diags: &mut Vec<Diagnostic>,
) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let Some(type_node) = children
        .iter()
        .find(|c| !matches!(c.kind(), "init_declarator" | "identifier" | ";" | ","))
        .copied()
    else {
        return;
    };
    // Only a node that starts at or before every declarator is really
    // "the type" -- guards against picking up a declarator's own
    // internal identifier by accident when the type itself is a bare
    // `identifier` (a typedef'd class name with no pointer stars, e.g.
    // `OZObject x = ...;` -- not realistic ObjC, but cheap to guard).
    let declared = classify_declared_type(type_node, src, program);

    for decl in children
        .iter()
        .filter(|c| matches!(c.kind(), "init_declarator" | "identifier" | "pointer_declarator"))
    {
        if decl.start_byte() <= type_node.start_byte() {
            continue;
        }
        let (name_node, init) = match decl.kind() {
            // A `pointer_declarator` is a declaration with no initializer, so
            // it contributes a name to check the *declared* type of and no
            // value to check against it -- handled by the `_` arm below,
            // which digs out the identifier. Listing it was the whole fix:
            // without it a bare `OZArray<Widget *> *a;` was skipped entirely
            // and its later assignment went unchecked, silently, while the
            // identical code written with an initializer was rejected.
            "init_declarator" => {
                let mut c = decl.walk();
                let name = decl
                    .children(&mut c)
                    .find(|n| n.kind() == "identifier" || n.kind() == "pointer_declarator")
                    .map(|n| find_first_of_kinds(n, &["identifier"]).unwrap_or(n));
                let mut c2 = decl.walk();
                let last: Vec<Node> = decl.children(&mut c2).collect();
                let init = last
                    .into_iter()
                    .last()
                    .filter(|n| !matches!(n.kind(), "identifier" | "=" | "pointer_declarator"));
                (name, init)
            }
            // A bare `identifier` declarator is already the name; a
            // `pointer_declarator` wraps it (`*a`), so take the identifier
            // inside rather than the declarator's own text -- otherwise the
            // name would come out as `*a` and never match anything.
            "pointer_declarator" => {
                (find_first_of_kinds(*decl, &["identifier"]).or(Some(*decl)), None)
            }
            _ => (Some(*decl), None),
        };
        let Some(name_node) = name_node else { continue };
        let name = node_text(name_node, src).to_string();

        match &declared {
            DeclaredType::PlainClass(class) => {
                scope.plain.insert(name, class.clone());
            }
            DeclaredType::Constrained(constraints) => {
                scope.constrained.insert(
                    name,
                    Constrained {
                        constraints: constraints.clone(),
                        declared_spelling: node_text(type_node, src).to_string(),
                    },
                );
                if let Some(init) = init {
                    check_value_against_constraints(init, src, program, scope, constraints, &node_text(type_node, src).to_string(), diags);
                }
            }
            DeclaredType::Other => {}
        }
    }
}

fn check_assignment(
    node: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
    diags: &mut Vec<Diagnostic>,
) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let Some(op_pos) = children.iter().position(|c| c.kind() == "=") else { return };
    let (Some(lhs), Some(rhs)) = (children.get(op_pos.wrapping_sub(1)), children.get(op_pos + 1))
    else {
        return;
    };
    if lhs.kind() != "identifier" {
        return;
    }
    let name = node_text(*lhs, src);
    if let Some(constrained) = scope.constrained.get(name) {
        check_value_against_constraints(
            *rhs, src, program, scope, &constrained.constraints, &constrained.declared_spelling, diags,
        );
    }
}

/// Checks `value` -- either a whole initializer/RHS for an `id<Proto>`
/// slot, or an `array_literal`/`dictionary_literal` for a
/// `Container<Arg, ...>` slot -- against `constraints`, reporting a
/// hard error per violation (matching the oracle's message shape:
/// `"generic type mismatch: 'X' does not satisfy constraint 'Y' \
/// (required by 'container<Y>')"`).
fn check_value_against_constraints(
    value: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
    constraints: &[Constraint],
    declared_spelling: &str,
    diags: &mut Vec<Diagnostic>,
) {
    match value.kind() {
        "array_literal" if constraints.len() == 1 => {
            for elem in literal_elements(value) {
                check_one(elem, src, program, scope, &constraints[0], declared_spelling, None, diags);
            }
        }
        "dictionary_literal" if constraints.len() == 2 => {
            for (key, val) in dictionary_pairs(value) {
                check_one(key, src, program, scope, &constraints[0], declared_spelling, Some("key"), diags);
                check_one(val, src, program, scope, &constraints[1], declared_spelling, Some("value"), diags);
            }
        }
        _ if constraints.len() == 1 => {
            check_one(value, src, program, scope, &constraints[0], declared_spelling, None, diags);
        }
        _ => {}
    }
}

fn check_one(
    value: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
    constraint: &Constraint,
    declared_spelling: &str,
    role: Option<&str>,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(concrete) = resolve_concrete_class(value, src, program, scope) else { return };
    if constraint.satisfied_by(&concrete, program) {
        return;
    }
    let (line, col) = line_col(src, value.start_byte());
    let role = role.map(|r| format!("{} ", r)).unwrap_or_default();
    diags.push(Diagnostic::new(
        format!(
            "generic type mismatch: {}'{}' does not satisfy constraint '{}' (required by '{}')",
            role,
            concrete,
            constraint.describe(),
            declared_spelling
        ),
        line,
        col,
    ));
}

fn literal_elements(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).filter(|c| !matches!(c.kind(), "@" | "[" | "]" | ",")).collect()
}

fn dictionary_pairs(node: Node) -> Vec<(Node, Node)> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|c| c.kind() == "dictionary_pair")
        .filter_map(|pair| {
            let mut pc = pair.walk();
            let exprs: Vec<Node> = pair.children(&mut pc).filter(|c| c.kind() != ":").collect();
            (exprs.len() == 2).then(|| (exprs[0], exprs[1]))
        })
        .collect()
}

/// Resolves an expression's concrete class, for the two shapes this pass
/// understands -- see this module's header comment for why the scope
/// stops there deliberately.
fn resolve_concrete_class(
    node: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
) -> Option<String> {
    match node.kind() {
        "message_expression" => {
            let mut cursor = node.walk();
            let receiver = node.children(&mut cursor).find(|c| !matches!(c.kind(), "[" | "]"))?;
            if receiver.kind() != "identifier" {
                return None;
            }
            let name = node_text(receiver, src);
            program.is_class(name).then(|| name.to_string())
        }
        "identifier" => {
            let name = node_text(node, src);
            scope.plain.get(name).cloned()
        }
        "parenthesized_expression" => {
            let mut cursor = node.walk();
            let inner = node.children(&mut cursor).find(|c| !matches!(c.kind(), "(" | ")"))?;
            resolve_concrete_class(inner, src, program, scope)
        }
        // `@"..."`/`@42` etc: boxed literals resolvable without any
        // scope lookup at all, since they always desugar to a fixed
        // Foundation class (see `emit::render_boxed_string_literal`/
        // `render_boxed_at_expression`) -- only when that class actually
        // exists in this program, matching every other boxed-literal
        // check in this codebase (`ctx.program.is_class(...)`).
        "string_literal" => {
            let mut cursor = node.walk();
            let boxed = node.children(&mut cursor).any(|c| c.kind() == "@");
            (boxed && program.is_class("OZString")).then(|| "OZString".to_string())
        }
        "at_expression" => program.is_class("OZQ31").then(|| "OZQ31".to_string()),
        _ => None,
    }
}
