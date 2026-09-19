// SPDX-License-Identifier: Apache-2.0
//
// collect.rs - CST -> Program symbol table (classes, ivars, method
// signatures). Two sub-passes: class names/hierarchy first, then
// ivars/methods (which need the class-name set to render object types).

use std::collections::{BTreeSet, HashMap, HashSet};

use tree_sitter::Node;

use crate::model::{
    ClassInfo, InterfaceKind, MethodSig, Ownership, Program, PropertyInfo, PropertyOrigin,
    ProtocolInfo,
};

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.start_byte()..node.end_byte()]
}

fn child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).find(|c| c.kind() == kind);
    found
}

/// class_interface / class_implementation share the shape:
/// [@interface|@implementation] identifier [: identifier]? [( identifier? )]?
///
/// The trailing parenthesised group has **three** meaningful states, not two,
/// and the returned `InterfaceKind` is the whole point of this function:
///
/// | source                    | kind                   |
/// |---------------------------|------------------------|
/// | `@interface Foo : Bar`    | `Primary`              |
/// | `@interface Foo ()`       | `Extension`            |
/// | `@interface Foo (Name)`   | `Category("Name")`     |
///
/// In tree-sitter-objc 3.0.2 the `category` field of
/// `_class_interface_inheritance` is `CHOICE[identifier, BLANK]`, so
/// `@interface Foo ()` is an ordinary `class_interface` whose only direct-child
/// `identifier` is `Foo`. The parenthesis tokens are therefore the *only*
/// evidence that an extension is one, and this function used to compute
/// exactly that fact -- as a local `saw_paren` -- and then throw it away,
/// returning `None` for the category name and leaving an extension
/// indistinguishable from a primary `@interface`. That single discarded bit is
/// the root cause of #529 and #530 both.
pub(crate) fn class_header(node: Node, src: &str) -> (String, Option<String>, InterfaceKind) {
    let mut cursor = node.walk();
    let mut idents = Vec::new();
    let mut saw_colon = false;
    let mut saw_paren = false;
    let mut superclass = None;
    let mut category = None;
    for child in node.children(&mut cursor) {
        match child.kind() {
            ":" => saw_colon = true,
            "(" => saw_paren = true,
            "identifier" => {
                idents.push(node_text(child, src).to_string());
                if saw_colon && superclass.is_none() {
                    superclass = idents.last().cloned();
                } else if saw_paren && category.is_none() {
                    category = idents.last().cloned();
                }
            }
            _ => {}
        }
    }
    let name = idents.first().cloned().unwrap_or_default();
    /* Order matters: a named category sets both `saw_paren` and `category`,
     * so the name is tested first and `saw_paren` alone is what remains to
     * mean "parenthesised, but unnamed" -- an extension. */
    let kind = match category {
        Some(cat) => InterfaceKind::Category(cat),
        None if saw_paren => InterfaceKind::Extension,
        None => InterfaceKind::Primary,
    };
    (name, superclass, kind)
}

/// The protocol names in a `<Protocol, ...>` conformance/reference list --
/// shared shape between a class's `@interface Foo : Bar <P1, P2>` (a
/// `parameterized_arguments` node) and a protocol's own
/// `@protocol Name <Super1, Super2>` (a `protocol_reference_list` node).
fn extract_protocol_list(node: Node, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(n: Node, src: &str, out: &mut Vec<String>) {
        if n.kind() == "type_identifier" || (n.kind() == "identifier" && n.child_count() == 0) {
            out.push(node_text(n, src).to_string());
            return;
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            walk(child, src, out);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "<" || child.kind() == ">" || child.kind() == "," {
            continue;
        }
        walk(child, src, &mut out);
    }
    out
}

/// A `class_interface` with both its own generic parameter list *and*
/// protocol conformance (`@interface OZArray<__covariant ObjectType> :
/// OZObject <OZIteratorProtocol>`) has TWO `parameterized_arguments`
/// children -- the class's own `<...>` right after its name, and the
/// conformance list `<...>` after the superclass. `child_by_kind` picks
/// the *first* one unconditionally, which is only ever correct when a
/// generic parameter list isn't also present -- otherwise it reads the
/// generic parameter names (`__covariant`, `ObjectType`) as if they were
/// protocol names. Confirmed via a tree-sitter-objc CST dump: both
/// lists really do share the same node kind, distinguished only by
/// position relative to the `:` token.
///
/// The conformance list, when a superclass is present, is whichever
/// `parameterized_arguments` comes *after* the `:` -- there is at most
/// one on each side. With no superclass (a root class conforming to a
/// protocol directly -- no precedent in this SDK, but syntactically
/// possible), there is at most one `parameterized_arguments` at all, so
/// it can only be the conformance list.
fn extract_conformance(node: Node, src: &str) -> Vec<String> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let list = match children.iter().position(|c| c.kind() == ":") {
        Some(colon_idx) => {
            children[colon_idx..].iter().find(|c| c.kind() == "parameterized_arguments")
        }
        None => children.iter().find(|c| c.kind() == "parameterized_arguments"),
    };
    match list {
        Some(list) => extract_protocol_list(*list, src),
        None => Vec::new(),
    }
}

/// `@protocol Name [<Super, ...>] method_declaration* @end`.
fn extract_protocol(node: Node, src: &str, known_classes: &HashSet<String>) -> ProtocolInfo {
    let mut cursor = node.walk();
    let name = node
        .children(&mut cursor)
        .find(|c| c.kind() == "identifier")
        .map(|n| node_text(n, src).to_string())
        .unwrap_or_default();
    let super_protocols = match child_by_kind(node, "protocol_reference_list") {
        Some(list) => extract_protocol_list(list, src),
        None => Vec::new(),
    };
    let mut methods = Vec::new();
    collect_protocol_methods(node, src, &name, known_classes, false, &mut methods);
    ProtocolInfo { name, super_protocols, methods, properties: Vec::new() }
}

/// `method_declaration`s directly inside a protocol body, or nested one
/// level inside a `@required`/`@optional`-qualified sub-block
/// (`qualified_protocol_interface_declaration`) -- tree-sitter-objc
/// wraps everything after such a marker in its own node, so a flat
/// direct-children scan misses them entirely.
///
/// **Required-vs-optional is tracked, and the comment here used to say it
/// was not** -- "protocols are a compile-time contract here, not a runtime
/// filter, so nothing downstream cares about the distinction". Something
/// downstream did: `emit::render_interface`'s conformance check reads this
/// list and requires *every* entry of every conformer, so a class omitting
/// an `@optional` member was refused outright (#536). That is the entire
/// purpose of the keyword, and px-app carried `WA-011` -- delete the
/// `@optional` section and redeclare the method on the implementing
/// class -- for as long as the claim stood.
///
/// The marker is **sticky**: it applies to every declaration after it
/// until a `@required` resets it. tree-sitter models that by giving each
/// marker its own node wrapping what follows, and those nodes are
/// **siblings** rather than nested -- verified on a dump, because a nested
/// shape would need the flag inherited-then-overridden instead. So
/// `optional` is read on entry to each qualified block and not passed down
/// from an enclosing one.
///
/// A `method_declaration` *directly* in the protocol body, before any
/// marker, is required -- which is the `false` the outer call starts with.
fn collect_protocol_methods(
    node: Node,
    src: &str,
    protocol_name: &str,
    known_classes: &HashSet<String>,
    optional: bool,
    out: &mut Vec<MethodSig>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "method_declaration" => {
                let mut sig = extract_method_sig(child, src, protocol_name, known_classes);
                sig.is_optional = optional;
                out.push(sig);
            }
            "qualified_protocol_interface_declaration" => {
                /* The marker is this block's own first child, so a
                 * `@required` after an `@optional` resets rather than
                 * inherits. Anything that is neither marker keeps the
                 * enclosing value, which is what an unmarked block would
                 * mean if the grammar ever produced one. */
                let marker = child.child(0).map(|c| c.kind());
                let nested = match marker {
                    Some("@optional") => true,
                    Some("@required") => false,
                    _ => optional,
                };
                collect_protocol_methods(child, src, protocol_name, known_classes, nested, out)
            }
            _ => {}
        }
    }
}

/// Every `property_declaration` inside a protocol body, paired with the
/// protocol's name.
///
/// Mirrors `collect_protocol_methods`' descent, including through
/// `qualified_protocol_interface_declaration` -- which is what `@optional`
/// and `@required` produce, so a property under either is found. That
/// function matched only `method_declaration`, which is why a protocol
/// `@property` was collected nowhere at all (#498).
fn collect_protocol_property_nodes<'a>(
    node: Node<'a>,
    protocol_name: &str,
    out: &mut Vec<(String, Node<'a>)>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "property_declaration" => out.push((protocol_name.to_string(), child)),
            "qualified_protocol_interface_declaration" => {
                collect_protocol_property_nodes(child, protocol_name, out)
            }
            _ => {}
        }
    }
}

/// The property `prop_name` names, declared by any protocol `conforms`
/// adopts -- transitively through `super_protocols`.
///
/// The walk mirrors `Program::protocol_methods`: same stack, same `visited`
/// guard against a cyclic `@protocol A <B>` / `@protocol B <A>`. Kept
/// separate rather than sharing it because this runs inside `collect`,
/// before a `Program` exists.
fn protocol_property(
    conforms: &[String],
    protocols: &HashMap<String, ProtocolInfo>,
    prop_name: &str,
) -> Option<PropertyInfo> {
    let mut stack: Vec<String> = conforms.to_vec();
    let mut visited: HashSet<String> = HashSet::new();
    while let Some(name) = stack.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let Some(info) = protocols.get(&name) else {
            continue;
        };
        if let Some(found) = info.properties.iter().find(|p| p.name == prop_name) {
            return Some(found.clone());
        }
        stack.extend(info.super_protocols.iter().cloned());
    }
    None
}

/// Is `type_text` a protocol-qualified `id` -- `id<Proto>`, `id <A, B>`?
///
/// A protocol qualification constrains what may be *assigned* to a
/// declaration and says nothing about its representation, so it lowers
/// exactly as a bare `id` does. `emit::is_bare_id_type` answers the same
/// question of a CST node; this answers it of already-flattened *text*,
/// which is what the text-driven callers of `render_type` have.
///
/// A prefix test rather than a `contains('<')` one, so a class merely
/// *named* with an `id` prefix (`identity<T>`) and a generic whose argument
/// happens to be an `id` (`OZArray<id<Proto> *>`) are both left alone.
fn is_protocol_qualified_id(type_text: &str) -> bool {
    let Some(rest) = type_text.trim().strip_prefix("id") else {
        return false;
    };
    let rest = rest.trim_start();
    rest.starts_with('<') && rest.ends_with('>')
}

pub(crate) fn render_type(type_text: &str, stars: usize, known_classes: &HashSet<String>) -> String {
    // `id<Proto>` is normalized here, and not only in the CST reader that
    // feeds this (`extract_type_and_stars_inner`'s `typedefed_specifier`
    // arm), because not every caller has a CST to read. A *text*-driven
    // one hands over whatever the author wrote: `emit::forin_binding`
    // joins the header's type nodes into a string, so
    // `for (id<PXCalibratable> d in c)` arrived as the literal text
    // `"id<PXCalibratable>"`, fell through to the verbatim return below
    // and emitted `for (id<PXCalibratable> d = ...)` -- not C at all, and
    // reported by GCC against a line the author never wrote (#531).
    //
    // Answering it in the one renderer covers every such caller at once,
    // and cannot change any *reader*'s verdict: `forin_binding`'s own
    // `type_text` is untouched, so `generics::check_forin_header` still
    // sees the author's spelling to judge against `is_class`.
    if type_text == "id" || is_protocol_qualified_id(type_text) {
        // "id" names no real C type, so left as-is this would emit
        // invalid C (as opposed to "instancetype", which callers resolve
        // to the concrete self/root class before ever reaching here).
        // `void *` is the natural stand-in: it's the untyped "any object
        // pointer" this spike has -- any `struct Foo *` converts to/from
        // it without even a cast, matching how a generic `id`-typed
        // parameter is meant to accept an instance of any class.
        return "void *".to_string();
    }
    if known_classes.contains(type_text) {
        let stars = stars.max(1);
        return format!("struct {} {}", type_text, "*".repeat(stars));
    }
    format!("{}{}", type_text, "*".repeat(stars))
}

