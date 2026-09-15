// SPDX-License-Identifier: Apache-2.0
//
// generics.rs - `id<Protocol>` and `Container<Arg, ...>` constraint
// checking, plus the for-in element-type check (#505).
//
// The for-in check lives here rather than in `emit.rs`, and that is a
// consequence of where the information is: `model::Program` stores no
// element or generic type at all -- `collect::render_type` reduces
// `OZArray<Owner *> *` to `struct OZArray *` -- so the emitter has
// nothing to consult even in principle. This pass already walks bodies
// with a scope, already resolves a literal element's concrete class
// (`resolve_concrete_class`), and already reports through `Diagnostic`,
// so checking the header here needs no new carrier threaded through the
// pipeline. The header itself is read through `emit::forin_binding` --
// the same reader the emitter and `arc` use, per #502's finding that two
// independent slicings of one header is how these drift apart.
//
// Parity item for tools/oz_transpile/resolve.py's
// `_validate_generic_types`/`_satisfies_constraint`/`_class_conforms_to`
// (the Python oracle), which oz2c previously had no counterpart
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
// -- a hack oz2c doesn't need, since it parses with tree-sitter
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
    // description, or `tools/oz2c/tests/type_constraints.rs`'s
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
    /// (`"required by 'OZArray<OZNumber *>'"`, matching the oracle's own
    /// message shape in `_validate_array_generics`).
    declared_spelling: String,
}

/// What a collection in scope is known to hold, element-wise, and how
/// that was established.
///
/// `spelling` is what the diagnostic quotes back at the author as the
/// evidence -- the declared type (`OZArray<Owner *>`) when they wrote
/// the element type, or the literal's own text when it was read off the
/// construction.
///
/// **Reading it off a literal is sound rather than heuristic, and that
/// is a property of the SDK rather than of this pass.**
/// `OZArray`/`OZDictionary` are immutable: they expose no `addObject:`,
/// `insertObject:`, `removeObject:` or `setObject:`, and there is no
/// mutable subclass of either (`OZMutableString` is not a collection).
/// So once `@[...]` has built a collection, nothing can put a different
/// class into it, and the element type observed at construction holds
/// for the collection's whole life. If a mutable collection is ever
/// added, this inference stops being sound and `Evidence::Literal` has
/// to go -- `Evidence::Declared` would survive, since a generic
/// argument constrains every later store too (#505).
struct ElementClass {
    class: String,
    spelling: String,
    evidence: Evidence,
}

enum Evidence {
    /// The author wrote the element type as a generic argument.
    Declared,
    /// Read off a homogeneous array literal at construction.
    Literal,
}

struct MethodScope {
    /// name -> concrete class, for a plain (unconstrained) declared type.
    plain: HashMap<String, String>,
    /// name -> its constraint(s) + declared spelling.
    constrained: HashMap<String, Constrained>,
    /// name -> what this collection holds, for the for-in header check.
    element: HashMap<String, ElementClass>,
}

pub fn check_program(source: &str, program: &Program) -> Vec<Diagnostic> {
    let tree = crate::parse::parse(source);
    let mut diags = Vec::new();
    walk_for_method_bodies(tree.root_node(), source, program, &mut diags);
    walk_for_owned_array_ivars(tree.root_node(), source, program, &mut diags);
    diags.extend(check_dispatch_signature_agreement(tree.root_node(), source, program));
    diags
}

