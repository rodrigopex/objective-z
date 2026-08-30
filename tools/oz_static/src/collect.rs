// SPDX-License-Identifier: Apache-2.0
//
// collect.rs - CST -> Program symbol table (classes, ivars, method
// signatures). Two sub-passes: class names/hierarchy first, then
// ivars/methods (which need the class-name set to render object types).

use std::collections::HashSet;

use tree_sitter::Node;

use crate::model::{ClassInfo, MethodSig, Ownership, Program, PropertyInfo, ProtocolInfo};

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.start_byte()..node.end_byte()]
}

fn child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).find(|c| c.kind() == kind);
    found
}

/// class_interface / class_implementation share the shape:
/// [@interface|@implementation] identifier [: identifier]? [( identifier )]?
pub(crate) fn class_header(node: Node, src: &str) -> (String, Option<String>, Option<String>) {
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
    (name, superclass, category)
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
/// OZObject <IteratorProtocol>`) has TWO `parameterized_arguments`
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
    collect_protocol_methods(node, src, &name, known_classes, &mut methods);
    ProtocolInfo { name, super_protocols, methods }
}

/// `method_declaration`s directly inside a protocol body, or nested one
/// level inside a `@required`/`@optional`-qualified sub-block
/// (`qualified_protocol_interface_declaration`) -- tree-sitter-objc
/// wraps everything after such a marker in its own node, so a flat
/// direct-children scan misses them entirely. Required-vs-optional
/// isn't tracked either way: protocols are a compile-time contract
/// here, not a runtime filter (see `model::Program::all_protocol_methods`'s
/// doc comment), so nothing downstream cares about the distinction.
fn collect_protocol_methods(
    node: Node,
    src: &str,
    protocol_name: &str,
    known_classes: &HashSet<String>,
    out: &mut Vec<MethodSig>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "method_declaration" => out.push(extract_method_sig(child, src, protocol_name, known_classes)),
            "qualified_protocol_interface_declaration" => {
                collect_protocol_methods(child, src, protocol_name, known_classes, out)
            }
            _ => {}
        }
    }
}