/// Extract (type_text, star_count) from a method_type / struct_declaration's
/// declared type, e.g. "(int)" -> ("int", 0), "OZObject *foo" -> ("OZObject", 1).
pub(crate) fn extract_type_and_stars(node: Node, src: &str) -> (String, usize) {
    extract_type_and_stars_inner(node, src, false)
}

/// `extract_type_and_stars`, stopping at the declarator -- any
/// `abstract_function_declarator` or `init_declarator` subtree is ignored.
///
/// For a block's return type (#303), which sits before a declarator that
/// holds a *parameter list* an unpruned walk would read as part of the
/// type. Two positions need it:
///
///   - the literal's own `^uint32_t(int seed)`, where `type_name` chains
///     the return type into an `abstract_function_declarator`, so an
///     unpruned walk counts the parameters' stars as the return type's and
///     `^void *(char *s)` comes out `void **`;
///   - the `declaration` around `static unsigned (^sq)(int) = ...`, whose
///     `init_declarator` holds both the parameter list and the `^`.
///
/// Deliberately *not* the default, because `abstract_function_declarator`
/// means the opposite thing one position over. In a block literal the
/// return type's own star sits in an `abstract_pointer_declarator`
/// *wrapping* the function declarator, so pruning still finds it. In a cast
/// it sits *inside* it -- `(void (*)(int))` parses the `(*)` as an
/// `abstract_parenthesized_declarator` under the
/// `abstract_function_declarator` -- so pruning there would drop the
/// pointer entirely. Casting to a function-pointer type is its own
/// unsupported shape (it already renders wrong, `void **`); this changes
/// nothing about it either way.
///
/// Stars *after* the pruned declarator are the caller's problem: in
/// `static void *(^f)(int)` the `*` is inside the `init_declarator`, not
/// beside the type specifier, so `emit::declared_block_pointer_type`
/// counts that chain itself.
pub(crate) fn extract_type_and_stars_to_declarator(node: Node, src: &str) -> (String, usize) {
    extract_type_and_stars_inner(node, src, true)
}

fn extract_type_and_stars_inner(
    node: Node,
    src: &str,
    prune_declarators: bool,
) -> (String, usize) {
    let mut type_text = String::new();
    let mut stars = 0;
    let mut qualifiers: Vec<String> = Vec::new();
    let mut cursor = node.walk();
    fn walk(
        n: Node,
        src: &str,
        type_text: &mut String,
        stars: &mut usize,
        qualifiers: &mut Vec<String>,
        prune_declarators: bool,
    ) {
        if prune_declarators
            && matches!(n.kind(), "abstract_function_declarator" | "init_declarator")
        {
            return;
        }
        match n.kind() {
            // `const`/`volatile`/`restrict`, which are their own nodes and
            // were simply dropped. That made a generated signature disagree
            // with the source it came from: `- (const char *)cString` in
            // `include/oz_sdk/Foundation/OZString.h` came out as
            // `char *OZString_cString(...)`, and returning `self->_data`
            // (a `const char *` ivar) from it warns
            // "discards qualifiers" under `-Wall` -- a build failure under
            // Zephyr's `-Werror`, and a quietly wrong public type either
            // way.
            //
            // Only qualifiers seen *before* the type name are collected,
            // which is where a pointee qualifier sits (`const char *`). One
            // written after the star qualifies the pointer itself
            // (`char *const`) and belongs to the declarator, not here.
            "type_qualifier" => {
                if type_text.is_empty() {
                    let text = node_text(n, src).trim().to_string();
                    // An allowlist, because this node kind also covers
                    // Objective-C's ARC and bridging qualifiers -- `__bridge`,
                    // `__strong`, `__unsafe_unretained` and friends -- which
                    // name nothing in C and must keep being dropped.
                    // Preserving them emitted `(__bridge void *)` into a
                    // generated cast, which is not C: "use of undeclared
                    // identifier '__bridge'". Found on `src/OZTimer.m`,
                    // retired in #267 -- but `__bridge` is ordinary
                    // Objective-C that any source may use.
                    //
                    // Allowlist rather than a denylist so an unrecognised
                    // qualifier keeps the old behaviour of being dropped:
                    // losing one is at worst a weaker type, while passing an
                    // unknown word through is invalid C.
                    const C_QUALIFIERS: [&str; 6] = [
                        "const",
                        "volatile",
                        "restrict",
                        "__restrict",
                        "__restrict__",
                        "_Atomic",
                    ];
                    if C_QUALIFIERS.contains(&text.as_str()) && !qualifiers.contains(&text) {
                        qualifiers.push(text);
                    }
                }
            }
            // `sized_type_specifier` is a multi-keyword primitive type
            // (`unsigned long`, `long long`, `unsigned char`, ...) --
            // tree-sitter-objc gives it its own node kind, distinct from
            // a single-keyword `primitive_type` (`int`, `char`, ...),
            // but the whole node's own text is exactly the desired type
            // text either way.
            "primitive_type" | "sized_type_specifier" | "type_identifier" => {
                if type_text.is_empty() {
                    *type_text = node_text(n, src).to_string();
                }
            }
            "typedefed_specifier" => {
                // Usually just wraps a bare `id`/`instancetype`/typedef'd
                // name, whose own text is exactly the desired type text
                // (the common case, handled by the fallback below). But
                // `id<Proto>` -- a protocol-qualified `id` -- parses as
                // this SAME node kind wrapping `id` *plus* a
                // `protocol_reference_list` (confirmed via a tree-sitter
                // CST dump: see `generics.rs`'s header comment), so its
                // own text is `"id<Proto>"`, not a real type name --
                // `render_type` only special-cases bare `"id"`. Detect
                // that shape and normalize to plain `"id"`, so it lowers
                // to `void *` exactly like an unqualified `id` would;
                // the protocol constraint itself is a
                // `generics::check_program` concern, not codegen (no
                // runtime type/selector registry is generated -- the
                // same reason `staticbar.rs` rejects `@selector`).
                if type_text.is_empty() {
                    let mut c = n.walk();
                    let has_protocol_list =
                        n.children(&mut c).any(|ch| ch.kind() == "protocol_reference_list");
                    *type_text = if has_protocol_list {
                        "id".to_string()
                    } else {
                        node_text(n, src).to_string()
                    };
                }
            }
            "generic_specifier" => {
                // `Container<Arg, ...>` (e.g. `OZArray<OZNumber *>`): this
                // spike renders a generic collection's *declared* type
                // exactly like its non-generic form -- element-type
                // constraints are a `generics::check_program` concern,
                // not codegen, matching the oracle (Clang erases
                // generics too, so it also just emits the base class).
                // Only the base name is a real C type; the recursive
                // fallback below must not be allowed to touch the
                // bracketed argument, whose own pointer star(s) belong
                // to the *argument* type, not to this declaration's.
                if type_text.is_empty() {
                    let mut c = n.walk();
                    let base = n.children(&mut c).find(|ch| ch.kind() == "type_identifier");
                    if let Some(base) = base {
                        *type_text = node_text(base, src).to_string();
                    }
                }
            }
            "enum_specifier" => {
                // `enum Name { ... }` -- the tag name is a `type_identifier`
                // child, but the "enum" keyword itself isn't a separate
                // node, so it must be prepended explicitly or the rendered
                // C type loses the tag (`Direction` instead of
                // `enum Direction`), which doesn't name a type on its own.
                if type_text.is_empty() {
                    let mut c = n.walk();
                    let found = n.children(&mut c).find(|ch| ch.kind() == "type_identifier");
                    *type_text = match found {
                        Some(name) => format!("enum {}", node_text(name, src)),
                        // Anonymous `enum { ... }` (no tag name): nothing
                        // can name this type in the generated C, so the
                        // bare keyword is the most that can be reported.
                        // Only reachable for an *ivar*, whose declaration
                        // `emit::lower_ivar_decl` copies through with its
                        // body intact -- in a method signature the shape
                        // is rejected outright by
                        // `reject_inline_anonymous_aggregates`, since
                        // there the bare keyword would reach codegen as
                        // invalid C.
                        None => "enum".to_string(),
                    };
                }
            }
            "struct_specifier" => {
                // Same reasoning as `enum_specifier` just above, for a
                // plain `struct Name` type reference (e.g. a parameter
                // typed `struct NSFastEnumerationState *`) -- the "struct"
                // keyword isn't a separate node either, so without this,
                // the generic recursive fallback below would find just
                // the tag name's own `type_identifier` child and use it
                // bare (`NSFastEnumerationState *`), which C rejects: an
                // incomplete (forward-declared, no body) struct type has
                // no typedef, so it can only ever be spelled with the
                // `struct` keyword, not bare.
                if type_text.is_empty() {
                    let mut c = n.walk();
                    let found = n.children(&mut c).find(|ch| ch.kind() == "type_identifier");
                    *type_text = match found {
                        Some(name) => format!("struct {}", node_text(name, src)),
                        None => "struct".to_string(),
                    };
                }
            }
            // An `init_declarator` is `<declarator> = <value>`, and the
            // *value* is an expression -- never part of the declared type.
            // The generic walk below descended into it anyway and counted
            // every `*` token it found there, so the initialiser's own
            // stars were attributed to the declaration's type:
            //
            //     Foo *v = (Foo *)[Foo make];   ->  ("Foo", 2)
            //     __block int acc = 2 * 3;      ->  ("int", 1)
            //
            // The first is #491. `emit::managed_object_locals` admits an
            // object local on `stars == 1`, so a cast in the initialiser
            // made `is_object` false and the variable was never a
            // candidate -- which is why instrumenting that function's
            // decision showed it never firing on either path: the
            // exclusion is here, one frame below it. The local kept its
            // scope-exit release because `emit::owned_locals_of` reaches
            // it by a second, independent path that asks
            // `arc::declares_pointer` per *declarator* and never consults
            // this count, so only release-on-overwrite was lost -- one
            // leaked object per overwritten binding. The second line is
            // the same overcount reached through a multiply: it hoisted
            // as `static int* acc;`.
            //
            // Only the declarator half is walked, which is why
            // `extract_type_and_stars_to_declarator`'s blunt prune of the
            // whole `init_declarator` is not the answer here -- `Foo *v`'s
            // own `*` lives *inside* this node, and pruning it would give
            // `("Foo", 0)` and lose every object local instead.
            //
            // The `=` is the boundary rather than a `declarator` field
            // lookup, matching how `emit::owned_locals_of` and
            // `managed_object_locals` already split this node.
            "init_declarator" => {
                let mut c = n.walk();
                let parts: Vec<Node> = n.children(&mut c).collect();
                for child in parts.iter().take_while(|p| p.kind() != "=") {
                    walk(*child, src, type_text, stars, qualifiers, prune_declarators);
                }
            }
            "*" => *stars += 1,
            _ => {
                let mut c = n.walk();
                for child in n.children(&mut c) {
                    walk(child, src, type_text, stars, qualifiers, prune_declarators);
                }
            }
        }
    }
    for child in node.children(&mut cursor) {
        walk(child, src, &mut type_text, &mut stars, &mut qualifiers, prune_declarators);
    }
    if !qualifiers.is_empty() && !type_text.is_empty() {
        type_text = format!("{} {}", qualifiers.join(" "), type_text);
    }
    (type_text, stars)
}

pub(crate) fn extract_ivars(node: Node, src: &str, known_classes: &HashSet<String>) -> Vec<(String, String)> {
    extract_ivars_with_ownership(node, src, known_classes).0
}

