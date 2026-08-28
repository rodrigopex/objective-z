// SPDX-License-Identifier: Apache-2.0
//
// emit.rs - in-place textual substitution emitter.
//
// Only ObjC-specific syntax spans (interface/implementation wrappers,
// method headers, message sends, non-capturing blocks) are replaced at
// their original byte position; everything else is byte-identical to the
// input. Multi-implementor dispatch (dealloc's const-vtable) and pool
// registration are isolated into one small companion file, mirroring the
// existing oz_dispatch.c/h pattern.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::model::{Diagnostic, Program};
use crate::parse::line_col;

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.start_byte()..node.end_byte()]
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

fn selector_to_c(selector: &str) -> String {
    selector.replace(':', "_")
}

/// Class methods get a `_cls` suffix so `+foo` and `-foo` on the same
/// class never collide on the same C function name.
fn method_fn_name(class_name: &str, selector: &str, is_class_method: bool) -> String {
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

fn method_return_type(
    program: &Program,
    class_name: &str,
    selector: &str,
    is_class_method: bool,
) -> Option<String> {
    program.classes.get(class_name)?.methods.iter().find(|m| {
        m.selector == selector && m.is_class_method == is_class_method
    }).map(|m| m.return_type.clone())
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
    hoisted_blocks: Vec<String>,
    hoisted_structs: Vec<(String, String)>,
    block_counter: usize,
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
    if matches!(node.kind(), "message_expression" | "block_literal" | "type_identifier" | "identifier") {
        return true;
    }
    let mut cursor = node.walk();
    let any_child = node.children(&mut cursor).any(needs_translation);
    any_child
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
    if parts.selector == "alloc" && parts.args.is_empty() {
        if let Some(cls) = recv_type.strip_prefix("class:") {
            return (format!("{}_oz_alloc()", cls), format!("struct {} *", cls));
        }
    }

    if let Some(target) = recv_type.strip_prefix("class:") {
        let target = target.to_string();
        return match find_defining_class(ctx.program, &target, &parts.selector, true) {
            Some(defining) => {
                let ret_ty = method_return_type(ctx.program, &defining, &parts.selector, true)
                    .unwrap_or_else(|| "void".to_string());
                (
                    format!(
                        "{}({})",
                        method_fn_name(&defining, &parts.selector, true),
                        arg_texts.join(", ")
                    ),
                    ret_ty,
                )
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
            Some(defining) => {
                let ret_ty = method_return_type(ctx.program, &defining, &parts.selector, false)
                    .unwrap_or_else(|| "void".to_string());
                let mut call_args = vec![format!("(struct {} *)({})", defining, recv_text)];
                call_args.extend(arg_texts);
                (
                    format!(
                        "{}({})",
                        method_fn_name(&defining, &parts.selector, false),
                        call_args.join(", ")
                    ),
                    ret_ty,
                )
            }
            None => {
                ctx.err(node, format!("class '{}' has no method matching '{}'", target, parts.selector));
                ("0".to_string(), "int".to_string())
            }
        },
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
    let body_text = match body {
        Some(body) => {
            // Block bodies use the same flat scope as their enclosing
            // method/function (a known spike simplification).
            collect_local_decls(body, ctx);
            render_expr(body, ctx).0
        }
        None => "{ }".to_string(),
    };

    ctx.hoisted_blocks.push(format!("static void {}{} {}\n", name, params, body_text));
    (name.clone(), "id".to_string())
}

/// One top-level `class_interface` (non-category) block. The struct
/// layout is hoisted into the companion header (`ctx.hoisted_structs`) --
/// both this class's own method definitions AND the companion's alloc/
/// dealloc-dispatch code need the full definition, so it can't live only
/// in this translation unit's in-place output. The prototypes stay
/// in-place at the interface's original source location.
fn render_interface(node: Node, ctx: &mut EmitCtx, program: &Program) -> String {
    let name = ctx.class_name.clone();
    let info = &program.classes[&name];

    let mut cursor0 = node.walk();
    for child in node.children(&mut cursor0) {
        if child.kind() == "property_declaration" {
            ctx.err(
                child,
                "@property is not supported in the static subset spike; declare an ivar and accessor methods explicitly",
            );
        }
    }
    let base_field = match &info.superclass {
        Some(sup) => format!("\tstruct {} base;\n", sup),
        // Root class: synthesize the tracking fields every object needs
        // (const-vtable dealloc dispatch reads oz_class_id; retain/release
        // use oz_refcount/oz_deallocating) instead of a `base` member.
        None => "\tuint8_t oz_class_id;\n\toz_atomic_t oz_refcount;\n\tuint8_t oz_deallocating;\n"
            .to_string(),
    };

    let mut ivars_text = String::new();
    let mut cursor = node.walk();
    if let Some(vars_node) = node.children(&mut cursor).find(|c| c.kind() == "instance_variables") {
        let mut c2 = vars_node.walk();
        for child in vars_node.children(&mut c2) {
            if child.kind() == "instance_variable" {
                ivars_text.push('\t');
                ivars_text.push_str(node_text(child, ctx.src));
                ivars_text.push('\n');
            }
        }
    }

    let struct_text =
        format!("struct {name} {{\n{base}{ivars}}};\n", name = name, base = base_field, ivars = ivars_text);

    let mut protos = String::new();
    for m in &info.methods {
        protos.push_str(&render_prototype(&name, m));
    }

    if info.superclass.is_none() {
        // Root: full struct hoisted to the companion, since
        // oz_static_retain/release/the dealloc switch need its tracking
        // fields directly. Its alloc/free live there too.
        ctx.hoisted_structs.push((name.clone(), struct_text));
        protos
    } else {
        // Every other class: struct + alloc/free (which need the full
        // struct for sizeof) stay in-place, next to this class's own
        // methods. The companion only forward-declares the type.
        let root = program.root_class().unwrap_or(&name).to_string();
        format!("{}\n{}\n{}", struct_text, crate::companion::render_alloc_free(&name, &root), protos)
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
        params.push_str(&format!("{} {}", ptype, pname));
    }
    if params.is_empty() {
        params = "void".to_string();
    }
    let fn_name = method_fn_name(class_name, &m.selector, m.is_class_method);
    format!("{} {}({});\n", m.return_type, fn_name, params)
}

/// One category `class_interface (Category)` block -> just prototypes.
fn render_category_interface(name: &str, program: &Program) -> String {
    let info = &program.classes[name];
    let mut protos = String::new();
    for m in &info.methods {
        protos.push_str(&render_prototype(name, m));
    }
    protos
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
        .unwrap_or_else(|| sig.return_type.clone());

    let mut sig_params = String::new();
    if !sig.is_class_method {
        sig_params.push_str(&format!("struct {} *self", class_name));
    }
    for (pname, ptype) in &sig.params {
        if !sig_params.is_empty() {
            sig_params.push_str(", ");
        }
        sig_params.push_str(&format!("{} {}", ptype, pname));
    }
    if sig_params.is_empty() {
        sig_params = "void".to_string();
    }
    let fn_name = method_fn_name(class_name, &sig.selector, sig.is_class_method);

    let mut cursor = node.walk();
    let body = node.children(&mut cursor).find(|c| c.kind() == "compound_statement");

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
                render_expr(body, ctx).0
            }
        }
        None => "{ }".to_string(),
    };

    format!("{} {}({})\n{}\n", ret_ty, fn_name, sig_params, body_text)
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
    let mut hoisted_blocks: Vec<String> = Vec::new();
    let mut hoisted_structs: Vec<(String, String)> = Vec::new();

    struct Patch {
        start: usize,
        end: usize,
        text: String,
    }
    let mut patches: Vec<Patch> = Vec::new();

    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        match node.kind() {
            "class_interface" => {
                let (name, _, category) = crate::collect::class_header(node, source);
                if let Some(cat) = category {
                    let _ = cat;
                    let text = render_category_interface(&name, program);
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
                    block_counter: 0,
                };
                let text = render_interface(node, &mut ctx, program);
                diags.extend(ctx.diags);
                hoisted_structs.extend(ctx.hoisted_structs);
                patches.push(Patch { start: node.start_byte(), end: node.end_byte(), text });
            }
            "class_implementation" => {
                let (name, _, _category) = crate::collect::class_header(node, source);
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
                    block_counter: 0,
                };
                let mut out = String::new();
                out.push_str(&format!("/* @implementation {} */\n\n", name));
                let mut c2 = node.walk();
                for child in node.children(&mut c2) {
                    if child.kind() != "implementation_definition" {
                        continue;
                    }
                    let mut c3 = child.walk();
                    let found_def = child.children(&mut c3).find(|c| c.kind() == "method_definition");
                    match found_def {
                        Some(method_def) => {
                            out.push_str(&render_method_definition(
                                method_def, &mut ctx, &name, &ivars_scope,
                            ));
                            out.push('\n');
                        }
                        None => {
                            let mut c4 = child.walk();
                            let is_synthesize = child
                                .children(&mut c4)
                                .any(|c| c.kind() == "property_implementation");
                            if is_synthesize {
                                ctx.err(
                                    child,
                                    "@synthesize is not supported in the static subset spike; declare an ivar and accessor methods explicitly",
                                );
                                continue;
                            }
                            // Not a method: e.g. a `static Foo *g;` file-scope
                            // declaration written directly inside
                            // @implementation. Copy through (translating any
                            // message send it happens to contain) instead of
                            // silently dropping it.
                            ctx.scope = ivars_scope.clone();
                            if needs_translation(child) {
                                out.push_str(&render_expr(child, &mut ctx).0);
                            } else {
                                out.push_str(node_text(child, ctx.src));
                            }
                            out.push('\n');
                        }
                    }
                }
                diags.extend(ctx.diags);
                hoisted_blocks.extend(ctx.hoisted_blocks);
                hoisted_structs.extend(ctx.hoisted_structs);
                patches.push(Patch { start: node.start_byte(), end: node.end_byte(), text: out });
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
                    block_counter: 0,
                };
                let mut c2 = node.walk();
                if let Some(body) =
                    node.children(&mut c2).find(|c| c.kind() == "compound_statement")
                {
                    if needs_translation(body) {
                        collect_local_decls(body, &mut ctx);
                        let (text, _) = render_expr(body, &mut ctx);
                        patches.push(Patch {
                            start: body.start_byte(),
                            end: body.end_byte(),
                            text,
                        });
                    }
                }
                diags.extend(ctx.diags);
                hoisted_blocks.extend(ctx.hoisted_blocks);
                hoisted_structs.extend(ctx.hoisted_structs);
            }
            _ => {}
        }
    }

    patches.sort_by(|a, b| b.start.cmp(&a.start));
    let mut out = source.to_string();
    for p in &patches {
        out.replace_range(p.start..p.end, &p.text);
    }
    out = format!(
        "/* Auto-generated by oz_static -- do not edit */\n#include \"oz_static_dispatch.h\"\n\n{}",
        out
    );

    let (companion_h, companion_c) = crate::companion::render(program, &hoisted_blocks, &hoisted_structs);

    EmitOutput { source_c: out, companion_h, companion_c, diagnostics: diags }
}

fn base_scope(class_name: &str, program: &Program) -> HashMap<String, String> {
    program.all_ivars(class_name).into_iter().collect()
}