/// Reject two classes implementing one dynamically-dispatched selector with
/// signatures that disagree.
///
/// `companion::render_protocol_dispatch` emits one `OZ_PROTOCOL_SEND_<sel>`
/// per selector *name* and takes its signature from whichever implementor was
/// declared first -- its own doc states the assumption, that "every
/// implementor of a given selector is expected to match it", and nothing
/// checked it. Two classes sharing a name but not a return type produced a
/// shim whose `case` arms disagreed with its own return type, and the only
/// complaint came from GCC, about generated code the author never wrote
/// (#290).
///
/// `instancetype` is exempt, and has to be: every implementor's
/// `return_type` is its own class, and the shim already collapses them to
/// `void *` for exactly that reason. Comparing the resolved types there
/// would reject `-init`.
///
/// The error is located on the *second* declaration, which is the one that
/// introduced the disagreement, and it names both types so the reader does
/// not have to go looking for the first. That declaration is a method
/// (`locate_method`) or a `@property` (`locate_property_accessor`) -- both,
/// because the collision this was filed for is property-declared.
fn check_dispatch_signature_agreement(
    root: Node,
    src: &str,
    program: &Program,
) -> Vec<Diagnostic> {
    use std::collections::HashMap;

    /* First declaration wins, matching what the shim actually takes its
     * signature from -- so the diagnostic describes the emitted code. */
    let mut first: HashMap<(String, bool), (String, String)> = HashMap::new();
    let mut conflicts: Vec<(String, String, String, String, String)> = Vec::new();

    for class_name in &program.class_order {
        let Some(info) = program.classes.get(class_name) else {
            continue;
        };
        for m in &info.methods {
            if !program.is_dynamically_dispatched(&m.selector, m.is_class_method) {
                continue;
            }
            if !program.method_is_defined(class_name, &m.selector, m.is_class_method) {
                continue;
            }
            /* Covariant by design; the shim returns `void *`. */
            if m.returns_instancetype {
                continue;
            }
            let key = (m.selector.clone(), m.is_class_method);
            let signature = m.return_type.trim().to_string();
            match first.get(&key) {
                None => {
                    first.insert(key, (class_name.clone(), signature));
                }
                Some((first_class, first_sig)) if returns_are_incompatible(first_sig, &signature) => {
                    conflicts.push((
                        m.selector.clone(),
                        first_class.clone(),
                        first_sig.clone(),
                        class_name.clone(),
                        signature,
                    ));
                }
                Some(_) => {}
            }
        }
    }

    conflicts
        .into_iter()
        .map(|(selector, first_class, first_sig, second_class, second_sig)| {
            let offset = locate_method(root, src, &second_class, &selector)
                .or_else(|| locate_property_accessor(program, &second_class, &selector));
            /* Name the `@property` when one is what introduced the
             * selector. The rule below is about dynamic dispatch and says
             * nothing about properties, so an author who wrote
             * `@property (nonatomic) int count;` got a paragraph about
             * `OZ_PROTOCOL_SEND_count` with no thread back to the line
             * they wrote -- and `count` and `length` are names the SDK
             * owns and an author reaches for constantly (#498). */
            let from_property = [(&second_class, &second_sig), (&first_class, &first_sig)]
                .iter()
                .find_map(|(class, _)| {
                    property_behind_selector(program, class, &selector)
                        .map(|prop| (prop.name.clone(), (*class).clone()))
                });
            let property_note = match &from_property {
                Some((prop, class)) => format!(
                    " '{selector}' on {class} is the accessor of '@property {prop}', not a \
                     method written by hand -- so renaming the property is what renames the \
                     selector.",
                    selector = selector,
                    class = class,
                    prop = prop
                ),
                None => String::new(),
            };
            Diagnostic::maybe_at(
                format!(
                    "'{selector}' is dispatched dynamically, so one shared \
                     'OZ_PROTOCOL_SEND_{selc}' routes every implementor -- but \
                     {first_class} returns '{first_sig}' and {second_class} returns \
                     '{second_sig}'. Dispatch is keyed on the selector name alone, so \
                     the two cannot share one. Rename one of them, or give them the \
                     same return type.{property_note}",
                    selector = selector,
                    selc = crate::emit::selector_to_c(&selector),
                    first_class = first_class,
                    first_sig = first_sig,
                    second_class = second_class,
                    second_sig = second_sig,
                    property_note = property_note
                ),
                src,
                offset,
            )
        })
        .collect()
}

/// Do two implementors' return types disagree at all?
///
/// Any difference, deliberately. One shared `OZ_PROTOCOL_SEND_<sel>` can
/// declare exactly one return type, so a disagreement means it is wrong for
/// at least one implementor -- a `-Wincompatible-pointer-types` error for
/// pointers, and a silent conversion for arithmetic types.
///
/// The narrower "pointers and aggregates only" rule this replaces existed
/// because the SDK itself disagreed: `-count` was `unsigned int` on
/// `OZArray` while other classes wrote `int`. With the SDK's size APIs
/// typed `size_t`/`ptrdiff_t` that disagreement is gone, and the rule can
/// say what it means.
///
/// The alternative -- keep accepting arithmetic differences and have the
/// shim declare the type C's usual arithmetic conversions would give -- was
/// considered and rejected as not implementable here. That common type is
/// not computable from the spelling: whether `size_t` is wider than
/// `unsigned int` is target-dependent, equal on 32-bit ARM and not on a
/// 64-bit host, so ranking them textually would be a guess. Refusing is
/// honest where guessing is not.
fn returns_are_incompatible(a: &str, b: &str) -> bool {
    a.trim() != b.trim()
}