/// `extract_ivars`, plus the names declared `__unsafe_unretained`, plus the
/// array extent of any ivar that has one (`"_values"` -> `"[4]"`).
///
/// The qualifier is dropped from the generated struct, meaning nothing to C
/// (see `emit::lower_ivar_decl`), so this is the only point at which it can
/// be recorded -- and it has to be. An unretained ivar is an unowned
/// backref; releasing one when its owner is deallocated is precisely the
/// double-free the qualifier exists to prevent (`OZDefer`'s `_owner` is
/// exactly that shape).
pub(crate) fn extract_ivars_with_ownership(
    node: Node,
    src: &str,
    known_classes: &HashSet<String>,
) -> (Vec<(String, String)>, HashSet<String>, HashMap<String, String>) {
    let Some(vars_node) = child_by_kind(node, "instance_variables") else {
        return (Vec::new(), HashSet::new(), HashMap::new());
    };
    let mut unretained = HashSet::new();
    let mut extents = HashMap::new();
    let mut out = Vec::new();
    let mut cursor = vars_node.walk();
    for child in vars_node.children(&mut cursor) {
        if child.kind() != "instance_variable" {
            continue;
        }
        let Some(decl) = child_by_kind(child, "struct_declaration") else {
            continue;
        };
        let (type_text, stars) = extract_type_and_stars(decl, src);
        let Some(declarator) = child_by_kind(decl, "struct_declarator")
            .or_else(|| child_by_kind(decl, "identifier"))
        else {
            continue;
        };
        // struct_declarator wraps either `identifier` or `pointer_declarator`.
        let name = find_declared_name(declarator, src);
        if qualifies(decl, declarator, src, "__unsafe_unretained") {
            unretained.insert(name.clone());
        }
        if let Some(extent) = array_extent(declarator, src) {
            extents.insert(name.clone(), extent);
        }
        out.push((name, render_type(&type_text, stars, known_classes)));
    }
    (out, unretained, extents)
}

/// The bracketed extent of an array declarator, verbatim -- `"[4]"` for
/// `int _values[4]`, `"[SLOTS]"` for `int _values[SLOTS]`, `"[]"` for an
/// unsized one. `None` when the declarator names no array.
///
/// Text rather than a parsed size: the extent only ever has to be copied
/// into a declaration, and a count would have to evaluate an arbitrary
/// constant expression that the C compiler is already going to evaluate
/// correctly.
///
/// Multi-dimensional declarators nest, so the extents concatenate on the
/// way back up -- `int _grid[2][3]` yields `"[2][3]"`.
fn array_extent(node: Node, src: &str) -> Option<String> {
    if node.kind() == "array_declarator" {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        let open = children.iter().position(|c| c.kind() == "[")?;
        let own = node_text(node, src).get(
            node_text(node, src).find('[')?..,
        )?;
        /* An inner declarator's own extent is already inside `own`, since
         * the text spans the whole nest. Nothing to concatenate. */
        let _ = open;
        return Some(own.to_string());
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().find_map(|c| array_extent(c, src))
}

/// Does `qualifier` apply to the variable `declarator` declares?
///
/// Read off the `type_qualifier` nodes, the way `emit::is_static_declaration`
/// reads the `storage_class_specifier` node -- and for the same reason,
/// stated in that function's own comment forty lines from one of the sites
/// this replaces: it "sees only the storage class and not a `static`
/// appearing anywhere else in the text". The ownership qualifier was matched
/// by `node_text(decl).contains(...)` over the whole declaration,
/// initialiser included.
///
/// What that cost is a **leak**, and it needs only one token in a cast:
///
/// ```objc
/// Foo *a = (__unsafe_unretained Foo *)[Foo make];
/// ```
///
/// `a` is `__strong` -- the cast qualifies the cast's type, not the
/// declaration -- so ARC releases it at scope exit. The substring search saw
/// the token, dropped `a` from the managed set, and emitted no release.
/// Measured against the same source with the cast's qualifier removed: the
/// control emits `oz_release`, this does not, and nothing else in the two
/// outputs differs.
///
/// **Two arguments rather than one, because C gives the two positions
/// different scopes**, and a per-declaration answer is wrong for the second:
///
/// ```objc
/// __unsafe_unretained Foo *a, *b;   /* both unretained */
/// Foo *__unsafe_unretained a, *b;   /* `a` unretained, `b` __strong */
/// ```
///
/// The substring search answered the whole declaration and so made `b`
/// unretained in both -- measured, a leak: `b` holds a `+1` from its own
/// initialiser and got no release, one object freed where ARC frees two.
/// Reading the node without also narrowing the scope would have kept that,
/// which is why "read the node instead of the text" is not on its own the
/// fix.
///
/// A qualifier among the **declaration's** own children introduces every
/// declarator, so it applies to all of them. One inside a **declarator**
/// applies to that declarator alone. And one after the `=` applies to
/// neither -- it belongs to a cast, a compound literal, or a nested
/// declaration in a block body:
///
/// ```text
/// __unsafe_unretained Foo *a;   declaration > type_qualifier
/// id __unsafe_unretained g;     declaration > type_qualifier   (after the specifier)
/// Foo *__unsafe_unretained b;   init_declarator > pointer_declarator > type_qualifier
/// Foo *d = (__unsafe_unretained Foo *)0;
///                               init_declarator > cast_expression > ... > type_qualifier
/// ```
///
/// What separates the last is not its depth but which **side of the `=`** it
/// falls on, so the declarator walk descends freely and stops there.
pub(crate) fn qualifies(decl: Node, declarator: Node, src: &str, qualifier: &str) -> bool {
    let mut cursor = decl.walk();
    let on_the_declaration = decl.children(&mut cursor).any(|child| {
        child.kind() == "type_qualifier" && node_text(child, src).trim() == qualifier
    });
    if on_the_declaration {
        return true;
    }
    fn walk(node: Node, src: &str, qualifier: &str) -> bool {
        if node.kind() == "type_qualifier" && node_text(node, src).trim() == qualifier {
            return true;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        let stop = if node.kind() == "init_declarator" {
            children.iter().position(|c| c.kind() == "=").unwrap_or(children.len())
        } else {
            children.len()
        };
        children[..stop].iter().any(|c| walk(*c, src, qualifier))
    }
    walk(declarator, src, qualifier)
}

pub(crate) fn find_declared_name(node: Node, src: &str) -> String {
    if node.kind() == "identifier" {
        return node_text(node, src).to_string();
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            return node_text(child, src).to_string();
        }
        let found = find_declared_name(child, src);
        if !found.is_empty() {
            return found;
        }
    }
    String::new()
}

/// `@property (attr, ...) Type name;` -- shape confirmed by direct
/// tree-sitter-objc S-expression dump: `property_attributes_declaration`
/// wraps zero or more `property_attribute` nodes, each either a single
/// `identifier` flag (`readonly`, `nonatomic`, `strong`, `assign`,
/// `unsafe_unretained`, `weak`, ...) or `identifier "=" identifier [":"]`
/// for `getter=name` / `setter=name:` (the trailing `:` of a setter
/// selector is its own sibling token, not part of the identifier, so the
/// value is read from the whole attribute's text minus its `key=` prefix
/// rather than from the second identifier alone). `weak` is hard-rejected
/// (mirrors `tools/oz_transpile/collect.py`'s `_collect_property`
/// exactly) -- returns `None` in that case, after pushing the diagnostic.
fn extract_property(
    node: Node,
    src: &str,
    known_classes: &HashSet<String>,
    diagnostics: &mut Vec<crate::model::Diagnostic>,
) -> Option<PropertyInfo> {
    let mut is_readonly = false;
    let mut is_nonatomic = false;
    let mut is_weak = false;
    let mut ownership = Ownership::default();
    let mut getter_sel = None;
    let mut setter_sel = None;

    if let Some(attrs) = child_by_kind(node, "property_attributes_declaration") {
        let mut cursor = attrs.walk();
        for attr in attrs.children(&mut cursor) {
            if attr.kind() != "property_attribute" {
                continue;
            }
            let mut c2 = attr.walk();
            let Some(key_node) = attr.children(&mut c2).find(|c| c.kind() == "identifier") else {
                continue;
            };
            match node_text(key_node, src) {
                "readonly" => is_readonly = true,
                "nonatomic" => is_nonatomic = true,
                "strong" | "retain" => ownership = Ownership::Strong,
                "assign" => ownership = Ownership::Assign,
                "unsafe_unretained" => ownership = Ownership::UnsafeUnretained,
                "weak" => is_weak = true,
                "getter" => {
                    getter_sel = node_text(attr, src).strip_prefix("getter=").map(String::from);
                }
                "setter" => {
                    setter_sel = node_text(attr, src).strip_prefix("setter=").map(String::from);
                }
                _ => {}
            }
        }
    }

    let decl = child_by_kind(node, "struct_declaration")?;
    let (type_text, stars) = extract_type_and_stars(decl, src);
    let is_object = type_text == "id" || known_classes.contains(&type_text);
    let c_type = render_type(&type_text, stars, known_classes);
    let declarator =
        child_by_kind(decl, "struct_declarator").or_else(|| child_by_kind(decl, "identifier"))?;
    let name = find_declared_name(declarator, src);
    let decl_offset = node.start_byte();

    if is_weak {
        diagnostics.push(crate::model::Diagnostic::at(
            format!("'weak' property '{}' is not supported; use 'unsafe_unretained' instead", name),
            src,
            decl_offset,
        ));
        return None;
    }

    Some(PropertyInfo {
        name,
        c_type,
        is_object,
        is_readonly,
        is_nonatomic,
        ownership,
        getter_sel,
        setter_sel,
        ivar_name: None,
        decl_offset,
        /* Corrected by the caller when the declaring block is a category;
         * `Class` is right for an `@interface`, a class extension and a
         * protocol, which is every other way of reaching here. */
        origin: PropertyOrigin::Class,
    })
}

/// `@synthesize name [= ivar];` -- `property_implementation`'s only
/// `identifier` children are the property name and, when present, the
/// explicit ivar name after `=`.
pub(crate) fn extract_synthesize(node: Node, src: &str) -> (String, Option<String>) {
    let mut cursor = node.walk();
    let idents: Vec<Node> = node.children(&mut cursor).filter(|c| c.kind() == "identifier").collect();
    let name = idents.first().map(|n| node_text(*n, src).to_string()).unwrap_or_default();
    let ivar = idents.get(1).map(|n| node_text(*n, src).to_string());
    (name, ivar)
}

/// Default setter selector for a property named `name`, e.g. `"count"` ->
/// `"setCount:"` -- matches `resolve.py`'s `_synthesize_properties`.
pub(crate) fn default_setter_sel(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => format!("set{}{}:", c.to_uppercase(), chars.as_str()),
        None => "set:".to_string(),
    }
}

/// Placeholder substituted with the actual parameter name inside a C type
/// string that needs the name embedded mid-declarator (a function-pointer
/// type, e.g. `int (*NAME)(int)`) rather than appended as a plain suffix
/// (`TYPE NAME`, e.g. `int NAME`). See `detect_block_param_type`.
pub(crate) const PARAM_NAME_PLACEHOLDER: &str = "@@PARAM_NAME@@";

/// Marker for an `id` standing in *type* position inside such a type string.
///
/// The position has to be decided here, where the CST is still in hand: `id`
/// is a type and a legal-to-Clang parameter name both, and once the
/// parameter list is flat text the two are indistinguishable (#317). What it
/// lowers to is the root class pointer, which only `emit::render_param`
/// knows, so this carries the decision across to it.
pub(crate) const ID_TYPE_PLACEHOLDER: &str = "@@ID_TYPE@@";

