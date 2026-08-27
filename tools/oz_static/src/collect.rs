// SPDX-License-Identifier: Apache-2.0
//
// collect.rs - CST -> Program symbol table (classes, ivars, method
// signatures). Two sub-passes: class names/hierarchy first, then
// ivars/methods (which need the class-name set to render object types).

use std::collections::HashSet;

use tree_sitter::Node;

use crate::model::{ClassInfo, MethodSig, Program};

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

pub(crate) fn render_type(type_text: &str, stars: usize, known_classes: &HashSet<String>) -> String {
    if type_text == "id" {
        return "id".to_string();
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
            "primitive_type" | "type_identifier" | "typedefed_specifier" => {
                if type_text.is_empty() {
                    *type_text = node_text(n, src).to_string();
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

fn extract_ivars(node: Node, src: &str, known_classes: &HashSet<String>) -> Vec<(String, String)> {
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
    let mut selector = String::new();
    let mut params = Vec::new();

    while let Some(child) = children.next() {
        match child.kind() {
            "method_type" if selector.is_empty() => {
                let (t, stars) = extract_type_and_stars(child, src);
                return_type = if t == "instancetype" {
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
                let (t, stars) = extract_type_and_stars(child, src);
                let param_type = if t == "instancetype" {
                    format!("struct {} *", self_class)
                } else {
                    render_type(&t, stars, known_classes)
                };
                let param_name = child_by_kind(child, "identifier")
                    .map(|n| node_text(n, src).to_string())
                    .unwrap_or_default();
                params.push((param_name, param_type));
            }
            _ => {}
        }
    }

    MethodSig { is_class_method, selector, return_type, params }
}

pub fn collect(source: &str) -> (Program, Vec<crate::model::Diagnostic>) {
    let tree = crate::parse::parse(source);
    let root = tree.root_node();

    // Pass 1: class names + hierarchy + category associations.
    let mut classes: std::collections::HashMap<String, ClassInfo> = std::collections::HashMap::new();
    let mut class_order = Vec::new();
    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        if node.kind() != "class_interface" && node.kind() != "class_implementation" {
            continue;
        }
        let (name, superclass, category) = class_header(node, source);
        if category.is_some() {
            continue; // category: doesn't declare a new class
        }
        if !classes.contains_key(&name) {
            classes.insert(
                name.clone(),
                ClassInfo { name: name.clone(), superclass, ..Default::default() },
            );
            class_order.push(name);
        } else if let Some(sup) = superclass {
            classes.get_mut(&name).unwrap().superclass = Some(sup);
        }
    }

    let known_classes: HashSet<String> = classes.keys().cloned().collect();

    // Pass 2: ivars (from interfaces) + method signatures (from
    // declarations and definitions, category included).
    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        match node.kind() {
            "class_interface" => {
                let (name, _, category) = class_header(node, source);
                if category.is_some() {
                    continue;
                }
                let ivars = extract_ivars(node, source, &known_classes);
                if let Some(info) = classes.get_mut(&name) {
                    info.own_ivars = ivars;
                }
                let mut c = node.walk();
                for decl in node.children(&mut c) {
                    if decl.kind() == "method_declaration" {
                        let sig = extract_method_sig(decl, source, &name, &known_classes);
                        if let Some(info) = classes.get_mut(&name) {
                            info.methods.push(sig);
                        }
                    }
                }
            }
            "class_implementation" => {
                let (name, _, _category) = class_header(node, source);
                let Some(info) = classes.get_mut(&name) else { continue };
                let mut c = node.walk();
                for impl_def in node.children(&mut c) {
                    if impl_def.kind() != "implementation_definition" {
                        continue;
                    }
                    let Some(method_def) = child_by_kind(impl_def, "method_definition") else {
                        continue;
                    };
                    let sig = extract_method_sig(method_def, source, &name, &known_classes);
                    if sig.is_class_method && sig.selector == "initialize" {
                        info.has_class_initialize = true;
                    }
                    if !info.methods.iter().any(|m| {
                        m.selector == sig.selector && m.is_class_method == sig.is_class_method
                    }) {
                        info.methods.push(sig);
                    }
                }
            }
            _ => {}
        }
    }

    let diagnostics = Vec::new();
    (Program { classes, class_order }, diagnostics)
}