pub(crate) fn render_type(type_text: &str, stars: usize, known_classes: &HashSet<String>) -> String {
    if type_text == "id" {
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
    let mut type_text = String::new();
    let mut stars = 0;
    let mut cursor = node.walk();
    fn walk(n: Node, src: &str, type_text: &mut String, stars: &mut usize) {
        match n.kind() {
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
                // `Container<Arg, ...>` (e.g. `OZArray<OZQ31 *>`): this
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
            "*" => *stars += 1,
            _ => {
                let mut c = n.walk();
                for child in n.children(&mut c) {
                    walk(child, src, type_text, stars);
                }
            }
        }
    }
    for child in node.children(&mut cursor) {
        walk(child, src, &mut type_text, &mut stars);
    }
    (type_text, stars)
}

pub(crate) fn extract_ivars(node: Node, src: &str, known_classes: &HashSet<String>) -> Vec<(String, String)> {
    let Some(vars_node) = child_by_kind(node, "instance_variables") else {
        return Vec::new();
    };
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
        out.push((name, render_type(&type_text, stars, known_classes)));
    }
    out
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
    let (decl_line, decl_col) = crate::parse::line_col(src, node.start_byte());

    if is_weak {
        diagnostics.push(crate::model::Diagnostic::new(
            format!("'weak' property '{}' is not supported; use 'unsafe_unretained' instead", name),
            decl_line,
            decl_col,
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
        decl_line,
        decl_col,
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
        .map(|p| node_text(p, src).to_string())
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

    MethodSig { is_class_method, selector, return_type, params, returns_instancetype }
}

pub fn collect(source: &str) -> (Program, Vec<crate::model::Diagnostic>) {
    let tree = crate::parse::parse(source);
    let root = tree.root_node();

    // Pass 1: class names + hierarchy + category associations, and
    // protocol declarations (name, inheritance, own methods).
    let mut classes: std::collections::HashMap<String, ClassInfo> = std::collections::HashMap::new();
    let mut class_order = Vec::new();
    let mut protocols: std::collections::HashMap<String, ProtocolInfo> = std::collections::HashMap::new();
    // First-seen (line, col) per class, kept only for the
    // superclass-resolution diagnostic below -- not part of `ClassInfo`
    // itself, since nothing downstream needs it.
    let mut first_seen: std::collections::HashMap<String, (usize, usize)> = std::collections::HashMap::new();
    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        if node.kind() == "protocol_declaration" {
            // Protocol methods don't reference class types in these
            // fixtures; an empty known-class set is fine here since
            // conformance/dispatch resolution happens later via selector
            // matching, not through this parse.
            let known: HashSet<String> = HashSet::new();
            let info = extract_protocol(node, source, &known);
            protocols.insert(info.name.clone(), info);
            continue;
        }
        if node.kind() != "class_interface" && node.kind() != "class_implementation" {
            continue;
        }
        let (name, superclass, category) = class_header(node, source);
        if category.is_some() {
            continue; // category: doesn't declare a new class
        }
        if !classes.contains_key(&name) {
            first_seen.insert(name.clone(), crate::parse::line_col(source, node.start_byte()));
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

    // A `superclass` reference that doesn't resolve to a class actually
    // collected above (e.g. a real Foundation class only ever pulled in
    // via `#import <Foundation/Foundation.h>` -- oz_static has no import
    // resolution of its own, so it's genuinely undefined in this
    // translation unit) must be a named, located hard error here, not a
    // downstream panic: every later pass -- `companion::topological_order`
    // in particular -- assumes every `superclass` string is itself a key
    // in `classes`, and indexes it directly (`program.classes[name]`,
    // which panics on a miss) rather than through a fallible lookup.
    for name in &class_order {
        let Some(sup) = &classes[name].superclass else { continue };
        if !known_classes.contains(sup) {
            let (line, col) = first_seen[name];
            diagnostics.push(crate::model::Diagnostic::new(
                format!(
                    "class '{}' extends '{}', but no class '{}' is defined in this source \
(oz_static has no #import resolution -- provide a single, self-contained translation unit)",
                    name, sup, sup
                ),
                line,
                col,
            ));
        }
    }

    // Pass 2: ivars (from interfaces) + method signatures (from
    // declarations and definitions, category included).
    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        match node.kind() {
            "class_interface" => {
                let (name, _, category) = class_header(node, source);
                // A category's methods and properties merge into the
                // class it extends (mirroring the oracle's
                // `collect.py::_merge_category`); its ivars do not,
                // because ObjC categories cannot declare any. A category
                // may restate a selector the main @interface already
                // declared, so pushes from here are deduplicated -- the
                // main interface has no such risk, nothing else declares
                // its selectors before it.
                let is_category = category.is_some();
                if !is_category {
                    let ivars = extract_ivars(node, source, &known_classes);
                    if let Some(info) = classes.get_mut(&name) {
                        info.own_ivars = ivars;
                    }
                }
                let mut c = node.walk();
                for decl in node.children(&mut c) {
                    if decl.kind() == "method_declaration" {
                        let sig = extract_method_sig(decl, source, &name, &known_classes);
                        if let Some(info) = classes.get_mut(&name) {
                            let dup = is_category
                                && info.methods.iter().any(|m| {
                                    m.selector == sig.selector
                                        && m.is_class_method == sig.is_class_method
                                });
                            if !dup {
                                info.methods.push(sig);
                            }
                        }
                    } else if decl.kind() == "property_declaration" {
                        if let Some(prop) =
                            extract_property(decl, source, &known_classes, &mut diagnostics)
                        {
                            if let Some(info) = classes.get_mut(&name) {
                                let dup = is_category
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
                let (name, _, _category) = class_header(node, source);
                if !classes.contains_key(&name) {
                    continue;
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
                        if !info.methods.iter().any(|m| {
                            m.selector == sig.selector && m.is_class_method == sig.is_class_method
                        }) {
                            info.methods.push(sig);
                        }
                        continue;
                    }
                    if let Some(prop_impl) = child_by_kind(impl_def, "property_implementation") {
                        let (prop_name, ivar) = extract_synthesize(prop_impl, source);
                        let info = classes.get_mut(&name).unwrap();
                        let matched = info.properties.iter_mut().find(|p| p.name == prop_name);
                        match matched {
                            Some(prop) => {
                                if let Some(iv) = ivar {
                                    prop.ivar_name = Some(iv);
                                }
                            }
                            None => {
                                let (line, col) = crate::parse::line_col(source, prop_impl.start_byte());
                                diagnostics.push(crate::model::Diagnostic::new(
                                    format!(
                                        "'@synthesize {}' but no '@property {}' is declared on '{}'",
                                        prop_name, prop_name, name
                                    ),
                                    line,
                                    col,
                                ));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    resolve_properties(&mut classes, &class_order);

    reject_inline_anonymous_aggregates(root, source, &mut diagnostics);

    (Program { classes, class_order, protocols }, diagnostics)
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
                let (line, col) = crate::parse::line_col(src, node.start_byte());
                diagnostics.push(crate::model::Diagnostic::new(
                    format!(
                        "an inline anonymous '{kw}' is not supported as a method return or \
                         parameter type -- it has no tag to name the type by in the generated C -- \
                         declare a named '{kw} Tag {{ ... }}' at file scope and refer to it as \
                         '{kw} Tag' here",
                        kw = keyword
                    ),
                    line,
                    col,
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
                    // adding a non-fatal warning diagnostic -- oz_static
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
            if !existing_ivar_names.contains(&ivar_name)
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
                    });
                }
            }
            resolved_props.push(prop);
        }

        let info = classes.get_mut(name).unwrap();
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