/// A block-typed method parameter -- `(RET (^)(ARGS))name` -- parses under
/// tree-sitter-objc as a `method_type` whose `type_name` contains an
/// `abstract_function_declarator` wrapping an `abstract_parenthesized_declarator`/
/// `abstract_block_pointer_declarator` (the exact same shape a plain
/// function-pointer parameter type `(RET (*)(ARGS))name` would produce,
/// just with `^` instead of `*` -- the static subset has no block runtime,
/// so both collapse to the same plain C function-pointer type). Returns the
/// full C type text with `PARAM_NAME_PLACEHOLDER` where the parameter name
/// must be embedded, or `None` if this parameter isn't block/function-
/// pointer shaped.
fn detect_block_param_type(method_parameter: Node, src: &str) -> Option<String> {
    let method_type = child_by_kind(method_parameter, "method_type")?;
    let type_name = child_by_kind(method_type, "type_name")?;
    let func_decl = child_by_kind(type_name, "abstract_function_declarator")?;
    let mut cursor = type_name.walk();
    let ret = type_name
        .children(&mut cursor)
        .find(|c| c.kind() != "abstract_function_declarator")
        .map(|c| node_text(c, src).to_string())
        .unwrap_or_else(|| "void".to_string());
    let params = child_by_kind(func_decl, "parameter_list")
        .map(|p| {
            // Mark type-position `id` while the nodes are still here; see
            // `ID_TYPE_PLACEHOLDER`.
            let mut edits = Vec::new();
            crate::emit::rewrite_id_types(p, src, 0, ID_TYPE_PLACEHOLDER, &mut edits);
            crate::emit::apply_edits(src, p.start_byte(), p.end_byte(), &edits)
        })
        .unwrap_or_else(|| "(void)".to_string());
    Some(format!("{} (*{}){}", ret, PARAM_NAME_PLACEHOLDER, params))
}

/// method_declaration / method_definition share the shape:
/// [-|+] method_type identifier (method_parameter | identifier method_parameter)* ...
pub(crate) fn extract_method_sig(
    node: Node,
    src: &str,
    self_class: &str,
    known_classes: &HashSet<String>,
) -> MethodSig {
    let is_class_method = child_by_kind(node, "+").is_some();
    let mut cursor = node.walk();
    let mut children = node.children(&mut cursor).peekable();

    let mut return_type = String::from("void");
    let mut returns_instancetype = false;
    let mut selector = String::new();
    let mut params = Vec::new();

    while let Some(child) = children.next() {
        match child.kind() {
            "method_type" if selector.is_empty() => {
                let (t, stars) = extract_type_and_stars(child, src);
                return_type = if t == "instancetype" {
                    returns_instancetype = true;
                    format!("struct {} *", self_class)
                } else {
                    render_type(&t, stars, known_classes)
                };
            }
            "identifier" => {
                selector.push_str(node_text(child, src));
            }
            "method_parameter" => {
                selector.push(':');
                let param_type = detect_block_param_type(child, src).unwrap_or_else(|| {
                    let (t, stars) = extract_type_and_stars(child, src);
                    if t == "instancetype" {
                        format!("struct {} *", self_class)
                    } else {
                        render_type(&t, stars, known_classes)
                    }
                });
                let param_name = child_by_kind(child, "identifier")
                    .map(|n| node_text(n, src).to_string())
                    .unwrap_or_default();
                params.push((param_name, param_type));
            }
            _ => {}
        }
    }

    /* `is_optional` is the protocol collector's to set -- this extractor
     * is shared with class declarations, which have no marker. */
    MethodSig {
        is_class_method,
        selector,
        return_type,
        params,
        returns_instancetype,
        is_optional: false,
    }
}