/// `(line, col)` of `class`'s `method_declaration` or `method_definition`
/// of `selector`, for a diagnostic that points at the code rather than at
/// the top of the file.
///
/// Methods only -- a selector a `@property` declares has neither node, and
/// `locate_property_accessor` answers for those.
///
/// A category counts as a declaration site. It did not, and a selector
/// declared only in `@interface Beta (Extra)` was located at `1:1` for the
/// same reason a property-declared one was (#297) -- `collect` pushes a
/// category's methods onto the class, so such a selector really does take
/// part in a collision and really does have a position. The primary
/// interface still wins when both declare it, because the walk takes the
/// first match in document order and a category cannot precede the class it
/// extends. (The `category.is_none()` guard belongs in
/// `walk_for_owned_array_ivars`, where a category genuinely cannot
/// contribute, and reached here by resemblance.)
fn locate_method(root: Node, src: &str, class: &str, selector: &str) -> Option<usize> {
    fn walk(node: Node, src: &str, class: &str, selector: &str) -> Option<usize> {
        if node.kind() == "class_interface" || node.kind() == "class_implementation" {
            let (name, _, _category) = crate::collect::class_header(node, src);
            if name == class {
                if let Some(byte) = find_selector_byte(node, src, selector) {
                    return Some(byte);
                }
            }
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        children.into_iter().find_map(|c| walk(c, src, class, selector))
    }
    fn find_selector_byte(node: Node, src: &str, selector: &str) -> Option<usize> {
        if node.kind() == "method_declaration" || node.kind() == "method_definition" {
            let known = std::collections::HashSet::new();
            let sig = crate::collect::extract_method_sig(node, src, "", &known);
            if sig.selector == selector {
                return Some(node.start_byte());
            }
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        children.into_iter().find_map(|c| find_selector_byte(c, src, selector))
    }
    walk(root, src, class, selector)
}

/// `(line, col)` of the `@property` on `class` whose accessor is `selector`.
///
/// `locate_method` finds nothing for a property-declared selector: a
/// `@property` declares its accessors without writing a
/// `method_declaration` or `method_definition`, so the CST walk has no node
/// whose selector matches and the diagnostic fell back to `1:1` -- for
/// exactly the shape that motivated the check, since the colliding `spec`
/// of #290 is property-declared (#297).
///
/// Matching on the property's spelling alone would be wrong:
/// `@property(getter=ackCount) int count;` declares the selector
/// `ackCount`, which is what `collect::extract_property` records and what
/// the shim is keyed on. The setter is resolved the same way. Its own
/// return type is `void`, so a setter can only collide with a hand-written
/// method of that name -- rarer than the getter case, and located here for
/// the same reason.
///
/// The two accessors are matched exactly as `collect::resolve_properties`
/// derives them, `readonly` included: a `readonly` property synthesizes no
/// setter, so it must not answer for one. `Program::method_is_defined` is
/// laxer on both counts on purpose -- it is asking whether *something*
/// defines the selector, where over-matching costs nothing -- but here a
/// loose match would point the reader at the wrong declaration.
///
/// The position comes from `PropertyInfo`, which recorded it at collection
/// time over the same `source` this pass parses, rather than from a second
/// CST walk that would have to re-derive the `getter=` resolution and could
/// disagree with it.
fn locate_property_accessor(program: &Program, class: &str, selector: &str) -> Option<usize> {
    property_behind_selector(program, class, selector).map(|prop| prop.decl_offset)
}

/// The `@property` on `class` whose accessor is named `selector`, if any.
///
/// `locate_property_accessor` already asked this question to place the
/// diagnostic's caret; #498 is that the *message* never said so. A property
/// called `count` collides with `OZArray`'s `-count`, and the author reads a
/// paragraph about `OZ_PROTOCOL_SEND_count` and dynamic dispatch with no
/// hint that a `@property` introduced the selector -- which is what made a
/// deterministic rule read as "`@synthesize` is unstable".
fn property_behind_selector<'a>(
    program: &'a Program,
    class: &str,
    selector: &str,
) -> Option<&'a crate::model::PropertyInfo> {
    let info = program.classes.get(class)?;
    info.properties
        .iter()
        .find(|prop| {
            if prop.getter_sel.as_deref().unwrap_or(prop.name.as_str()) == selector {
                return true;
            }
            if prop.is_readonly {
                return false;
            }
            match &prop.setter_sel {
                Some(setter) => setter == selector,
                None => crate::collect::default_setter_sel(&prop.name) == selector,
            }
        })
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
                diags.push(Diagnostic::at(
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
                    src,
                    node.start_byte(),
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

/// Walks every body this pass checks: `method_definition` for an
/// `@implementation`'s methods, and `function_definition` for a plain C
/// function.
///
/// **`function_definition` was missing, and that made the whole pass
/// inert wherever a sample keeps its code.** Only `method_definition`
/// opened a scope, so a `declaration` inside `main()` never reached
/// `walk_statements` and no generic argument written there was checked
/// against anything. Measured rather than reasoned: the identical
/// mismatched program was rejected from a method body and accepted from
/// `main()`, and all nine generic declarations in
/// `samples/transpiled_generics/src/main.m` -- the tree's only sample
/// that uses generics at all -- sit in `main()`. Adding the arm changed
/// no verdict on any of the 146 sources in `samples/`,
/// `tests/behavior/`, `tests/adapted/` and `benchmarks/`: every one of
/// those declarations is honest, so the extension is reach, not new
/// strictness (#505).
fn walk_for_method_bodies(node: Node, src: &str, program: &Program, diags: &mut Vec<Diagnostic>) {
    if matches!(node.kind(), "method_definition" | "function_definition") {
        let mut cursor = node.walk();
        if let Some(body) = node.children(&mut cursor).find(|c| c.kind() == "compound_statement") {
            let mut scope = MethodScope {
                plain: HashMap::new(),
                constrained: HashMap::new(),
                element: HashMap::new(),
            };
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
        // A for-in header is checked before its body is walked, so the
        // loop variable's own binding is never mistaken for a
        // collection. Falls through to the recursion below rather than
        // returning, because the body still has to be walked -- a
        // nested for-in lives there (`nested_forin.m`), and so does
        // every declaration the loop makes.
        "for_statement" => {
            check_forin_header(node, src, program, scope, diags);
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

        // Any element class recorded for this name under an earlier
        // declaration is stale the moment the name is re-declared, and a
        // stale entry rejects correct code rather than merely missing a
        // defect -- so it goes before the new one is considered, whether
        // or not this declaration establishes a replacement (#505).
        scope.element.remove(&name);
        let declared_element = declared_element_class(type_node, src).map(|class| ElementClass {
            class,
            spelling: node_text(type_node, src).to_string(),
            evidence: Evidence::Declared,
        });

        match &declared {
            DeclaredType::PlainClass(class) => {
                // The author wrote no element type, so read one off the
                // literal if the literal is homogeneous. Sound because
                // the collection can never be mutated -- see
                // `ElementClass`.
                if let Some(init) = init {
                    if let Some(inferred) = inferred_element_class(init, src, program, scope) {
                        scope.element.insert(
                            name.clone(),
                            ElementClass {
                                class: inferred,
                                spelling: node_text(init, src).to_string(),
                                evidence: Evidence::Literal,
                            },
                        );
                    }
                }
                scope.plain.insert(name, class.clone());
            }
            DeclaredType::Constrained(constraints) => {
                if let Some(element) = declared_element {
                    scope.element.insert(name.clone(), element);
                }
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
    scope: &mut MethodScope,
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
    let name = node_text(*lhs, src).to_string();
    if let Some(constrained) = scope.constrained.get(&name) {
        check_value_against_constraints(
            *rhs, src, program, scope, &constrained.constraints.clone(), &constrained.declared_spelling.clone(), diags,
        );
    }
    /* An element class read off a *literal* describes the object that
     * literal built, not the name -- so assigning the name something
     * else retires it. Re-read the new right-hand side, and drop the
     * entry when it establishes nothing: a stale entry here would
     * reject a correct for-in rather than merely miss a wrong one.
     *
     * An `Evidence::Declared` entry survives, because a generic
     * argument constrains every later store too and the loop above is
     * what enforces that. */
    if matches!(scope.element.get(&name), Some(e) if matches!(e.evidence, Evidence::Declared)) {
        return;
    }
    match inferred_element_class(*rhs, src, program, scope) {
        Some(class) => {
            scope.element.insert(
                name,
                ElementClass {
                    class,
                    spelling: node_text(*rhs, src).to_string(),
                    evidence: Evidence::Literal,
                },
            );
        }
        None => {
            scope.element.remove(&name);
        }
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
    let role = role.map(|r| format!("{} ", r)).unwrap_or_default();
    diags.push(Diagnostic::at(
        format!(
            "generic type mismatch: {}'{}' does not satisfy constraint '{}' (required by '{}')",
            role,
            concrete,
            constraint.describe(),
            declared_spelling
        ),
        src,
        value.start_byte(),
    ));
}

/// The element class the author *wrote*, from a `Container<Arg, ...>`
/// declared type.
///
/// Takes the **first** generic argument, which is the element type for
/// `OZArray<T>` and the *key* type for `OZDictionary<K, V>` -- and a
/// for-in over a dictionary binds its keys, not its values, so the first
/// argument is right for both. That is read from `src/OZDictionary.m`'s
/// `-nextObject`, which returns `_keys[_enumerationIndex]`, rather than
/// assumed from the shape of the header.
///
/// Reads the argument nodes with the same punctuation-only filter
/// `classify_declared_type` uses and takes `first()`, deliberately
/// *not* `Constrained::constraints[0]`. That list has already dropped
/// every argument `parse_constraint` declined, so on
/// `OZDictionary<id, OZNumber *>` its element 0 is the **value** class
/// and using it would check a for-in header against the wrong half of
/// the declaration -- rejecting correct code. Pinned by
/// `forin_element_type.rs`'s `id`-keyed-dictionary control.
fn declared_element_class(type_node: Node, src: &str) -> Option<String> {
    if type_node.kind() != "generic_specifier" {
        return None;
    }
    let mut cursor = type_node.walk();
    let first = type_node
        .children(&mut cursor)
        .find(|c| !matches!(c.kind(), "type_identifier" | "<" | ">" | ","))?;
    match parse_constraint(first, src)? {
        Constraint::Class(class) => Some(class),
        Constraint::Protocol(_) => None,
    }
}

/// The element class read off an `@[...]` literal, when every element
/// resolves to the *same* class.
///
/// Conservative on both axes, because a wrong answer here rejects
/// correct code. One unresolvable element gives up on the whole literal
/// (`?` on `resolve_concrete_class`), because an element this pass
/// cannot resolve may well be of some other class; two elements that
/// resolve to different classes give up too, since the honest element
/// type is then their common ancestor and this pass does not compute
/// one. An empty literal yields nothing to read.
fn inferred_element_class(
    value: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
) -> Option<String> {
    if value.kind() != "array_literal" {
        return None;
    }
    let mut agreed: Option<String> = None;
    for elem in literal_elements(value) {
        let resolved = resolve_concrete_class(elem, src, program, scope)?;
        match &agreed {
            None => agreed = Some(resolved),
            Some(seen) if *seen == resolved => {}
            Some(_) => return None,
        }
    }
    agreed
}

/// Checks a `for (<Class> *v in <collection>)` header against what the
/// collection is known to hold (#505).
///
/// **Refuses only an *unrelated* class.** Equality is the ordinary case;
/// a header naming an ancestor is widening and correct (`for (OZObject
/// *o in arrayOfOwner)`); a header naming a descendant is a downcast
/// loop whose only other spelling is `id` plus an explicit cast, so it
/// stays accepted by decision rather than by omission. What is left --
/// two classes on different branches, `Ghost` against `Owner` -- cannot
/// be a cast of any kind, and is the shape that made the emitter call
/// `Ghost_ghostOnly` on an `Owner`.
///
/// Silent on everything it cannot resolve: an `id` header, a collection
/// that is not a bare local, a local whose element class was never
/// established. That is this module's standing rule -- silence on the
/// unresolvable, never a false positive -- and it is why this check
/// refuses nothing that was accepted before.
fn check_forin_header(
    node: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(binding) = crate::emit::forin_binding(node, src) else { return };
    let header = binding.type_text.trim();
    if !program.is_class(header) {
        return;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let Some(collection) = children.get(binding.in_pos + 1) else { return };
    if collection.kind() != "identifier" {
        return;
    }
    let Some(element) = scope.element.get(node_text(*collection, src)) else { return };
    if header == element.class
        || program.is_descendant_of(&element.class, header)
        || program.is_descendant_of(header, &element.class)
    {
        return;
    }
    let evidence = match element.evidence {
        Evidence::Declared => {
            format!("'{}' is declared '{}'", node_text(*collection, src), element.spelling)
        }
        Evidence::Literal => format!(
            "'{}' was built from '{}', every element of which is '{}'",
            node_text(*collection, src),
            element.spelling,
            element.class
        ),
    };
    diags.push(
        Diagnostic::spanning(
            format!(
                "for-in header binds '{}' as '{}', but this collection holds '{}' -- and '{}' \
                 is unrelated to '{}', neither the same class nor one of its ancestors or \
                 descendants",
                binding.var_name, header, element.class, header, element.class
            ),
            src,
            node.start_byte()..collection.end_byte(),
        )
        .with_note(format!(
            "{}, so every send to '{}' in the body would be dispatched statically to a \
             '{}' function with an object of an unrelated class as `self`",
            evidence, binding.var_name, header
        ))
        .with_help(format!("bind '{}' if the header was wrong", element.class))
        .with_help(
            "bind 'id' and cast at each send if the element class is not known here"
                .to_string(),
        ),
    );
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
        "at_expression" => program.is_class("OZNumber").then(|| "OZNumber".to_string()),
        _ => None,
    }
}