pub fn collect(source: &str) -> (Program, Vec<crate::model::Diagnostic>) {
    let tree = crate::parse::parse(source);
    let root = tree.root_node();

    /* Which preprocessor conditional arms are part of the program at all.
     * Derived here, where the tree is already parsed, and carried on the
     * `Program` so `emit` reads these verdicts rather than deriving its
     * own -- two oracles could disagree, and a class collected from one
     * arm and emitted from the other is not a shape worth allowing to
     * exist (#570, #573). */
    let preproc = crate::preproc::Liveness::scan(source);

    // Pass 1: class names + hierarchy + category associations, and
    // protocol declarations (name, inheritance, own methods).
    let mut classes: std::collections::HashMap<String, ClassInfo> = std::collections::HashMap::new();
    let mut class_order = Vec::new();
    let mut protocols: std::collections::HashMap<String, ProtocolInfo> = std::collections::HashMap::new();
    /* (protocol name, `property_declaration` node), resolved once
     * `known_classes` is built -- see the comment at the recording site. */
    let mut protocol_property_nodes: Vec<(String, Node)> = Vec::new();
    // First-seen (line, col) per class, kept only for the
    // superclass-resolution diagnostic below -- not part of `ClassInfo`
    // itself, since nothing downstream needs it.
    let mut first_seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    /* Every `@interface`/`@implementation Class (Category)` seen, as
     * (extended class, category name, span) -- checked against
     * `known_classes` once pass 1 has seen every declaration (#501). */
    /* Every block that extends a class rather than declaring one: named
     * categories and class extensions alike, each with the `InterfaceKind`
     * needed to word its diagnostic. */
    let mut category_sites: Vec<(String, InterfaceKind, std::ops::Range<usize>)> = Vec::new();
    /* Every `@implementation Foo` with no category or extension marker, so
     * it can be checked for a matching `@interface` once pass 1 has seen
     * every declaration (#567). Separate from `category_sites` because the
     * diagnostic differs: a category extends a class, an implementation
     * *is* one half of it. */
    let mut impl_sites: Vec<(String, std::ops::Range<usize>)> = Vec::new();
    /* Names an `@interface` actually declared. `known_classes` cannot answer
     * this: pass 1 still creates a `ClassInfo` from an `@implementation`, so
     * the fabricated class is in the map by the time the check runs -- which
     * is the same reason #529's extension check could not use it either. */
    let mut interface_declared: HashSet<String> = HashSet::new();
    for node in preproc.effective_top_level(root) {
        if node.kind() == "protocol_declaration" {
            // Protocol methods don't reference class types in these
            // fixtures; an empty known-class set is fine here since
            // conformance/dispatch resolution happens later via selector
            // matching, not through this parse.
            let known: HashSet<String> = HashSet::new();
            let info = extract_protocol(node, source, &known);
            /* Recorded, not resolved -- the same reason the `@synthesize`
             * loop below defers. `extract_property` needs the known-class
             * set to render a class-typed property (`Thing *p` has to
             * become `struct Thing *`), and that set does not exist until
             * every `@interface` has been seen. Extracting here with the
             * empty set would type an object property as a bare
             * `Thing *` (#498). */
            collect_protocol_property_nodes(node, &info.name, &mut protocol_property_nodes);
            protocols.insert(info.name.clone(), info);
            continue;
        }
        if node.kind() != "class_interface" && node.kind() != "class_implementation" {
            continue;
        }
        let (name, superclass, kind) = class_header(node, source);
        /* An `@implementation` does not bring a class into existence -- only
         * an `@interface` does. Until #567 this loop accepted both node
         * kinds and `class_header` answered `Primary` for either, so a bare
         * `@implementation` **fabricated** the class: exactly what a class
         * extension used to do before #529, one construct over.
         *
         * The consequence is worse than the extension's was. Every class
         * gets synthesized members -- `Foo_oz_alloc`, `Foo_oz_free`, the
         * slab -- and those are driven by the `@interface`, which is absent,
         * so the shared `oz2c_dispatch.c`'s `oz_release` called a
         * `Foo_oz_free` nothing defined. `--gc-sections` cannot hide that
         * one: the dispatch is reached from `main`.
         *
         * Clang only *warns* here ("cannot find interface declaration"), so
         * the AST-dump gate lets it through -- a warning is not a gate.
         * Recorded and checked against `known_classes` below, beside the
         * category (#501) and the extension (#529): one construct, three
         * spellings, one refusal. */
        if node.kind() == "class_implementation" && kind.declares_class() {
            impl_sites.push((name.clone(), node.start_byte()..node.end_byte()));
        }
        if node.kind() == "class_interface" && kind.declares_class() {
            interface_declared.insert(name.clone());
        }
        if !kind.declares_class() {
            /* Neither a category nor a class extension declares a new
             * class, so neither contributes anything here -- but both are
             * constructs whose extended class may never be declared at all,
             * and the `classes` map is not complete until this loop ends.
             * Recorded and checked below (#501 for the category, #529 for
             * the extension, which reached that check as a *primary*
             * interface and so silently fabricated the class instead). */
            category_sites.push((name, kind, node.start_byte()..node.end_byte()));
            continue;
        }
        if !classes.contains_key(&name) {
            first_seen.insert(name.clone(), node.start_byte());
            classes.insert(
                name.clone(),
                ClassInfo { name: name.clone(), superclass, ..Default::default() },
            );
            class_order.push(name.clone());
        } else if let Some(sup) = superclass {
            classes.get_mut(&name).unwrap().superclass = Some(sup);
        }
        if node.kind() == "class_interface" {
            let conforms = extract_conformance(node, source);
            if !conforms.is_empty() {
                classes.get_mut(&name).unwrap().conforms = conforms;
            }
        }
    }

    let known_classes: HashSet<String> = classes.keys().cloned().collect();
    let mut diagnostics: Vec<crate::model::Diagnostic> = Vec::new();

    /* The deferred half of protocol property collection (#498). */
    for (protocol_name, decl) in protocol_property_nodes {
        let Some(prop) = extract_property(decl, source, &known_classes, &mut diagnostics) else {
            continue;
        };
        if let Some(info) = protocols.get_mut(&protocol_name) {
            info.properties.push(prop);
        }
    }

    /*
     * Before anything else, because it is a fact about the *names* in the
     * source rather than about any class: `id` is reserved, and a
     * declaration that uses it as a name cannot be lowered coherently.
     * Here rather than in one of `staticbar`'s three body-scoped entry
     * points (`check_method_body`, `check_function_body`,
     * `check_macro_body`) -- none of those sees an ivar block or a
     * file-scope struct, and the name is reserved in those positions too.
     */
    diagnostics.extend(crate::staticbar::check_reserved_names(root, source));

    /*
     * And for the same reason: a send of `-retain`, `-release`,
     * `-autorelease` or `-dealloc` is a fact about the send, not about the
     * body it sits in. ARC is always enabled, so those four are the
     * runtime's to emit and not the author's to write (#428), and none of
     * `staticbar`'s body-scoped entry points sees every position one can
     * appear in -- `walk_for_reject` treats a block literal as opaque.
     */
    /*
     * A malformed send, ahead of the checks below because it is the one
     * that makes the others reachable: until #494, `emit::parse_message`
     * could not say "not a send", so `collect::prescan_reflection` panicked
     * on `[s isEqual other]` before any diagnostic was registered, and
     * `[s take:n n]` silently dropped a token into valid-looking C.
     */
    diagnostics.extend(crate::staticbar::check_malformed_sends(root, source));

    diagnostics.extend(crate::staticbar::check_manual_memory_sends(root, source));

    /*
     * And `@autoreleasepool`, for the third time the same reason: the
     * construct is refused wherever it appears, and what makes it refusable
     * is that `-autorelease` is one of the sends above -- with no way to
     * make a reference pending, a pool has nothing to drain (#430).
     */
    diagnostics.extend(crate::staticbar::check_autoreleasepool(root, source));

    /*
     * And the ARC ownership attributes, for the fourth time the same
     * reason -- plus one of its own: `ns_returns_not_retained` on a
     * create-rule selector makes ARC and oz2c disagree about who owns
     * the result, which is a use-after-free rather than a leak (#458).
     * None of the body-scoped entry points sees a method *declaration* in
     * an `@interface`, which is where these are usually written.
     */
    diagnostics.extend(crate::staticbar::check_ownership_attributes(root, source));

    /*
     * And the two bridging casts that transfer a reference, for the fifth
     * time the same reason. `__bridge_retained` would have to emit a retain
     * and `__bridge_transfer` a release; neither does, so one hands C a
     * freed pointer and the other strands a `+1` (#460). Plain `__bridge`
     * transfers nothing and stays supported.
     */
    /*
     * And `__weak`, for the sixth time the same reason -- plus the reason
     * that made it urgent: it was refused in *one* position out of ten, and
     * the other nine copied the token into the generated C, where the
     * failure is the C compiler's on a file the author never wrote. Worse
     * than that on macOS, where Apple clang accepts ARC qualifiers in plain
     * C, so every host gate can pass over output that only CI or a target
     * build rejects (#448).
     */
    diagnostics.extend(crate::staticbar::check_refused_qualifiers(root, source));

    diagnostics.extend(crate::staticbar::check_bridging_casts(root, source));

    // A `superclass` reference that doesn't resolve to a class actually
    // collected above (e.g. a real Foundation class only ever pulled in
    // via `#import <Foundation/Foundation.h>` -- oz2c has no import
    // resolution of its own, so it's genuinely undefined in this
    // translation unit) must be a named, located hard error here, not a
    // downstream panic: every later pass -- `companion::topological_order`
    // in particular -- assumes every `superclass` string is itself a key
    // in `classes`, and indexes it directly (`program.classes[name]`,
    // which panics on a miss) rather than through a fallible lookup.
    for name in &class_order {
        let Some(sup) = &classes[name].superclass else { continue };
        if !known_classes.contains(sup) {
            let offset = first_seen[name];
            diagnostics.push(crate::model::Diagnostic::at(
                format!(
                    "class '{}' extends '{}', but no class '{}' is defined in this source \
(oz2c has no #import resolution -- provide a single, self-contained translation unit)",
                    name, sup, sup
                ),
                source,
                offset,
            ));
        }
    }

    /* A superclass **cycle**, for the same reason as the unresolved
     * reference above and with a worse consequence. Every ancestry walk in
     * the tree climbs `superclass` with `while let Some(name) = cur`, and
     * exactly one of the fifteen -- `companion.rs`'s `visit` -- carries a
     * visited set. The other fourteen run forever on a cycle, and the first
     * one reached is `Program::owned_object_ivar_names`: it pushes a
     * `String` per step into a `Vec` that is never read, so the process
     * grows at ~500 MB/s and is killed by the supervisor with an **empty
     * stderr** -- no file, no line, no construct named. `@interface A : A`
     * is a one-character typo that turns `west build` into a
     * memory-exhaustion event (#547).
     *
     * Checked **here**, once, rather than by guarding fourteen walks.
     * `lib.rs` gates the pipeline on this function's diagnostics before
     * arc, generics, pools or emit run, so rejecting the cycle at this
     * point makes every later walk acyclic *by invariant* -- which is a
     * stronger guarantee than fourteen independent visited sets, and one
     * that a fifteenth walk added later inherits for free.
     *
     * The protocol equivalents need no such check: Clang refuses them
     * first ("protocol has circular dependency"), and a class cycle
     * reaches oz2c before Clang runs. */
    /* One diagnostic per *cycle*, not per class in it. A two-class cycle
     * reported from both ends is one defect twice: fixing either class's
     * superclass fixes both, and the second message sends the reader
     * looking for a second problem. Every class on a reported cycle is
     * recorded here so the walk starting from it stays silent. */
    let mut in_reported_cycle: HashSet<String> = HashSet::new();
    for name in &class_order {
        if in_reported_cycle.contains(name) {
            continue;
        }
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(name.clone());
        let mut path = vec![name.clone()];
        let mut cur = classes[name].superclass.clone();
        while let Some(sup) = cur {
            if !classes.contains_key(&sup) {
                /* The unresolved-reference check above owns this case and
                 * has already reported it; walking further would index a
                 * missing key. */
                break;
            }
            path.push(sup.clone());
            if !seen.insert(sup.clone()) {
                in_reported_cycle.extend(path.iter().cloned());
                let offset = first_seen[name];
                let chain = path.join(" -> ");
                let message = if path.len() == 2 && path[0] == path[1] {
                    format!("class '{}' cannot be its own superclass", name)
                } else {
                    format!("superclass cycle: {}", chain)
                };
                diagnostics.push(
                    crate::model::Diagnostic::at(message, source, offset)
                        .with_note(
                            "every pass that resolves an inherited ivar, method or                              protocol climbs the superclass chain, so a cycle has no                              root to stop at and the walk does not terminate"
                                .to_string(),
                        )
                        .with_help(format!(
                            "give '{}' a superclass that does not lead back to it, or                              make it a root class",
                            name
                        )),
                );
                break;
            }
            cur = classes[&sup].superclass.clone();
        }
    }

    /* An `@implementation` with no `@interface` (#567). Beside the category
     * and extension checks below for the same reason: a construct that does
     * not declare a class, reaching a pipeline that assumes one was
     * declared.
     *
     * Reported per class rather than per block, so a class with several
     * `@implementation` blocks -- the primary plus categories -- names its
     * missing `@interface` once. */
    /* `impl_sites` is every primary `@implementation` in this source, which
     * is also the question `emit::reject_undefined_target` needs answered
     * (#566): a declared selector with no body is an omission only when the
     * class's own implementation is here to have omitted it. Recorded off
     * the same list rather than a second scan, so the two checks cannot
     * disagree about what counts as primary. */
    for (name, _) in &impl_sites {
        if let Some(info) = classes.get_mut(name) {
            info.has_primary_implementation = true;
        }
    }
    /* Not when the source did not parse (#567's own regression, found by
     * sweeping the whole mutation corpus rather than the fixtures the issue
     * named -- `oz2c-challenges` M21).
     *
     * This check asserts an **absence**: no `@interface` for this name
     * exists in this source. That claim is only sound over a tree that
     * parsed. `@interface : OZObject` -- a nameless interface, M21 -- puts
     * an `ERROR` node on the stray `:` and leaves `OZObject` as the first
     * `identifier`, so `class_header` reads the *superclass* as the class
     * name and the real class is declared nowhere. The conclusion is then
     * true of the parse and derived from the syntax error, and the
     * diagnostic points at the `@implementation` while the mistake is on
     * the `@interface` line several lines above.
     *
     * Worse, it *pre-empted* the AST gate: this runs in `collect`, which
     * returns before `attach_ast`, so the author never reached Clang's
     * `expected identifier` -- the message `MUTATIONS.md` grades M21
     * `CLANG` for, meaning the C front end is the right reporter. M04, M05
     * and M09 reach #566's check the same way and are saved only by the
     * AST gate firing first, which is luck rather than design.
     *
     * A file with a parse error loses no coverage by being skipped here:
     * it is refused by the AST requirement, or by Clang at dump time, and
     * either names the cause instead of a consequence. Same reasoning as
     * #561 -- do not diagnose a fact derived from an earlier failure. */
    let mut reported_impl: HashSet<String> = HashSet::new();
    let source_parsed = !crate::staticbar::tree_has_errors(root);
    for (name, span) in &impl_sites {
        if !source_parsed {
            break;
        }
        if interface_declared.contains(name) || !reported_impl.insert(name.clone()) {
            continue;
        }
        diagnostics.push(
            crate::model::Diagnostic::spanning(
                format!("'@implementation {}' has no '@interface {}' in this source", name, name),
                source,
                span.clone(),
            )
            .with_note(
                "every class gets synthesized members driven by its '@interface' -- an \
                 allocator, a deallocator and a slab -- so without one the shared \
                 dispatch calls a deallocator nothing defines, and the author reads a \
                 mangled symbol in a generated file. Clang only warns here, so the \
                 AST-dump gate does not catch it"
                    .to_string(),
            )
            .with_help(format!(
                "add '@interface {} : OZObject' (or a suitable superclass), or '#import' \
                 the header that declares it",
                name
            )),
        );
    }

    /* A category on a class this translation unit never declares, for the
     * same reason and with the same consequence as the superclass check
     * above: `emit::render_category_interface` and
     * `emit::render_method_definition` both index `program.classes[name]`
     * directly, so a category whose class was never collected panics with
     * `no entry found for key` -- unlocated, naming neither the class nor
     * the file (#501).
     *
     * Only an *empty* category body escapes that, because nothing inside it
     * reaches an indexing site; that one transpiles successfully, emits a
     * banner comment where the category had been, and drops it in silence.
     * Neither outcome is a diagnostic, and the two differ only in whether
     * the category happens to declare a member. The empty shape is also
     * why this carries a `!`: it is a program that builds today and will
     * not after this.
     *
     * Hard rather than warning-level: there is no non-fatal diagnostic
     * channel (`Diagnostic` carries no severity and `lib::transpile`
     * returns `Err` on any diagnostic at all), and there is nothing for
     * oz2c to attach the methods to even if it warned -- a category's
     * members merge into the extended class's `ClassInfo`, and there is no
     * `ClassInfo`. Clang only warns here, but Clang has a runtime that can
     * carry an unattached category; the generated C has a struct or it has
     * nothing.
     *
     * A **class extension** on an undeclared class is the same defect and is
     * checked here too, which it could not be before #529: an extension came
     * back from `class_header` as a primary `@interface`, so it never reached
     * this list at all and instead *fabricated* the class in pass 1 -- a
     * `struct Ghost` with no superclass, hence a second root class, from
     * source that declares no such thing. The check could not see it because
     * by the time it ran the class it was looking for existed, having been
     * invented three hundred lines earlier. */
    for (class_name, kind, span) in &category_sites {
        if known_classes.contains(class_name) {
            continue;
        }
        let (what, spelled, drop_hint) = match kind.category_name() {
            Some(cat) => (
                "category",
                format!("{}({})", class_name, cat),
                format!(
                    "if '{}' was meant to be a new class rather than a category on an \
                     existing one, drop the '({})'",
                    class_name, cat
                ),
            ),
            None => (
                "class extension",
                format!("{}()", class_name),
                format!(
                    "if '{}' was meant to be a new class rather than an extension of an \
                     existing one, drop the '()' and give it a superclass",
                    class_name
                ),
            ),
        };
        diagnostics.push(
            crate::model::Diagnostic::spanning(
                format!(
                    "{} '{}' extends '{}', but no class '{}' is declared in this \
source",
                    what, spelled, class_name, class_name
                ),
                source,
                span.clone(),
            )
            .with_note(format!(
                "a {}'s methods and properties merge into the class it extends, so \
                 without an '@interface' for that class there is no struct to add them to \
                 and nothing would be emitted for the {} at all",
                what, what
            ))
            .with_help(format!(
                "declare '@interface {}' in this translation unit, or '#import' the header \
                 that does",
                class_name
            ))
            .with_help(drop_hint),
        );
    }

    // Every `@synthesize` seen in pass 2, as (class name, node), resolved
    // against the collected properties only once the pass is done -- see
    // where they are pushed for why it cannot happen inline.
    let mut synthesizes: Vec<(String, Node)> = Vec::new();

    // Pass 2: ivars (from interfaces) + method signatures (from
    // declarations and definitions, category included).
    for node in preproc.effective_top_level(root) {
        match node.kind() {
            "class_interface" => {
                let (name, _, kind) = class_header(node, source);
                // A category's methods and properties merge into the
                // class it extends (mirroring the oracle's
                // `collect.py::_merge_category`); its ivars do not,
                // because ObjC categories cannot declare any. A class
                // extension merges *everything*, ivars included -- it is
                // part of the class, not an addition to it (#529).
                //
                // Either way this block is not the class's only one, so a
                // selector or property it restates may already be present:
                // pushes from anything but the primary interface are
                // deduplicated.
                let is_primary = kind.is_primary();
                if kind.may_declare_ivars() {
                    let (ivars, unretained, extents) =
                        extract_ivars_with_ownership(node, source, &known_classes);
                    if let Some(info) = classes.get_mut(&name) {
                        /* Append-if-absent, not assignment. This used to
                         * read `info.own_ivars = ivars;`, which was
                         * invisible while an `@interface` was a class's
                         * only ivar-declaring interface block and became a
                         * silent catastrophe once a class extension also
                         * reached here: the extension *replaced* the
                         * primary's ivar list. An extension declaring one
                         * ivar left the class owning only that one; an
                         * extension declaring none -- the common shape,
                         * adding only private methods -- left the class
                         * owning *nothing*, which loses every ARC release
                         * on dealloc and drops every ivar out of method
                         * scope (#529). The `@implementation` arm below
                         * always appended; the two arms simply disagreed.
                         *
                         * Idempotent, so the primary interface is
                         * unaffected: its own names are absent the first
                         * time and skipped on any later pass. */
                        for (ivar, c_type) in ivars {
                            if !info.own_ivars.iter().any(|(n, _)| *n == ivar) {
                                info.own_ivars.push((ivar, c_type));
                            }
                        }
                        info.unretained_ivars.extend(unretained);
                        info.array_extents.extend(extents);
                    }
                }
                /* `@interface Foo () <Proto>` -- a class extension is where
                 * a privately adopted protocol is conventionally declared,
                 * and pass 1 no longer visits an extension at all, so the
                 * conformance is merged here. Appended rather than assigned
                 * for the same reason as the ivars above. */
                if kind.is_extension() {
                    let conforms = extract_conformance(node, source);
                    if let Some(info) = classes.get_mut(&name) {
                        for protocol in conforms {
                            if !info.conforms.contains(&protocol) {
                                info.conforms.push(protocol);
                            }
                        }
                    }
                }
                let mut c = node.walk();
                for decl in node.children(&mut c) {
                    if decl.kind() == "method_declaration" {
                        let sig = extract_method_sig(decl, source, &name, &known_classes);
                        if let Some(info) = classes.get_mut(&name) {
                            let dup = !is_primary
                                && info.methods.iter().any(|m| {
                                    m.selector == sig.selector
                                        && m.is_class_method == sig.is_class_method
                                });
                            if !dup {
                                info.methods.push(sig);
                            }
                        }
                    } else if decl.kind() == "property_declaration" {
                        if let Some(mut prop) =
                            extract_property(decl, source, &known_classes, &mut diagnostics)
                        {
                            /* The one place the declaring block's kind and
                             * the property are both in hand. Recorded on
                             * the property because it is unrecoverable
                             * afterwards: a category's properties merge
                             * into the extended class's list, and by emit
                             * time nothing distinguishes them from the
                             * class's own (#530). */
                            prop.origin = if kind.is_category() {
                                PropertyOrigin::Category
                            } else {
                                PropertyOrigin::Class
                            };
                            if let Some(info) = classes.get_mut(&name) {
                                let dup = !is_primary
                                    && info.properties.iter().any(|p| p.name == prop.name);
                                if !dup {
                                    info.properties.push(prop);
                                }
                            }
                        }
                    }
                }
            }
            "class_implementation" => {
                let (name, _, kind) = class_header(node, source);
                if !classes.contains_key(&name) {
                    continue;
                }
                // Modern Objective-C lets a class declare its ivars in the
                // `@implementation` block rather than the `@interface`, which
                // keeps them private to the implementation
                // (`samples/hello_category`'s Car does exactly this). They
                // were never collected, so the generated struct simply
                // lacked them and every use became "use of undeclared
                // identifier '_throttleLevel'". A category cannot declare
                // ivars, so only a block that may contributes -- which for
                // an `@implementation` means anything that is not a
                // category, since there is no such thing as an
                // `@implementation Foo ()`.
                if kind.may_declare_ivars() {
                    let (impl_ivars, impl_unretained, impl_extents) =
                        extract_ivars_with_ownership(node, source, &known_classes);
                    if let Some(info) = classes.get_mut(&name) {
                        for (ivar, c_type) in impl_ivars {
                            if !info.own_ivars.iter().any(|(n, _)| *n == ivar) {
                                info.own_ivars.push((ivar, c_type));
                            }
                        }
                        info.unretained_ivars.extend(impl_unretained);
                        info.array_extents.extend(impl_extents);
                    }
                }
                let mut c = node.walk();
                for impl_def in node.children(&mut c) {
                    if impl_def.kind() != "implementation_definition" {
                        continue;
                    }
                    if let Some(method_def) = child_by_kind(impl_def, "method_definition") {
                        let sig = extract_method_sig(method_def, source, &name, &known_classes);
                        let info = classes.get_mut(&name).unwrap();
                        if sig.is_class_method && sig.selector == "initialize" {
                            info.has_class_initialize = true;
                        }
                        info.defined_selectors
                            .insert((sig.selector.clone(), sig.is_class_method));
                        let declared = info.methods.iter().find(|m| {
                            m.selector == sig.selector && m.is_class_method == sig.is_class_method
                        });
                        match declared {
                            /* The declaration already holds this selector, so
                             * the body adds nothing to the table -- except
                             * when the two disagree about the return type
                             * (#568).
                             *
                             * The `@interface`'s spelling wins here, silently:
                             * `method_return_type` answers `int` for a body
                             * that returns an object, and `arc` then claims a
                             * `+1` reference on an `int`, which is an
                             * impossible state it reports as an
                             * ownership-analysis *bug* -- "please report it
                             * with the snippet (#398)". Ordinary malformed
                             * source was asking the author to file a
                             * transpiler issue.
                             *
                             * Gating here rather than deferring, on #540's
                             * criterion and not by default: this is the case
                             * where later passes read an inconsistent
                             * `Program`. One selector has two return types and
                             * the table can only hold one, so every consumer
                             * downstream -- `arc`'s ownership, emit's casts,
                             * the dispatch's signature -- is reasoning from a
                             * type the body does not have.
                             *
                             * Comparison is on the *resolved* C spelling, so
                             * `instancetype` against `Foo *` on `Foo` agrees
                             * (both are `struct Foo *`, per
                             * `extract_method_sig`) and only a genuine
                             * disagreement is reported. Clang's own wording
                             * for this is `conflicting return type in
                             * implementation of 'value'`, and it never gets to
                             * say it -- the internal error fired before the
                             * AST dump. */
                            Some(prior) if prior.return_type != sig.return_type => {
                                let dash = if sig.is_class_method { '+' } else { '-' };
                                diagnostics.push(
                                    crate::model::Diagnostic::spanning(
                                        format!(
                                            "'{}{}' is declared on '{}' returning '{}' and \
                                             defined returning '{}'",
                                            dash,
                                            sig.selector,
                                            name,
                                            prior.return_type,
                                            sig.return_type
                                        ),
                                        source,
                                        method_def.start_byte()..method_def.end_byte(),
                                    )
                                    .with_note(
                                        "the '@interface' spelling is the one every caller \
                                         compiles against, so the two have to agree. Taking \
                                         the declaration's type over the body's is what \
                                         reached `a +1 reference was claimed for an \
                                         expression of type 'int'` -- an internal-invariant \
                                         message asking for a bug report, from source the \
                                         author can fix"
                                            .to_string(),
                                    )
                                    .with_help(format!(
                                        "change the definition to return '{}', or the \
                                         declaration to return '{}'",
                                        prior.return_type, sig.return_type
                                    )),
                                );
                            }
                            Some(_) => {}
                            None => info.methods.push(sig),
                        }
                        continue;
                    }
                    if let Some(prop_impl) = child_by_kind(impl_def, "property_implementation") {
                        // Recorded, not resolved: the matching @property
                        // may not be collected yet. This loop walks the
                        // translation unit in source order, and an
                        // @implementation can precede its own @interface
                        // there -- which happens for real, not just in
                        // contrived input: a class whose .m uses
                        // `#include "Car.h"` rather than `#import` has its
                        // header spliced in later, by whichever file
                        // imports it. Resolving inline made the outcome
                        // depend on the order entry files were listed in.
                        synthesizes.push((name.clone(), prop_impl));
                    }
                }
            }
            _ => {}
        }
    }

    for (name, prop_impl) in synthesizes {
        let (prop_name, ivar) = extract_synthesize(prop_impl, source);
        /* Read before the mutable borrow, since the protocol lookup needs
         * the class's `conforms` list and `protocols` at once. */
        let conforms = classes.get(&name).map(|i| i.conforms.clone()).unwrap_or_default();
        let Some(info) = classes.get_mut(&name) else {
            /* Unreachable, and here is the reason rather than an
             * assertion that it is (#498). The recording arm this loop
             * consumes opens with
             *
             *     "class_implementation" => {
             *         let (name, _, kind) = class_header(...);
             *         if !classes.contains_key(&name) { continue; }
             *
             * so every `name` that reaches `synthesizes` has already been
             * found in `classes`, and nothing removes from that map in
             * between. Left as a `continue` because there is no input to
             * point a diagnostic at.
             *
             * The first draft of this comment gave a *different* reason --
             * that pass 1 inserts every implementation's name -- and that
             * reason is true but does not cover a category, which pass 1
             * skips. The guard above is what actually holds. Worth the
             * distinction: an unreachability claim is only as good as the
             * invariant it names, and the wrong invariant reads as
             * confirmation. (#529 widened what pass 1 skips from categories
             * to class extensions as well, so the superseded reason is now
             * wrong in two ways rather than one.)
             *
             * Two shapes were tried against it, both silently accepted and
             * neither reaching here: a category on a class declared
             * nowhere (filtered by that guard), and an `@implementation`
             * with no `@interface` (pass 1 inserts it, so the guard
             * passes). Clang only *warns* on the second, so neither is a
             * defect this loop should be answering for.
             *
             * "I could not find an input" is evidence about reachability,
             * not about safety: on #448 the same conclusion about
             * `static_object_locals` turned out to be a *different* defect
             * masking the path. */
            continue;
        };
        match info.properties.iter_mut().find(|p| p.name == prop_name) {
            Some(prop) => {
                if let Some(iv) = ivar {
                    prop.ivar_name = Some(iv);
                }
            }
            /* Declared by a protocol the class adopts. Clang accepts this
             * -- it is the idiomatic way to adopt a protocol property --
             * and oz2c refused it with a message that was wrong about the
             * cause: the property *is* declared, in a protocol nobody
             * looked in (#498).
             *
             * Adopting it onto the class is what makes the rest of the
             * pipeline work unchanged: `resolve_properties` below, the
             * accessor synthesis and the dealloc release all read
             * `info.properties`, so a protocol property that lands there
             * is thereafter indistinguishable from one the `@interface`
             * declared. */
            None => match protocol_property(&conforms, &protocols, &prop_name) {
                Some(mut adopted) => {
                    if let Some(iv) = ivar {
                        adopted.ivar_name = Some(iv);
                    }
                    info.properties.push(adopted);
                }
                None => {
                    diagnostics.push(crate::model::Diagnostic::at(
                        format!(
                            "'@synthesize {}' but no '@property {}' is declared on '{}' or on \
                             any protocol it adopts. A property declared on a *superclass* \
                             cannot be synthesized again here -- the subclass would be \
                             claiming the superclass's backing ivar",
                            prop_name, prop_name, name
                        ),
                        source,
                        prop_impl.start_byte(),
                    ));
                }
            },
        }
    }

    resolve_properties(&mut classes, &class_order);

    reject_undefined_category_accessors(&classes, &class_order, source, &mut diagnostics);

    reject_inline_anonymous_aggregates(root, source, &mut diagnostics);

    /* An Objective-C-bearing conditional oz2c cannot evaluate. A hard
     * gate, like `collect`'s other refusals: the class table is missing
     * whichever arm would have supplied it, and emit indexes
     * `superclass` strings into `classes` directly (#205, #501). */
    diagnostics.extend(preproc.undecidable.iter().cloned());
    /* Nothing a dead arm contains is part of the program, so no check's
     * complaint about one is either -- `check_malformed_sends` above is
     * the one #570 reported, and filtering here rather than inside each
     * check covers the other 305 tree walks and the ones not yet
     * written. */
    preproc.retain_live(&mut diagnostics);

    let reflection = prescan_reflection(root, source);
    let function_return_types = function_return_types(root, source, &known_classes, &preproc);

    (
        Program {
            classes,
            class_order,
            protocols,
            function_return_types,
            preproc,
            owning_methods: Default::default(),
            ast: None,
            heap_support: false,
            introspection: false,
            reflection: false,
            reflected_selectors: reflection.selectors,
            performed_selectors: reflection.performed,
            performs_via_value: reflection.performs_via_value,
            uses_perform_selector: reflection.performs,
            uses_responds_to_selector: reflection.responds,
            uses_synchronized: contains_kind(root, "synchronized_statement"),
            forward_declared: forward_declared_classes(root, source),
            source_parsed: !crate::staticbar::tree_has_errors(root),
        },
        diagnostics,
    )
}

/// Every class name a `@class` forward declaration names.
///
/// `@class A, B;` is a *single* `class_declaration` carrying one
/// `identifier` per name, so this takes every identifier child rather
/// than the first -- confirmed against the parse, which builds
/// `class_declaration` as `@`, `class`, `identifier`..., `;`. Reading
/// only the first would collect `A` and silently lose `B`, and the loss
/// would show up as the generic unresolvable-receiver error for `B`
/// alone: the failure mode the caller of this exists to remove.
///
/// Walks the whole tree rather than the top level. `@class` belongs at
/// file scope and every real one sits there, but a walk that assumed so
/// would silently collect nothing from a file that nests one, and not
/// assuming costs one recursion.
fn forward_declared_classes(root: Node, source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    walk_forward_declared(root, source, &mut names);
    names
}

fn walk_forward_declared(node: Node, source: &str, names: &mut BTreeSet<String>) {
    if node.kind() == "class_declaration" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                names.insert(node_text(child, source).to_string());
            }
        }
        /* Nothing under a `@class` but its own names. */
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_forward_declared(child, source, names);
    }
}

/// A top-level `function_definition`'s C return type, rendered the way
/// every other generated type is -- so a class name arrives with its
/// `struct` tag (`Thing *` -> `struct Thing *`), an `id` lowers to
/// `void *`, and a `struct`/`enum` keeps its keyword.
///
/// `render_method_definition` records the method equivalent into
/// `EmitCtx::method_return_type`; this is the free-function half, and its
/// absence was #336: the `function_definition` arm built a fresh `EmitCtx`
/// and left the field at `EmitCtx::new`'s placeholder, so the temporary
/// `render_return_statement` synthesizes on the cleanup path came out
/// `int` whatever the function actually returned. A returned pointer was
/// then a constraint violation on any target and a truncation on a 64-bit
/// one, and a `double` was silently rounded.
///
/// Read off the function's *own* declarator rather than guessed from the
/// returned expression, which is the only thing that can be right for
/// `return 42;` in a `size_t` function -- there is nothing in the
/// expression to read.
///
/// The type text comes from the whole `function_definition` (the first
/// type specifier in child order is the return type, and every later one
/// is ignored -- see `collect::extract_type_and_stars`); the stars do not,
/// because a `*` inside the `function_declarator` belongs to a parameter.
/// `declared_block_pointer_type` splits the two for the same reason.
pub(crate) fn function_return_type(
    node: Node,
    src: &str,
    known: &HashSet<String>,
) -> Option<String> {
    let declarator = node.child_by_field_name("declarator")?;
    let (type_text, _) = extract_type_and_stars(node, src);
    if type_text.is_empty() {
        return None;
    }
    Some(render_type(&type_text, declarator_return_stars(declarator), known))
}

/// `*`s belonging to the declared thing itself, i.e. those before the
/// `function_declarator` that carries the parameter list. A star past it
/// is a parameter's, and a `block_literal` is a whole nested signature.
pub(crate) fn declarator_return_stars(node: Node) -> usize {
    if node.kind() == "function_declarator" || node.kind() == "block_literal" {
        return 0;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .map(|c| if c.kind() == "*" { 1 } else { declarator_return_stars(c) })
        .sum()
}

/// Every top-level C function's declared return type, by name, rendered
/// as the C the output will use (`Thing *` -> `struct Thing *`).
///
/// This is what lets a **call result be a message receiver** (#355).
/// Without it `render_expr` had no `call_expression` arm at all, so
/// `makeThing()` fell to the default arm and was typed `id` -- and the
/// send was then refused, or worse, silently resolved against the root
/// class, because `render_owning_operand_statement` reads a
/// non-pointer type as "some object, cast it to the root pointer". So
/// `[makeThing() poke]` reported `class 'OZObject' has no method
/// matching 'poke'` -- naming a class the source never mentions -- while
/// `Thing *t = makeThing(); [t poke];` compiled. The information was
/// there in the callee's own signature; nothing read it.
///
/// **Prototypes are recorded as well as definitions**, and deliberately:
/// a function declared in a header and defined in a plain `.c` compiled
/// alongside has no `function_definition` in this translation unit, and
/// its declared return type is no less definite for that. A definition
/// wins if both are seen, though a program where they disagree does not
/// compile anyway.
///
/// What is *not* recorded is a call through anything but a plain
/// identifier -- a function pointer, a block variable, a member. Those
/// keep the old `id`, so the only behaviour that changes is the one the
/// declared type answers.
pub(crate) fn function_return_types(
    root: Node,
    src: &str,
    known: &HashSet<String>,
    preproc: &crate::preproc::Liveness,
) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let mut from_definition: HashSet<String> = HashSet::new();
    for node in preproc.effective_top_level(root) {
        let is_definition = node.kind() == "function_definition";
        if !is_definition && node.kind() != "declaration" {
            continue;
        }
        /* A `declaration` covers far more than a prototype -- every
         * file-scope variable is one -- so the presence of a
         * `function_declarator` is what says this declares a function.
         * `declares_function` looks only as deep as the declarator
         * chain, so a variable whose *type* mentions a function
         * (a function pointer) is not mistaken for one. */
        if !is_definition && !declares_function(node, src) {
            continue;
        }
        let Some(name) = function_declared_name(node, src) else {
            continue;
        };
        if !is_definition && from_definition.contains(&name) {
            continue;
        }
        let Some(ty) = function_return_type(node, src, known) else {
            continue;
        };
        if is_definition {
            from_definition.insert(name.clone());
        }
        out.insert(name, ty);
    }
    out
}

/// Does this `declaration` declare a function -- i.e. is its outermost
/// declarator a `function_declarator`, however many pointer layers the
/// return type wraps it in?
///
/// A function *pointer* variable (`Thing *(*fp)(void);`) is not one: its
/// `function_declarator` sits inside a `parenthesized_declarator`, which
/// this stops at. That matters because such a variable's *call* result is
/// exactly what `function_return_types` declines to answer for.
fn declares_function(decl: Node, src: &str) -> bool {
    fn walk(node: Node, src: &str) -> bool {
        match node.kind() {
            "function_declarator" => true,
            "parenthesized_declarator" => false,
            "pointer_declarator" | "init_declarator" => {
                let mut cursor = node.walk();
                let children: Vec<Node> = node.children(&mut cursor).collect();
                children.into_iter().any(|child| walk(child, src))
            }
            _ => false,
        }
    }
    let mut cursor = decl.walk();
    let children: Vec<Node> = decl.children(&mut cursor).collect();
    children.into_iter().any(|child| walk(child, src))
}

/// The name a `function_definition` or function `declaration` gives its
/// function, reached through however many declarator layers its return
/// type needs.
///
/// The identifier taken is the `function_declarator`'s own, not the first
/// one found anywhere beneath: a parameter is an identifier too, and
/// `Thing *f(int n)` would otherwise answer `n` about half the time
/// depending on child order.
fn function_declared_name(node: Node, src: &str) -> Option<String> {
    fn walk(node: Node, src: &str) -> Option<String> {
        if node.kind() == "function_declarator" {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            for child in &children {
                if child.kind() == "identifier" {
                    return Some(node_text(*child, src).to_string());
                }
            }
            for child in &children {
                if child.kind() == "parameter_list" {
                    continue;
                }
                if let Some(found) = walk(*child, src) {
                    return Some(found);
                }
            }
            return None;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            if let Some(found) = walk(child, src) {
                return Some(found);
            }
        }
        None
    }
    walk(node, src)
}

/// The reflection facts the dispatch tables have to know before they are
/// generated: which selectors a `@selector(...)` names, and whether the
/// source performs or asks about responding.
///
/// A prescan rather than something `emit` accumulates, because
/// `Program::is_dynamically_dispatched` consults it to decide which
/// selectors get an `OZ_PROTOCOL_SEND_*` function -- a question already
/// answered by the time `emit` runs. The introspection facts have no such
/// ordering constraint and are tracked during emission instead (see
/// `emit::IntrospectionUse`).
///
/// This reads the syntax, not the option: with `CONFIG_OBJZ_REFLECTION`
/// off these constructs are refused, and refusing them is `emit`'s and the
/// static bar's job, not this scan's.
fn prescan_reflection(root: Node, source: &str) -> ReflectionFacts {
    const PERFORM_SELECTORS: &[&str] = &[
        "performSelector:",
        "performSelector:withObject:",
        "performSelector:withObject:withObject:",
    ];
    let mut facts = ReflectionFacts::default();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "selector_expression" => {
                if let Some(name) = selector_literal_name(node, source) {
                    facts.selectors.insert(name);
                }
            }
            "message_expression" => {
                let selector = crate::staticbar::message_selector(node, source);
                if PERFORM_SELECTORS.contains(&selector.as_str()) {
                    facts.performs = true;
                    // Which selector this site performs, when the answer
                    // is written at the site. `@selector(...)` is a direct
                    // child of the message, immediately after the first
                    // `:`, so the first argument is exactly identifiable.
                    match first_argument(node) {
                        Some(arg) if arg.kind() == "selector_expression" => {
                            if let Some(name) = selector_literal_name(arg, source) {
                                facts.performed.insert(name);
                            }
                        }
                        // A `SEL` held in a local, an ivar, a parameter or
                        // a cast. Nothing can say which selector arrives
                        // here, so performability stops being a per-
                        // selector question and becomes a whole-program
                        // one.
                        _ => facts.performs_via_value = true,
                    }
                } else if selector == "respondsToSelector:" {
                    facts.responds = true;
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    facts
}

/// What `prescan_reflection` found.
#[derive(Default)]
struct ReflectionFacts {
    selectors: std::collections::BTreeSet<String>,
    performed: std::collections::BTreeSet<String>,
    performs: bool,
    performs_via_value: bool,
    responds: bool,
}

/// The first argument of a message expression: the child immediately
/// following its first `:`.
fn first_argument(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let colon = children.iter().position(|c| c.kind() == ":")?;
    children.get(colon + 1).copied()
}

/// The selector `@selector(name)` names, as its Objective-C spelling
/// (`setValue:` keeps its colon).
///
/// Read from the node's own text rather than from its children, because
/// tree-sitter-objc 3.0.2 exposes no children at all inside a *keyword*
/// selector: `@selector(poke)` yields an `identifier`, while
/// `@selector(wrap:)` and `@selector(a:b:)` yield only the `@selector`,
/// `(` and `)` tokens -- the name is simply absent from the tree. Walking
/// children therefore found nothing for every selector that takes an
/// argument, which is most of them. The node's text is unambiguous, so
/// slicing between the parentheses is both simpler and complete.
///
/// Returns `None` for anything that is not a well-formed selector, so a
/// malformed `@selector( )` is refused rather than mangled into a bogus C
/// identifier.
pub(crate) fn selector_literal_name(node: Node, src: &str) -> Option<String> {
    let text = &src[node.start_byte()..node.end_byte()];
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close <= open {
        return None;
    }
    let name = text[open + 1..close].trim();
    if name.is_empty() {
        return None;
    }
    // A selector is identifier pieces separated by colons. Nothing else
    // belongs, and a stray character would otherwise reach
    // `emit::selector_to_c` and come out as an identifier that no
    // generated record matches.
    let valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
        && !name.starts_with(':')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    if !valid {
        return None;
    }
    Some(name.to_string())
}

/// Is there a node of `kind` anywhere under `node`?
fn contains_kind(node: Node, kind: &str) -> bool {
    if node.kind() == kind {
        return true;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().any(|c| contains_kind(c, kind))
}

/// An inline anonymous aggregate -- `enum { A, B }`, `struct { int x; }`,
/// `union { ... }` written directly as a method's return or parameter type
/// -- has no tag to spell the type by anywhere outside its own
/// declaration, so `extract_type_and_stars` has nothing to hand back but
/// the bare keyword. That reaches codegen as `enum Foo_ret(struct Foo *)`
/// / `struct p`, which is not valid C; for a `union` it is worse, because
/// the generic recursive fallback descends into the body and picks up the
/// first member's type, silently emitting `int u` for a union-typed
/// parameter. Both are exactly the silent degradation this backend is not
/// allowed to do, so they are rejected here instead.
///
/// This is not a parity gap: no oracle case uses the shape (every enum
/// case in `tests/behavior/cases/enum/` declares a *named* top-level enum
/// and refers to it by tag), and the oracle's own `_collect_enum_def`
/// (`tools/oz_transpile/collect.py`) keys its reconstruction on the enum's
/// name, degenerating to `"enum "` when there isn't one. Supporting the
/// shape would also mean giving the *same* logical type a stable
/// synthesized tag across two syntactically distinct anonymous
/// declarations -- the `@interface` prototype's and the
/// `@implementation` definition's -- which C itself treats as two
/// unrelated types, so there is nothing well-formed to aim at.
///
/// Scoped to `method_type` (the wrapper the grammar puts around both a
/// return type and each parameter type) on purpose: the same anonymous
/// aggregate is fine as an *ivar*, where `emit::lower_ivar_decl` copies
/// the declaration through with its body intact.
fn reject_inline_anonymous_aggregates(
    root: Node,
    src: &str,
    diagnostics: &mut Vec<crate::model::Diagnostic>,
) {
    fn anonymous_aggregate_keyword(node: Node) -> Option<&'static str> {
        let (keyword, body_kind) = match node.kind() {
            "enum_specifier" => ("enum", "enumerator_list"),
            "struct_specifier" => ("struct", "field_declaration_list"),
            "union_specifier" => ("union", "field_declaration_list"),
            _ => return None,
        };
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        // A tag makes the type nameable; a body is what makes this a
        // definition rather than a reference to one declared elsewhere.
        let tagged = children.iter().any(|c| c.kind() == "type_identifier");
        let has_body = children.iter().any(|c| c.kind() == body_kind);
        if !tagged && has_body {
            Some(keyword)
        } else {
            None
        }
    }

    fn walk(
        node: Node,
        src: &str,
        in_method_type: bool,
        diagnostics: &mut Vec<crate::model::Diagnostic>,
    ) {
        if in_method_type {
            if let Some(keyword) = anonymous_aggregate_keyword(node) {
                diagnostics.push(crate::model::Diagnostic::at(
                    format!(
                        "an inline anonymous '{kw}' is not supported as a method return or \
                         parameter type -- it has no tag to name the type by in the generated C -- \
                         declare a named '{kw} Tag {{ ... }}' at file scope and refer to it as \
                         '{kw} Tag' here",
                        kw = keyword
                    ),
                    src,
                    node.start_byte(),
                ));
                return;
            }
        }
        let entering = in_method_type || node.kind() == "method_type";
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, src, entering, diagnostics);
        }
    }

    walk(root, src, false, diagnostics);
}

/// Pass 3: resolve every collected `@property` against its (explicit,
/// bare, or entirely absent) `@synthesize` -- defaulting the backing
/// ivar name, growing `own_ivars` for one that doesn't already exist,
/// and appending a synthesized getter/setter `MethodSig` (skipped if the
/// class already implements that selector by hand) -- so that
/// everything downstream (dispatch classification, prototype/struct
/// emission) sees a synthesized accessor exactly like a hand-written
/// one, without needing to know the difference. Mirrors
/// `tools/oz_transpile/resolve.py`'s `_synthesize_properties`. Also
/// grows the root class's `own_ivars` with a shared `oz_prop_lock` field
/// when any class in the program has an atomic property -- reusing
/// `Program::ivar_access_path`'s existing generic base-chain machinery
/// for every class's lock expression, rather than a bespoke helper.
/// A category `@property` whose accessors are defined nowhere (#530).
///
/// A category cannot add storage to the class it extends, so oz2c cannot
/// synthesize an accessor body for one: there is no field to read. The
/// category's own `@implementation` is the definition, and if it does not
/// provide one then the selector is declared, dispatched to, and defined by
/// nothing.
///
/// This is the diagnostic #530 asks for by name, and it exists because the
/// alternative was not "nothing happens" -- it was a **link** error naming
/// generated C (`undefined reference to 'Sensor_diagnosticCode'`), which is
/// the same complaint both #529 and #530 file under "diagnostic quality:
/// none from oz2c". Without it, fixing #530's double *definition* would have
/// traded one link error for another at the far end of the pipeline.
///
/// Clang warns here rather than erroring ("property 'x' requires method 'x'
/// to be defined -- use @dynamic or provide a method implementation in this
/// category"), and a warning is the right severity for a runtime that can
/// carry a selector nothing implements: the send fails at runtime, on that
/// object, if it is ever made. The generated C has a symbol or it does not,
/// so there is no such deferral available here -- and no non-fatal
/// diagnostic channel to use even if there were.
///
/// `@dynamic` is the half of Clang's advice that does not apply: it promises
/// the accessor arrives at runtime, and nothing here has a runtime.
fn reject_undefined_category_accessors(
    classes: &std::collections::HashMap<String, ClassInfo>,
    class_order: &[String],
    source: &str,
    diagnostics: &mut Vec<crate::model::Diagnostic>,
) {
    for name in class_order {
        let Some(info) = classes.get(name) else {
            continue;
        };
        for prop in &info.properties {
            if prop.origin.may_add_storage() {
                continue;
            }
            let getter_sel = prop.getter_sel.clone().unwrap_or_else(|| prop.name.clone());
            let mut missing: Vec<String> = Vec::new();
            if !info.defined_selectors.contains(&(getter_sel.clone(), false)) {
                missing.push(getter_sel);
            }
            if !prop.is_readonly {
                let setter_sel = prop
                    .setter_sel
                    .clone()
                    .unwrap_or_else(|| default_setter_sel(&prop.name));
                if !info.defined_selectors.contains(&(setter_sel.clone(), false)) {
                    missing.push(setter_sel);
                }
            }
            if missing.is_empty() {
                continue;
            }
            let plural = if missing.len() == 1 { "" } else { "s" };
            diagnostics.push(
                crate::model::Diagnostic::at(
                    format!(
                        "category property '{}' on '{}' declares accessor{} '{}' that no \
                         '@implementation' defines",
                        prop.name,
                        name,
                        plural,
                        missing.join("', '")
                    ),
                    source,
                    prop.decl_offset,
                )
                .with_note(
                    "a category cannot add an instance variable to the class it extends, so \
                     there is no backing storage for an accessor to read and none can be \
                     synthesized -- the category's own '@implementation' has to define it"
                        .to_string(),
                )
                .with_help(format!(
                    "define '{}' in the '@implementation' block for this category",
                    missing.join("' and '")
                ))
                .with_help(format!(
                    "or, if '{}' is meant to have storage, declare the '@property' in the \
                     class's own '@interface' or in a class extension '@interface {} ()', \
                     either of which does get a backing ivar",
                    prop.name, name
                )),
            );
        }
    }
}

fn resolve_properties(classes: &mut std::collections::HashMap<String, ClassInfo>, class_order: &[String]) {
    let mut any_atomic_property = false;

    for name in class_order {
        let existing_ivar_names: HashSet<String> =
            classes[name].own_ivars.iter().map(|(n, _)| n.clone()).collect();
        let mut seen_sels: HashSet<(String, bool)> = classes[name]
            .methods
            .iter()
            .map(|m| (m.selector.clone(), m.is_class_method))
            .collect();

        let props = classes[name].properties.clone();
        let mut resolved_props = Vec::with_capacity(props.len());
        let mut new_ivars = Vec::new();
        let mut new_methods = Vec::new();

        for mut prop in props {
            if !prop.is_nonatomic {
                any_atomic_property = true;
            }
            if prop.ivar_name.is_none() {
                if existing_ivar_names.contains(&prop.name) {
                    // Bare `@synthesize name;` (or no `@synthesize` at
                    // all) with an ivar already declared under the
                    // bare name itself: Python's oracle (`resolve.py`'s
                    // `_synthesize_properties`) accepts this too, only
                    // adding a non-fatal warning diagnostic -- oz2c
                    // has no non-fatal diagnostic channel (see
                    // `lib::transpile`'s doc comment: any diagnostic at
                    // all is a hard error), so matching Python's actual
                    // default (non-`--strict`) behavior means accepting
                    // it silently rather than making it a hard error
                    // Python itself doesn't make it by default.
                    prop.ivar_name = Some(prop.name.clone());
                } else {
                    prop.ivar_name = Some(format!("_{}", prop.name));
                }
            }
            let ivar_name = prop.ivar_name.clone().unwrap();
            /* A category cannot add an ivar to the class it extends, so a
             * category property gets no backing storage -- this is the line
             * that used to put `int _diagnosticCode;` into `struct
             * PXSensorBase` from a declaration in
             * `PXSensorBase+Diagnostics.h`, changing the class's layout from
             * another translation unit (#530).
             *
             * The accessor `MethodSig`s below are synthesized regardless: a
             * category property still *declares* its accessors, and dispatch
             * needs those signatures to route a send or a `.` access to the
             * definition the category's own `@implementation` provides.
             * Only the storage, and the body that would read it, go away.
             *
             * `ivar_name` stays populated rather than being cleared, so
             * nothing downstream has to handle a second kind of `None` --
             * `PropertyOrigin::may_add_storage` is the authority, and the
             * two emit-side sites that would otherwise materialise a field
             * or a body ask it. */
            if prop.origin.may_add_storage()
                && !existing_ivar_names.contains(&ivar_name)
                && !new_ivars.iter().any(|(n, _): &(String, String)| n == &ivar_name)
            {
                new_ivars.push((ivar_name, prop.c_type.clone()));
            }

            let getter_sel = prop.getter_sel.clone().unwrap_or_else(|| prop.name.clone());
            if seen_sels.insert((getter_sel.clone(), false)) {
                new_methods.push(MethodSig {
                    is_class_method: false,
                    selector: getter_sel,
                    return_type: prop.c_type.clone(),
                    params: Vec::new(),
                    returns_instancetype: false,
                    /* A synthesized accessor on a class: no protocol
                     * marker can reach it. */
                    is_optional: false,
                });
            }
            if !prop.is_readonly {
                let setter_sel = prop.setter_sel.clone().unwrap_or_else(|| default_setter_sel(&prop.name));
                if seen_sels.insert((setter_sel.clone(), false)) {
                    new_methods.push(MethodSig {
                        is_class_method: false,
                        selector: setter_sel,
                        return_type: "void".to_string(),
                        params: vec![(prop.name.clone(), prop.c_type.clone())],
                        returns_instancetype: false,
                        is_optional: false,
                    });
                }
            }
            resolved_props.push(prop);
        }

        let info = classes.get_mut(name).unwrap();
        // A non-strong property does not own what its ivar points at, so
        // the ivar joins the do-not-release set alongside the ones declared
        // `__unsafe_unretained` directly.
        for prop in &resolved_props {
            /* Only where the ivar exists: a category property's
             * `ivar_name` names no field of the struct, so recording it
             * here would put a phantom name in the do-not-release set --
             * harmless today, and exactly the sort of entry a later reader
             * would take as evidence that the field is real. */
            if prop.ownership != Ownership::Strong && prop.origin.may_add_storage() {
                if let Some(ivar) = &prop.ivar_name {
                    info.unretained_ivars.insert(ivar.clone());
                }
            }
        }
        info.properties = resolved_props;
        info.own_ivars.extend(new_ivars);
        info.methods.extend(new_methods);
    }

    if any_atomic_property {
        let root = class_order.iter().find(|n| classes[*n].superclass.is_none()).cloned();
        if let Some(root) = root {
            let info = classes.get_mut(&root).unwrap();
            if !info.own_ivars.iter().any(|(n, _)| n == "oz_prop_lock") {
                info.own_ivars.push(("oz_prop_lock".to_string(), "oz_spinlock_t".to_string()));
            }
        }
    }
}
