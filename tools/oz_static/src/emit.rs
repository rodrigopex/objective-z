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
//
// The substitution is per *construct*: `rebuild` and `apply_edits` replace
// spans inside one top-level node and copy the gaps between them verbatim.
// The top level itself is assembled from what `walk_top_level` buckets, not
// patched over the whole file -- `emit()` did once work that way, and its
// doing so is how it managed to disagree with `emit_split()` four times
// (#254): anything no arm claimed simply survived, so a missing arm produced
// no error and no output difference until a C compiler saw it.
//
// Several passes may contribute edits for the same construct, and they must
// be *disjoint*: a pass that replaces a subtree owns its whole byte range.
// Two overlapping edits truncate each other and produce text no C compiler
// accepts, which is #331; `apply_edits` states the rule and asserts it.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use tree_sitter::Node;

use crate::model::{Diagnostic, Program};
use crate::parse::line_col;

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.start_byte()..node.end_byte()]
}

/// The `#line` policy for one emit run: whether directives are emitted at
/// all, and what they name (#305).
///
/// Without them every artefact carrying a source position -- a `gdb`
/// breakpoint, a Zephyr fatal-error backtrace, `addr2line`, a coverage
/// report -- points at `oz_static_generated/<Class>.c`, the only file the
/// C compiler is ever handed, and translating that back to the `.m` is
/// done by eye against files that are neither the same length nor skewed
/// by a constant.
///
/// **A `None` map is the off switch**, and the only one: a caller with no
/// map (`transpile()`, whose source is a string with no file behind it, and
/// `oz2c` without `--line-directives`) emits exactly the bytes it emitted
/// before this existed. Nothing else in here changes output.
///
/// Two directions to point in, both needed:
///
///   - `at`, for code the author wrote -- a method body statement, a
///     hoisted block. Resolved by byte offset through
///     `imports::SourceMap`, *never* by counting newlines in the text
///     `emit` walks: `parse::repair_bare_macro_statements` preserves every
///     offset but eats a newline per repair, so the repaired text has
///     fewer lines than the buffer the map describes while the offsets
///     still agree. Counting locally is the exact bug #305 was filed for.
///   - `reset`, for code oz_static synthesized -- a slab definition, a
///     dispatch thunk, a hoisted prototype. That code genuinely lives in
///     the generated `.c`, and without a directive handing attribution
///     back it would inherit whatever `.m` line preceded it.
#[derive(Default)]
pub struct LineDirectives<'a> {
    map: Option<&'a crate::imports::SourceMap>,
    /// Directory each origin stem's generated `.h`/`.c` pair is written
    /// to, so `reset` can name that file absolutely. Empty (or missing a
    /// stem) falls back to the bare `<stem>.c`, which is all a caller that
    /// never writes the pair to disk can honestly say.
    dirs: Option<&'a HashMap<String, PathBuf>>,
    /// Resolved once, to absolutize a relative source path -- a debugger
    /// has to find the file from whatever directory it is run in, and the
    /// path in the map is whatever the caller passed on the command line.
    /// Not canonicalized: that is a `stat` per component, and a `..` or a
    /// symlink in the path resolves fine for every consumer of a `#line`.
    cwd: Option<PathBuf>,
}

impl<'a> LineDirectives<'a> {
    pub fn new(
        map: Option<&'a crate::imports::SourceMap>,
        dirs: Option<&'a HashMap<String, PathBuf>>,
    ) -> Self {
        LineDirectives {
            map,
            dirs,
            cwd: map.and_then(|_| std::env::current_dir().ok()),
        }
    }

    /// A directive putting the *next* emitted line at the `.m`/`.h` line
    /// the byte at `merged_offset` was spliced from, or `None` when
    /// directives are off or the offset is not covered by the map.
    ///
    /// Includes its own trailing newline, so a caller splices it in ahead
    /// of a line and nothing else moves.
    fn at(&self, merged_offset: usize) -> Option<String> {
        let (file, line) = self.map?.source_location(merged_offset)?;
        Some(format!("#line {} \"{}\"\n", line, self.quoted(file)))
    }

    /// `at`, or an empty string -- for the many call sites that splice the
    /// directive into a `format!` and must produce their old bytes exactly
    /// when directives are off.
    fn before(&self, merged_offset: usize) -> String {
        self.at(merged_offset).unwrap_or_default()
    }

    /// The (line, column) of the source file the byte at `merged_offset`
    /// was written at. `None` when directives are off, in which case a
    /// caller naming a symbol after a position keeps the merged-buffer
    /// position it always used.
    fn position(&self, merged_offset: usize) -> Option<(usize, usize)> {
        self.map?.source_position(merged_offset).map(|(_, line, col)| (line, col))
    }

    /// A directive re-anchoring a **verbatim** body after something was
    /// spliced in behind its opening brace.
    ///
    /// A verbatim body is attributed by one directive on the brace and
    /// then by nothing: its lines are the source's lines, so they follow
    /// on their own. Inserting an unused-parameter acknowledgement
    /// (`(void)self;`) breaks that -- every line after it is one late, per
    /// line inserted, which is a whole body silently misattributed. So the
    /// splice re-states the position it interrupted: the line after the
    /// brace's, or the brace's own line when the body is written on one
    /// line and the statement really does share it.
    ///
    /// `brace` is the body's own start offset and `end` its end, both into
    /// the text `emit` walks -- and both offsets, never lines, for the
    /// usual reason.
    fn resume_after_brace(&self, src: &str, brace: usize, end: usize) -> String {
        if self.map.is_none() {
            return String::new();
        }
        match src.get(brace..end).and_then(|body| body.find('\n')) {
            Some(rel) => self.before(brace + rel + 1),
            None => self.before(brace),
        }
    }

    /// Hand attribution back to the generated file itself, for the
    /// synthesized code about to be appended to `out`.
    ///
    /// The line number is counted from `out` as it stands: the directive
    /// occupies the line it is written on, so what follows is the line
    /// after it. That is only correct because the whole file is assembled
    /// front to back, which it is -- every caller appends.
    fn reset(&self, out: &mut String, stem: &str, extension: &str) {
        if self.map.is_none() {
            return;
        }
        let generated = match self.dirs.and_then(|d| d.get(stem)) {
            Some(dir) => dir.join(format!("{}.{}", stem, extension)),
            None => PathBuf::from(format!("{}.{}", stem, extension)),
        };
        /* Lines already written, then the directive's own line, then the
         * line the next byte lands on. */
        let written = out.bytes().filter(|b| *b == b'\n').count();
        out.push_str(&format!(
            "#line {} \"{}\"\n",
            written + 2,
            self.quoted(&generated)
        ));
    }

    /// `path` as a directive's file field: absolute, and with the two
    /// characters a C string cannot carry raw escaped.
    fn quoted(&self, path: &Path) -> String {
        let absolute = match (path.is_absolute(), &self.cwd) {
            (false, Some(cwd)) => cwd.join(path),
            _ => path.to_path_buf(),
        };
        absolute.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"")
    }
}

/// Collapse whitespace (including newlines) into single spaces, for a
/// readable one-line `/* ... */` comment out of a possibly multi-line or
/// oddly-indented original statement. Every caller wraps the result in
/// `/* ... */`, so any embedded comment delimiter in the original text (a
/// real inline comment, or even a string literal containing those two
/// characters) is neutralized here -- C block comments don't nest.
fn one_line(text: &str) -> String {
    neutralize_comment_delimiters(&text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Make `text` safe to place inside a `/* ... */` block comment.
///
/// Both delimiters have to go, not just the closing one. `*/` left as-is
/// closes the wrapping comment early and hands the rest to the compiler as
/// live code -- the obvious hazard, and the only one this used to handle.
/// But a surviving `/*` is a diagnostic in its own right: Clang and GCC both
/// warn "'/*' within block comment" under `-Wall`, and Zephyr builds with
/// `-Werror`, so echoing a source comment into a banner was enough to fail
/// the build. 36 of those came from `OZQ31.h`'s ivar doc comments alone.
fn neutralize_comment_delimiters(text: &str) -> String {
    text.replace("*/", "* /").replace("/*", "/ *")
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
/// parsed as live C, and a surviving `/*` warns under `-Wall`. Both
/// delimiters are neutralized before wrapping -- see
/// `neutralize_comment_delimiters`; cosmetic only, since this text is
/// documentation either way.
fn banner_box(content: &str, fill: char) -> String {
    let content = neutralize_comment_delimiters(content.trim_end());
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
/// Build the scope table for `body`, then decide which of its object locals
/// ARC manages as strong variables. Both passes are driven from here so that
/// every caller -- method bodies and the two plain-C-function arms alike --
/// gets the second one; an earlier shape had the strong-local decision at
/// each call site, where a new call site would silently miss it.
fn collect_local_decls(body: Node, ctx: &mut EmitCtx) {
    collect_local_decls_inner(body, ctx);
    let managed = managed_object_locals(body, ctx.src, ctx.program);
    /* A `static` object local is a strong slot, not a strong local: it is
     * stored into the same way and released at scope exit never (#359). */
    let statics = static_object_locals(body, ctx.src, ctx.program);
    ctx.arc_managed_locals.extend(managed.difference(&statics).cloned());
    ctx.arc_managed_slots.extend(statics);
}

fn collect_local_decls_inner(node: Node, ctx: &mut EmitCtx) {
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
            // `pointer_declarator` is the shape a declaration with *no*
            // initializer takes -- `Counter *c;` parses as
            // `type_identifier` + `pointer_declarator(* identifier)`, with
            // no `init_declarator` anywhere. Without it such a local never
            // reached `ctx.scope`, so a later `[c poke]` reported its
            // receiver as `id` and was rejected, while the identical code
            // written `Counter *c = ...;` resolved fine. That is the
            // local-scope twin of the file-scope gap fixed by
            // `emit::file_scope_vars`, and it also has to be fixed here for
            // ARC to recognise a strong local declared before the loop that
            // assigns it (see `arc_managed_locals`).
            if child.kind() == "init_declarator"
                || child.kind() == "identifier"
                || child.kind() == "pointer_declarator"
            {
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
        collect_local_decls_inner(child, ctx);
    }
}

/// Is `decl` a declaration of exactly one strong local, written with no
/// initializer? Those need ARC's implicit `= nil` -- see the `"declaration"`
/// arm in `render_expr`.
///
/// The single-declarator requirement is not cosmetic: the initializer is
/// spliced in before the declaration's trailing `;`, which on
/// `Counter *a, *b;` would initialize only `b` and leave `a` indeterminate
/// -- exactly the pointer the first overwrite would then release. A
/// multi-declarator line is therefore left alone rather than half-handled,
/// which also keeps it out of `arc_managed_locals`' release paths (see
/// `owned_locals_of_in`, which is reached through the same declarator kind).
fn declares_bare_managed_local(decl: Node, ctx: &EmitCtx) -> bool {
    let mut cursor = decl.walk();
    let declarators: Vec<Node> = decl
        .children(&mut cursor)
        .filter(|c| {
            matches!(
                c.kind(),
                "pointer_declarator" | "init_declarator" | "array_declarator" | "identifier"
            )
        })
        .collect();
    if declarators.len() != 1 || declarators[0].kind() != "pointer_declarator" {
        return false;
    }
    let name = crate::collect::find_declared_name(declarators[0], ctx.src);
    !name.is_empty() && ctx.arc_managed_locals.contains(&name)
}

/// Is this initializer just "nothing yet" -- `nil`, `NULL` or `0`?
///
/// `Foo *f = nil;` followed by real assignments is an everyday Objective-C
/// idiom and means exactly what a bare `Foo *f;` means: the variable starts
/// empty. Both must be treated the same, or the explicit spelling would
/// silently lose ARC while the implicit one kept it. It also keeps sources
/// portable to the Python pipeline, which cannot emit the implicit nil at all
/// (`OZ003: unhandled AST node 'ImplicitValueInitExpr'`) and so needs the
/// explicit form.
///
/// `oz_static_release` is null-safe, so a first overwrite releasing this is a
/// no-op either way.
fn is_null_initializer(node: Node, src: &str) -> bool {
    let text = node_text(node, src).trim();
    matches!(text, "0" | "nil" | "NULL" | "((id)0)" | "(id)0")
}

/// `(void)name;` acknowledgements for every parameter a translated method body
/// never mentions, innermost-scope first in signature order.
///
/// Zephyr's own warning set does not include `-Wextra`, so an unused parameter
/// is not a build failure -- but it is noise that hides the next real warning,
/// and three of the four defects gap M found were only visible because someone
/// counted warnings by kind. 58 of these across the samples made that counting
/// harder than it should be.
///
/// The same acknowledgement the SDK's own C already uses: `(void)inner;` in
/// `oz_platform.h`'s heap stubs, `(void)expr;` in `oz_sdk/assert.h`.
///
/// Decided from the **rendered** body -- the C a compiler will actually see --
/// not from the Objective-C source it came from. That distinction is not
/// pedantic: an ivar reference like `_n` lowers to `self->_n`, so a method
/// whose source never writes the word `self` can still use the parameter.
/// Checking the source marked `- (int)useAll:… { return a + b + _n; }` as not
/// using `self` and emitted a redundant `(void)self;` for it.
///
/// `self` is included at all because an empty `-dealloc` is idiomatic
/// Objective-C, so the warning fires on entirely correct code.
///
/// Word-boundary matched, so `_next` does not count as a use of `n`. The
/// rendered body also carries the per-statement source comments, so a name a
/// comment mentions but the code does not is treated as used and keeps its
/// warning -- the safe direction, and the only inaccuracy left: a false "used"
/// leaves a warning in place, while a false "unused" would emit a redundant
/// `(void)x;`, which is valid C either way. Neither can change behaviour.
fn unused_param_acks(
    rendered_body: &str,
    params: &[(String, String)],
    is_class_method: bool,
) -> Vec<String> {
    let mut names: Vec<&str> = Vec::new();
    if !is_class_method {
        names.push("self");
    }
    for (pname, _) in params {
        names.push(pname.as_str());
    }
    acks_for_names(rendered_body, &names)
}

/// The name-list form, shared with `render_block`: a hoisted block literal is
/// a function oz_static synthesizes outright, signature included, so its own
/// unused parameters are its to acknowledge. `samples/gpio_demo`'s
/// `blockCallback:^(const struct device *port, struct gpio_callback *cb,
/// gpio_port_pins_t pins)` accounts for three of them and
/// `transpiled_generics`'s `^(id obj, unsigned int idx, BOOL *stop)` for a
/// fourth.
fn acks_for_names(rendered_body: &str, names: &[&str]) -> Vec<String> {
    fn mentions_word(haystack: &str, name: &str) -> bool {
        let bytes = haystack.as_bytes();
        let mut from = 0;
        while let Some(rel) = haystack[from..].find(name) {
            let start = from + rel;
            let end = start + name.len();
            let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
            let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
            if before_ok && after_ok {
                return true;
            }
            from = start + 1;
        }
        false
    }
    fn is_ident_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_'
    }

    /* A `#line` directive is not code, and its file field is a path the
     * author never wrote: a body in `src/status.m` whose method takes a
     * `status` parameter would look as though it mentioned it, and the
     * acknowledgement that keeps `-Wunused-parameter` quiet would be
     * dropped for a parameter nothing actually reads (#305). Filtered
     * rather than scanned around, so no caller has to know. */
    let code_only: String = rendered_body
        .lines()
        .filter(|line| !line.trim_start().starts_with("#line "))
        .collect::<Vec<&str>>()
        .join("\n");

    names
        .iter()
        .filter(|name| !name.is_empty() && !mentions_word(&code_only, name))
        .map(|name| format!("\t(void){};", name))
        .collect()
}

/// Parameter names declared by a `parameter_list`, in order. An unnamed or
/// `void` parameter yields nothing.
fn parameter_list_names(plist: Node, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = plist.walk();
    for child in plist.children(&mut cursor) {
        if child.kind() != "parameter_declaration" {
            continue;
        }
        // The *last* identifier in the subtree is the declared name: the
        // earlier ones belong to the type (`const struct device *port` has
        // `device` before `port`).
        let mut found: Option<String> = None;
        fn walk(node: Node, src: &str, found: &mut Option<String>) {
            if node.kind() == "identifier" {
                *found = Some(node_text(node, src).to_string());
            }
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                walk(ch, src, found);
            }
        }
        walk(child, src, &mut found);
        if let Some(name) = found {
            out.push(name);
        }
    }
    out
}

/// Splice `lines` in directly after a rendered body's opening `{`.
///
/// The body always starts with `{` (`render_body_with_comments` builds it that
/// way, and the untranslated fallback is the source text of a
/// `compound_statement`). If it somehow does not, the body is returned
/// unchanged rather than corrupted -- an acknowledgement is a nicety, and
/// mangling a body to add one would not be.
///
/// `resume` is a `#line` directive re-stating where the body was, written
/// after the spliced lines -- empty when directives are off. Inserting a
/// line into a body whose attribution came from *counting on* from one
/// directive puts every line after it one late, which is a whole body
/// misattributed by a nicety; see `LineDirectives::resume_after_brace`.
fn splice_after_open_brace(body_text: &str, lines: &[String], resume: &str) -> String {
    if lines.is_empty() {
        return body_text.to_string();
    }
    match body_text.find('{') {
        Some(i) => {
            let (head, tail) = body_text.split_at(i + 1);
            // A single-line body (`{ return a; }`) leaves the next statement
            // sharing the last acknowledgement's line, so start a fresh one.
            // Generated C is read by people; `(void)b; return a; }` on one line
            // is valid and unpleasant.
            if tail.starts_with('\n') {
                format!(
                    "{}\n{}\n{}{}",
                    head,
                    lines.join("\n"),
                    resume,
                    tail.strip_prefix('\n').unwrap_or(tail)
                )
            } else {
                format!("{}\n{}\n{}\t{}", head, lines.join("\n"), resume, tail.trim_start())
            }
        }
        None => body_text.to_string(),
    }
}

/// The `#line` anchor for a **verbatim** body about to be emitted at the
/// start of a line, or nothing.
///
/// A body `render_body_with_comments` returned byte-identical has no
/// per-statement directives, so its attribution is whatever precedes it,
/// counted on line by line. That works out on its own only when the
/// generated text ahead of it has the same line count as the source did,
/// and a method's does not: `- (void)foo:(int)a\n bar:(int)b` is two source
/// lines and one generated one. One anchor on the brace fixes every line of
/// the body at once.
///
/// Nothing when the body was rendered (each statement carries its own),
/// when it is written on a single line (it lands on the line after the
/// signature, which is where the source has it too), or when directives are
/// off.
fn body_anchor(body: Node, body_text: &str, ctx: &EmitCtx) -> String {
    let verbatim = body_text == node_text(body, ctx.src);
    if !verbatim || !body_text.contains('\n') {
        return String::new();
    }
    ctx.lines.before(body.start_byte())
}

/// Does `root`'s subtree read the identifier `name`?
fn references_identifier(name: &str, root: Node, src: &str) -> bool {
    if root.kind() == "identifier" && node_text(root, src) == name {
        return true;
    }
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    children.into_iter().any(|child| references_identifier(name, child, src))
}

/// How a single store to a strong local can be emitted.
#[derive(PartialEq, Clone, Copy)]
enum LocalStore {
    /// A `+1` right-hand side that does not mention the variable: the old
    /// value can be released *before* evaluating it.
    ///
    /// `+1` is `arc::binds_ownership`, so a cast over a genuinely new
    /// reference lands here rather than in `Unsupported` -- and a cast over
    /// one the receiver already owns still does not (#332).
    Owning,
    /// A plain identifier: free of side effects, so it can be named twice
    /// and retained before the release, which is what makes `c = c` safe.
    BorrowedIdent,
    /// Anything else -- a `+0` call, or a `+1` one that reads the variable
    /// it is about to overwrite. Both would need a temporary, and a
    /// temporary cannot be placed correctly here (see
    /// `render_strong_local_assign`), so a local with any such store is not
    /// managed at all.
    Unsupported,
}

fn classify_store(name: &str, rhs: Node, src: &str, program: &Program) -> LocalStore {
    let owning = crate::arc::binds_ownership(rhs, src, program, &program.owning_methods);
    let mentions_self = references_identifier(name, rhs, src);
    if owning && !mentions_self {
        return LocalStore::Owning;
    }
    if rhs.kind() == "identifier" {
        return LocalStore::BorrowedIdent;
    }
    LocalStore::Unsupported
}

/// Every store to `name` under `root`, in source order.
fn stores_to_local(
    name: &str,
    root: Node,
    src: &str,
    program: &Program,
    out: &mut Vec<LocalStore>,
) {
    if root.kind() == "assignment_expression" {
        let mut cursor = root.walk();
        let parts: Vec<Node> = root.children(&mut cursor).collect();
        if parts.len() >= 3 && parts[0].kind() == "identifier" && node_text(parts[0], src) == name {
            let op = node_text(parts[1], src);
            if op == "=" {
                out.push(classify_store(name, *parts.last().unwrap(), src, program));
            } else {
                // A compound store (`|=` and friends) on an object local is
                // not something ARC can reason about.
                out.push(LocalStore::Unsupported);
            }
        }
    }
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    for child in children {
        stores_to_local(name, child, src, program, out);
    }
}

/// Record which object locals declared under `body` are strong locals ARC
/// manages -- see `EmitCtx::arc_managed_locals`.
///
/// The membership rule is deliberately narrow, because the two halves of
/// ownership have to match exactly: an overwrite may release the previous
/// value only if that value was itself owned. Releasing a reference never
/// taken is a double free, which is precisely the bug gap L was (the
/// release half of strong-ivar ownership shipped without the retain half).
/// So a local qualifies only when every value it can hold is owned:
///
/// - **Bare declaration** (`Counter *c;`) with at least one `+1` assignment.
///   Every assignment then goes through `render_strong_local_assign`, which
///   retains a borrowed right-hand side, so all values are owned. The
///   declaration is also given ARC's implicit `= nil`, without which the
///   first overwrite would release an indeterminate pointer.
/// - **Owning initializer** (`Counter *c = [Counter alloc];`) -- already
///   owned and already released at scope exit today; all this adds is
///   release-on-overwrite.
///
/// A **borrowed initializer** (`Counter *c = [arr objectAtIndex:0];`) is
/// excluded. Real ARC would retain it, but oz_static does not, so its value
/// is unowned and releasing it on overwrite would be that same double free.
/// Making those strong is a larger change to observable refcounts and is not
/// what this fix is for.
///
/// A local the body releases by hand is excluded throughout, keeping the
/// standing rule that ARC defers to manual retain/release -- see
/// `released_by_hand`.
/// Does this `declaration` carry `static` storage?
///
/// The distinction ARC draws and oz_static did not: a `static` local is
/// `__strong` like any other object local -- a store into it retains and
/// releases what it replaced -- but its storage duration is the
/// program's, so it must **not** be released when the scope ends. Doing
/// both destroyed the object on the way out and then released the freed
/// block again on the next call (#359).
///
/// Read off the declaration's own `storage_class_specifier`, so it sees
/// only the storage class and not a `static` appearing anywhere else in
/// the text.
fn is_static_declaration(decl: Node, src: &str) -> bool {
    let mut cursor = decl.walk();
    let children: Vec<Node> = decl.children(&mut cursor).collect();
    children.into_iter().any(|child| {
        child.kind() == "storage_class_specifier" && node_text(child, src).trim() == "static"
    })
}

/// The object locals of `body` that have `static` storage -- strong slots
/// whose lifetime outlives the scope. See `is_static_declaration`.
pub(crate) fn static_object_locals(
    body: Node,
    src: &str,
    program: &Program,
) -> std::collections::HashSet<String> {
    fn walk(
        node: Node,
        src: &str,
        program: &Program,
        found: &mut std::collections::HashSet<String>,
    ) {
        if node.kind() == "block_literal" {
            return;
        }
        if node.kind() == "declaration" && is_static_declaration(node, src) {
            let (type_text, stars) = crate::collect::extract_type_and_stars(node, src);
            if stars == 1
                && program.is_class(type_text.trim())
                && !node_text(node, src).contains("__unsafe_unretained")
            {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if matches!(child.kind(), "pointer_declarator" | "init_declarator") {
                        let name = crate::collect::find_declared_name(child, src);
                        if !name.is_empty() {
                            found.insert(name);
                        }
                    }
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, src, program, found);
        }
    }

    let mut found = std::collections::HashSet::new();
    walk(body, src, program, &mut found);
    found
}

pub(crate) fn managed_object_locals(
    body: Node,
    src: &str,
    program: &Program,
) -> std::collections::HashSet<String> {
    fn walk(
        node: Node,
        body: Node,
        found: &mut Vec<String>,
        src: &str,
        program: &Program,
    ) {
        if node.kind() == "declaration" && !is_block_qualified_declaration(node, src) {
            // `extract_type_and_stars` yields the *source* spelling, so an
            // object type is just the class name -- no `struct` prefix to
            // strip, and `is_class` is the authority on whether the name is
            // a class at all rather than a plain C struct.
            let (type_text, stars) = crate::collect::extract_type_and_stars(node, src);
            let is_object = stars == 1 && program.is_class(type_text.trim());
            if is_object && !node_text(node, src).contains("__unsafe_unretained") {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    let bare = child.kind() == "pointer_declarator";
                    let init = child.kind() == "init_declarator";
                    if !bare && !init {
                        continue;
                    }
                    let name = crate::collect::find_declared_name(child, src);
                    if name.is_empty() || released_by_hand(&name, body, src) {
                        continue;
                    }
                    // Every store has to be one the renderer can emit, or
                    // the variable would be managed for some assignments and
                    // not others -- and then the scope-exit release could
                    // free a value nothing ever retained.
                    let mut stores = Vec::new();
                    stores_to_local(&name, body, src, program, &mut stores);
                    if stores.contains(&LocalStore::Unsupported) {
                        continue;
                    }
                    // An initializer that is just `nil`/`0` leaves the
                    // variable empty, which is what a bare declaration means
                    // too -- so the two are decided by the same rule.
                    let starts_empty = if bare {
                        true
                    } else {
                        let mut c2 = child.walk();
                        let parts: Vec<Node> = child.children(&mut c2).collect();
                        let eq = parts.iter().position(|n| n.kind() == "=");
                        eq.and_then(|i| parts.get(i + 1))
                            .copied()
                            .is_some_and(|v| is_null_initializer(v, src))
                    };
                    if starts_empty {
                        // Strong only if something owned is ever stored. A
                        // local that only ever receives borrowed values is
                        // left alone: retaining those is a broader change to
                        // observable refcounts than this fix is for.
                        if stores.contains(&LocalStore::Owning) {
                            found.push(name);
                        }
                    } else {
                        // An owning initializer is already owned today; a
                        // borrowed one is deliberately left alone.
                        let mut c2 = child.walk();
                        let parts: Vec<Node> = child.children(&mut c2).collect();
                        let eq = parts.iter().position(|n| n.kind() == "=");
                        let Some(value) = eq.and_then(|i| parts.get(i + 1)).copied() else {
                            continue;
                        };
                        if crate::arc::binds_ownership(
                            value,
                            src,
                            program,
                            &program.owning_methods,
                        ) {
                            found.push(name);
                        }
                    }
                }
            }
        }
        // A block literal has its own scope and its own locals; those are
        // not this body's to manage.
        if node.kind() == "block_literal" {
            return;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, body, found, src, program);
        }
    }

    let mut found = Vec::new();
    walk(body, body, &mut found, src, program);
    found.into_iter().collect()
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
        // `pointer_declarator` covers the one shape the other two miss: a
        // *pointer* declared with no initializer. `__block int q;` was already
        // handled, because a bare non-pointer declarator is itself an
        // `identifier` -- which is exactly why this went unnoticed. Only
        // `__block Foo *p;` fell through, and then nothing was hoisted at all,
        // leaving the block referencing a name that was not there.
        if !matches!(child.kind(), "init_declarator" | "identifier" | "pointer_declarator") {
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
///
/// # Where a parameter type has to be spelled the same way twice
///
/// This function is the roster the others point at, because it is the one
/// every *method* signature goes through. Every position below produces or
/// consumes the same C function-pointer type, so all of them have to agree
/// on how a parameter type is spelled -- one side disagreeing is an
/// incompatible-pointer initialization at best and text no C compiler
/// accepts at worst:
///
///   - here, for a block-typed method parameter, off the
///     `ID_TYPE_PLACEHOLDER` that `collect::detect_block_param_type` marked
///     while it still had the CST;
///   - `collect_ivar_lowering_edits`, for a function-pointer ivar's field
///     type -- which is also where the reasoning for *not* making `id` the
///     root pointer globally is written down;
///   - `render_block`, for the signature it synthesizes for a hoisted block
///     literal;
///   - `render_block_type_param_list`, for a block variable's own declared
///     type inside a method body;
///   - `block_pointer_edits`, for that same declared type at file scope, and
///     for a free function's block-typed parameter.
///
/// The last two are #319; the split between them is only that the `^` -> `*`
/// lowering itself is spelled once as a render and once as text edits.
///
/// **Two** spellings have to agree at those positions, not one, and a
/// position can get one right and the other wrong:
///
///   1. a type-position `id` becomes the root class pointer (#317, #319).
///      Keeping the `id` typedef says `void (*)(void *)` where its
///      counterpart says `void (*)(struct OZObject *)`, and the assignment,
///      initialization or call between them stops compiling.
///   2. a bare class name gets its `struct` tag (#246, #326) -- see
///      `class_tag_edits`. This one is not merely a weaker type: `Widget *`
///      with no tag is `error: must use 'struct' tag to refer to type
///      'Widget'`, not valid C at all.
///
/// Rule 2 was the one `render_block` missed while getting rule 1 right, so a
/// class-typed block parameter did not compile even after #319. Every
/// position that rebuilds its type through `collect::render_type` gets both
/// for free; the ones above patch source text instead, which is why they
/// have to spell each rule out.
pub(crate) fn render_param(ptype: &str, pname: &str, root: Option<&str>) -> String {
    if ptype.contains(crate::collect::PARAM_NAME_PLACEHOLDER) {
        // A function-pointer parameter: its own parameter list came through
        // from source, with any `id` *type* in it already marked by
        // `collect::detect_block_param_type` -- which had the CST and could
        // therefore tell a parameter typed `id` from one merely named it
        // (#317). It has to become the root class pointer, for the same
        // reason a function-pointer *ivar*'s does -- see
        // `collect_ivar_lowering_edits`. Those two must agree in particular:
        // an `-initWithBlock:` parameter is assigned straight into the
        // matching field, and with only the field lowered the assignment
        // itself stopped compiling.
        let rendered = ptype.replace(crate::collect::PARAM_NAME_PLACEHOLDER, pname);
        return match root {
            Some(root) => {
                rendered.replace(crate::collect::ID_TYPE_PLACEHOLDER, &format!("struct {} *", root))
            }
            // No root class, so there is nothing to lower `id` to; put the
            // spelling back rather than leaking the marker.
            None => rendered.replace(crate::collect::ID_TYPE_PLACEHOLDER, "id"),
        };
    }
    format!("{} {}", ptype, pname)
}

/// Is this `function_declarator` the shape a block-pointer *type* parses
/// as -- `RET (^NAME)(ARGS)` -- rather than an ordinary function
/// declarator?
///
/// The `^` sits one level down, inside the parenthesized declarator:
/// `function_declarator > parenthesized_declarator > block_pointer_declarator`.
/// A `*` on the return type (`void *(^f)(id)`) hangs off a
/// `pointer_declarator` *above* this node, so it does not disturb the
/// two-level walk.
fn wraps_block_pointer_declarator(node: Node) -> bool {
    let mut cursor = node.walk();
    let parens: Vec<Node> = node
        .children(&mut cursor)
        .filter(|c| {
            matches!(c.kind(), "parenthesized_declarator" | "abstract_parenthesized_declarator")
        })
        .collect();
    for paren in parens {
        let mut inner = paren.walk();
        let children: Vec<Node> = paren.children(&mut inner).collect();
        let carets = children.into_iter().any(|c| {
            matches!(c.kind(), "block_pointer_declarator" | "abstract_block_pointer_declarator")
        });
        if carets {
            return true;
        }
    }
    false
}

/// Render a block-pointer declarator's own parameter list, lowering a
/// type-position `id` to `root` (the root class pointer, already spelled
/// `struct Root *`).
///
/// The same lowering `render_param`, `collect_ivar_lowering_edits` and
/// `render_block` already apply -- see `render_param` for the roster and why
/// they have to agree -- at the one position that did not.
/// `void (^b)(id) = ^(id obj) { ... };` hoisted a function taking
/// `struct OZObject *` while declaring `b` as `void (*)(id)`, which is
/// `void (*)(void *)`, and the initialization was then an
/// incompatible-function-pointer error (#319).
///
/// A recursive render rather than `rewrite_id_types` + `apply_edits`, which
/// is how `render_block` and `collect_ivar_lowering_edits` spell the same
/// lowering: unlike theirs, this list sits inside a declaration that is
/// *already* being rendered, and the ordinary `needs_translation` recursion
/// promotes a class name in it (`void (^b)(Widget *)` ->
/// `void (*b)(struct Widget *)`) as a side effect. The `id` cases are
/// therefore tested before `needs_translation`, so a
/// `parameter_declaration` carrying both an `id` and a translatable child
/// still gets both.
///
/// That side effect is what left this side right and the hoisted side wrong
/// until #326: a flat-text rewrite of only the `id` drops the class-name
/// promotion, and `render_block` -- which does patch text -- had to be told
/// to promote class names explicitly, through `class_tag_edits`. The two
/// sides agree again; see `render_param`'s roster for both rules.
///
/// `root` is `None` when the program has no root class, in which case there
/// is nothing to lower `id` to and the spelling stays -- the same answer
/// `render_param` gives.
fn render_block_type_param_list(node: Node, ctx: &mut EmitCtx, root: Option<&str>) -> String {
    rebuild(node, ctx, &mut |child, ctx| {
        if is_bare_id_type(child, ctx.src) {
            return root.map(|root| root.to_string());
        }
        if contains_bare_id_type(child, ctx.src) {
            return Some(render_block_type_param_list(child, ctx, root));
        }
        if needs_translation(child) {
            return Some(render_expr(child, ctx).0);
        }
        None
    })
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

/// Note this is purely a spelling transform: it says nothing about whether
/// the name is a *class*. Every plain C struct type is spelled `struct Foo`
/// too, so a caller about to treat the result as a class must ask
/// `Program::is_class` as well.
/// Suffix marking a scope entry as an *array of* its element type rather
/// than one of it.
///
/// The scope maps a name to a C type string, and C spells an array's extent
/// after the name, so there is nowhere in the type to say "array" -- yet
/// every decision about a subscript, a send, and a store needs to know.
/// The extent itself is not carried here (it is in
/// `ClassInfo::array_extents`, and only a declaration ever needs it); this
/// only has to answer the yes/no.
const ARRAY_MARK: &str = "[]";

/// `struct Leaf *[]` -> `struct Leaf *`: the element type of an array scope
/// entry, or the type unchanged when it is not one.
fn element_type(t: &str) -> &str {
    t.strip_suffix(ARRAY_MARK).map(str::trim_end).unwrap_or(t)
}

/// Is this scope entry an array?
fn is_array_type(t: &str) -> bool {
    t.trim_end().ends_with(ARRAY_MARK)
}

fn class_name_from_type(t: &str) -> Option<String> {
    let t = t.trim();
    /* An *array of* a class is not a class: `struct Leaf *[]` indexes with
     * ordinary C, and answering `Leaf` here is what made `_leaves[0]` a
     * subscript send and then a hard error, since no class implements
     * `objectAtIndexedSubscript:` for it (#287). See `ARRAY_MARK`. */
    if t.ends_with(ARRAY_MARK) {
        return None;
    }
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
    /// Expressions already evaluated into a temporary by an enclosing
    /// renderer, keyed by `Node::id`: rendering one again yields the
    /// temporary's name and type instead of re-evaluating it.
    ///
    /// Populated only by `render_owning_operand_statement` (#328, #340),
    /// which has to hold a send operand's `+1` in a named local so it can
    /// release it after the send, and then wants the rest of the statement
    /// rendered *exactly* as it would have been. Substituting at the node
    /// rather than rewriting the call keeps the emitted operand the same
    /// type it always was, so nothing downstream -- direct call, dynamic
    /// dispatch, a cast around it -- has to know this happened. Checked
    /// ahead of `render_expr`'s match so it holds for a *receiver* as
    /// readily as an argument, which is what let #340 reuse this whole
    /// mechanism without touching how a send is called. Entries are
    /// removed as soon as that statement is rendered, so the map is empty
    /// everywhere else.
    arg_temps: HashMap<usize, (String, String)>,
    /// Cleanup statements owed by the `@synchronized` blocks currently
    /// enclosing the node being rendered, outermost first. A `return` has
    /// to replay them (innermost first) before leaving -- see
    /// `render_return_statement`.
    sync_cleanups: Vec<String>,
    /// C return type of the method *or free function* being rendered,
    /// needed to declare the temporary a `return` on the cleanup path
    /// evaluates into -- either `@synchronized` cleanups or pending ARC
    /// releases (see `render_return_statement`).
    ///
    /// Named for the method case because that is the only one it had until
    /// #336. A free function is not a method and shares no code with
    /// `render_method_definition`: the `function_definition` arm in
    /// `walk_top_level` records its own, from `function_return_type`.
    ///
    /// Every position that renders a body has to set this for itself, and
    /// there are three:
    ///
    /// - `render_method_definition`, from the method's declared type;
    /// - `walk_top_level`'s `function_definition` arm, from
    ///   `function_return_type` (#336);
    /// - `render_block`, from `block_return_type` -- the same type it gives
    ///   the hoisted function's signature (#339).
    ///
    /// The first two each own a fresh `EmitCtx` and so just assign. The
    /// block is different: it is rendered *inside* an enclosing body, on
    /// that body's `EmitCtx`, so it saves this field, sets it, renders the
    /// body and puts the enclosing body's type back. Without that restore a
    /// `return` written after the literal in the same body would take the
    /// block's type.
    method_return_type: String,
    /// Slots to reserve in each class's slab -- see `pools`.
    pools: &'a crate::pools::PoolSizes,
    /// Object locals owned by each enclosing block, innermost last. Released
    /// when their block ends and at any jump out of it -- see
    /// `render_scoped_block`.
    arc_scopes: Vec<ArcScope>,
    /// Object locals ARC manages as *strong* variables, in the sense real
    /// ARC does: an overwrite releases what was there, so the variable
    /// holds at most one live object at a time. Decided once per body by
    /// `note_arc_managed_locals` and read by `render_strong_local_assign`.
    ///
    /// This is what makes the ordinary Objective-C shape
    ///
    /// ```objc
    /// Counter *c;
    /// for (int i = 0; i < 100; i++) {
    ///         c = [Counter alloc];
    /// }
    /// ```
    ///
    /// correct on one slab slot instead of leaking 99 objects. oz_static
    /// already did retain-new/release-old for strong *ivars*
    /// (`render_strong_ivar_assign`) and for properties (a synthesized
    /// setter); a plain local was the one strong storage class left doing
    /// neither, which is why `staticbar` had to reject the loop above
    /// rather than emit it.
    arc_managed_locals: std::collections::HashSet<String>,
    /// Strong slots whose lifetime outlives the enclosing scope: object
    /// locals declared `static`. A store into one retains and releases
    /// what it replaced, exactly like `arc_managed_locals`, but nothing
    /// here is ever released at scope exit -- that is the whole
    /// distinction, and conflating the two was #359's double release.
    ///
    /// File-scope globals need no set: they are already in `scope` with
    /// their C type and absent from `locals`, which is what identifies
    /// them where the store is rendered.
    arc_managed_slots: std::collections::HashSet<String>,
    /// See `IntrospectionUse`.
    introspection_used: IntrospectionUse,
    /// Where a `#line` directive on this construct's own code points, and
    /// whether one is emitted at all -- see `LineDirectives`.
    lines: &'a LineDirectives<'a>,
}

/// One block's worth of owned object locals.
#[derive(Default)]
struct ArcScope {
    owned: Vec<String>,
    /// Where this scope's block begins in the source.
    ///
    /// This is how a `break` or `continue` decides which scopes it
    /// leaves: exactly those that *began inside* the construct being
    /// jumped out of, i.e. whose `start_byte` is past that construct's
    /// own (see `releases_up_to_jump_target`).
    ///
    /// It replaces an `is_loop_body` flag, and the flag was wrong in a
    /// way the byte cannot be. `break` inside a **`switch`** exits only
    /// the switch, but the flag made it unwind as though it were leaving
    /// the enclosing loop -- so
    ///
    /// ```objc
    /// for (...) {
    ///     Thing *t = [[Thing alloc] init];
    ///     switch (i) { case 0: break; }
    ///     [t n];
    /// }
    /// ```
    ///
    /// released `t` inside the `case`, read it after the switch, and
    /// released it again at the end of the iteration: a use-after-free
    /// and a double free, in a shape with nothing unusual about it. The
    /// flag could not express the difference because a `switch` body is a
    /// `break` boundary but not a `continue` one, while a loop body is
    /// both -- and `continue` inside a switch must still unwind past it.
    ///
    /// Asking which construct the jump actually targets, and taking the
    /// scopes inside it, answers both without a flag per construct.
    start_byte: usize,
    /// Is this block a block literal's body? A `return` unwinds through
    /// scopes up to and including the nearest one of these, and no
    /// further.
    ///
    /// The reason is the same shape as `start_byte`'s, one level up. A
    /// `block_literal` is rendered on the *enclosing* body's `EmitCtx`
    /// (see `render_block`: block bodies deliberately share their
    /// enclosing body's flat scope), so the enclosing method's ARC scopes
    /// are still stacked when the block's own `return` is rendered. Left
    /// unmarked, that `return` released the enclosing method's locals from
    /// inside the *hoisted* function, where those names do not exist:
    /// `error: 'outerKeep' undeclared` and no valid C at all (#342).
    ///
    /// The enclosing body's locals are released by the enclosing body, on
    /// its own exit -- and they are still live while the block runs, since
    /// the block may be called before the enclosing body returns.
    is_block_body: bool,
}

impl ArcScope {
    /// The scope to enter for `body`, marked with whichever unwinding
    /// boundaries it is.
    ///
    /// One constructor because every field has to be set the same way at
    /// every push site, and there are two (`arc_enter` and
    /// `render_body_with_comments`). The loop-body flag this replaced was
    /// already spelled out at both, so adding a second field to only one
    /// of them was the available mistake.
    fn for_body(body: Node) -> ArcScope {
        ArcScope {
            owned: Vec::new(),
            start_byte: body.start_byte(),
            is_block_body: is_block_body(body),
        }
    }
}

impl<'a> EmitCtx<'a> {
    /// A fresh context for one top-level construct.
    ///
    /// Only the four things that actually vary between call sites are
    /// arguments; everything else starts empty or at its one sensible
    /// default. This exists because the eighteen fields used to be spelled
    /// out at six separate call sites -- three per emitter -- so adding a
    /// field meant six edits and seeding something into scope meant at
    /// least two, which is the exact shape of #250's fix (see #254).
    fn new(
        src: &'a str,
        program: &'a Program,
        class_name: String,
        scope: HashMap<String, String>,
        pools: &'a crate::pools::PoolSizes,
        lines: &'a LineDirectives<'a>,
    ) -> Self {
        EmitCtx {
            src,
            program,
            class_name,
            scope,
            locals: HashSet::new(),
            diags: Vec::new(),
            hoisted_blocks: Vec::new(),
            hoisted_structs: Vec::new(),
            hoisted_string_literals: Vec::new(),
            hoisted_statics: Vec::new(),
            block_counter: 0,
            pre_stmts: Vec::new(),
            arg_temps: HashMap::new(),
            sync_cleanups: Vec::new(),
            // A placeholder, not a default that is ever right: whoever
            // renders a body overwrites it with that body's real return
            // type before any statement of it is rendered. It reads as
            // harmless, which is exactly why #336 survived -- the
            // free-function arm never overwrote it, and `int` is a
            // plausible-looking type, so the wrong output compiled on ARM
            // and truncated on the host instead of failing anywhere
            // obvious. #339 was the same omission in `render_block`, where
            // what leaked through was not this placeholder but the
            // *enclosing* body's type, equally plausible-looking.
            method_return_type: "int".to_string(),
            pools,
            arc_scopes: Vec::new(),
            arc_managed_locals: HashSet::new(),
            arc_managed_slots: HashSet::new(),
            introspection_used: IntrospectionUse::default(),
            lines,
        }
    }

    fn err(&mut self, node: Node, message: impl Into<String>) {
        let (line, col) = line_col(self.src, node.start_byte());
        self.diags.push(Diagnostic::new(message, line, col));
    }
}

/// Which introspection support the emitted code actually referenced.
///
/// Gated on *use*, not on `--introspection` being set, so a program that
/// enables the option and never introspects anything pays nothing: the
/// superclass chain and each protocol's conformance bitmap are emitted
/// only if some call site named them. That is why this is threaded back
/// out of the walk instead of being derived from the `Program` -- the
/// emitter is the only thing that knows what it wrote.
#[derive(Default, Debug)]
pub struct IntrospectionUse {
    /// `-isKindOfClass:` appeared, so the ancestry walk and its table are
    /// needed.
    pub kind_of: bool,
    /// Protocols named by a `@protocol(...)` reaching
    /// `-conformsToProtocol:`; one conformance bitmap each. Ordered so the
    /// generated text is deterministic.
    pub protocols: std::collections::BTreeSet<String>,
}

impl IntrospectionUse {
    fn merge(&mut self, other: IntrospectionUse) {
        self.kind_of |= other.kind_of;
        self.protocols.extend(other.protocols);
    }

    pub fn is_empty(&self) -> bool {
        !self.kind_of && self.protocols.is_empty()
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
            | "cast_expression"
            // Visited so ARC can prepend the releases a jump out of a loop
            // owes (`render_loop_jump`). With no owned local in scope the
            // keyword is returned unchanged, so this costs no output churn.
            | "break_statement"
            | "continue_statement"
            // `return` for the same reason, and it was missing: a return
            // inside an otherwise pure-C subtree was never visited, so
            // `render_return_statement` never ran and the scopes it unwinds
            // past kept their releases at the end of the block, where the
            // jump had already skipped them. `arc/return_in_nested_scope`
            // leaked its loop-body local on exactly that path -- an early
            // `return` from an `if` inside a `while`, with no Objective-C
            // anywhere in the `if` to force a visit. Found by running the
            // corpus under LeakSanitizer through this backend for the first
            // time; the case passed its own assertions throughout, since a
            // leak is invisible to a test that only checks return values.
            //
            // Costs no churn for the same reason the two above do not:
            // `render_return_statement` returns the original text when
            // there is nothing to release.
            | "return_statement"
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
/// The protocol named by a `@protocol(Name)` expression, or `None` if
/// `node` is not one.
///
/// A protocol has no runtime representation of its own here: the name is
/// resolved to a generated conformance bitmap
/// (`companion::render_introspection`), so `@protocol(...)` is legal only
/// where that bitmap is what is wanted -- as the argument of
/// `-conformsToProtocol:`. The static bar enforces the position; this only
/// reads the name.
pub(crate) fn protocol_literal_name(node: Node, src: &str) -> Option<String> {
    if !is_protocol_literal_shape(node, src) {
        return None;
    }
    let mut cursor = node.walk();
    let inner = node.children(&mut cursor).find(|c| c.kind() != "@")?;
    let mut c2 = inner.walk();
    let args = inner.children(&mut c2).find(|c| c.kind() == "argument_list")?;
    let mut c3 = args.walk();
    let name = args.children(&mut c3).find(|c| c.kind() == "identifier")?;
    Some(node_text(name, src).to_string())
}

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
    // An expression an enclosing renderer already evaluated into a
    // temporary is *this* expression -- naming the temporary is what keeps
    // the send from evaluating it a second time (#328). Checked ahead of
    // the match so it holds for every shape, and skipped entirely when the
    // map is empty, which is everywhere but inside one statement renderer.
    if !ctx.arg_temps.is_empty() {
        if let Some((name, ty)) = ctx.arg_temps.get(&node.id()) {
            return (name.clone(), ty.clone());
        }
    }
    match node.kind() {
        /* A send whose `+1` operand nothing will hoist accounts for it
         * inside the expression instead -- see
         * `render_comma_operand_expr` and `conditionally_evaluated`.
         * Ahead of the plain arm because it *is* the plain arm with the
         * operands held, and it calls `render_message` directly rather
         * than recursing back through here. */
        "message_expression" if !unhoisted_owning_operands(node, ctx).is_empty() => {
            let values = unhoisted_owning_operands(node, ctx);
            render_comma_operand_expr(node, ctx, values)
        }
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
        // ARC's implicit `= nil` on a strong local declared without an
        // initializer. Real ARC zero-initializes every strong variable, and
        // here it is load-bearing rather than tidy: the first
        // `c = [Counter alloc]` releases whatever `c` held, so an
        // indeterminate `c` would be passed to `oz_static_release` and
        // dereferenced. `oz_static_release` is null-safe (`if (!self)
        // return;`), so nil makes that first release a no-op.
        "declaration" if declares_bare_managed_local(node, ctx) => {
            let text = rebuild(node, ctx, &mut |child, ctx| {
                if needs_translation(child) {
                    Some(render_expr(child, ctx).0)
                } else {
                    None
                }
            });
            let initialized = match text.rfind(';') {
                Some(i) => format!("{} = 0{}", text[..i].trim_end(), &text[i..]),
                None => text,
            };
            (initialized, "id".to_string())
        }
        "selector_expression" => render_selector_literal(node, ctx),
        "at_expression" if is_protocol_literal_shape(node, ctx.src) => {
            render_protocol_literal(node, ctx)
        }
        "at_expression" => render_boxed_at_expression(node, ctx),
        "string_literal" => render_boxed_string_literal(node, ctx),
        "array_literal" => render_boxed_array_literal(node, ctx),
        "dictionary_literal" => render_boxed_dictionary_literal(node, ctx),
        // The `(ARGS)` half of a block-typed variable's own declared type:
        // `void (^b)(id) = ^(id obj) { ... };`. The list hangs off this
        // `function_declarator` as a *sibling* of the
        // `parenthesized_declarator` holding the `^`, so the
        // `block_pointer_declarator` arm below never sees it -- and left to
        // the generic rebuild it passed through verbatim, so `b` was
        // declared `void (*b)(id)` (i.e. `void (*)(void *)`) and
        // initialized from a hoisted function taking `struct OZObject *`.
        // Clang rejects that as incompatible function pointer types (#319).
        // See `render_block_type_param_list`.
        "function_declarator" | "abstract_function_declarator"
            if wraps_block_pointer_declarator(node) =>
        {
            let root = ctx.program.root_class().map(|root| format!("struct {} *", root));
            let text = rebuild(node, ctx, &mut |child, ctx| {
                if child.kind() == "parameter_list" {
                    return Some(render_block_type_param_list(child, ctx, root.as_deref()));
                }
                if needs_translation(child) {
                    Some(render_expr(child, ctx).0)
                } else {
                    None
                }
            });
            (text, "id".to_string())
        }
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
        "field_expression" => render_field_expression(node, ctx),
        "assignment_expression" => render_assignment_expression(node, ctx),
        "cast_expression" => render_cast_expression(node, ctx),
        "for_statement" if is_forin_shape(node) => render_forin_statement(node, ctx),
        // A +1 operand in a `for` header's initialiser (#341). Its own arm
        // rather than a widened guard on the declaration one below: a `for`
        // header cannot take a statement group, so the whole loop is
        // wrapped instead, with the temporary above it and the release
        // after it. Bracing scopes nothing out -- a header declaration is
        // already scoped to the `for` -- and the release lands after the
        // loop rather than after the send, which costs the temporary its
        // slab slot for the loop's duration and buys a release that happens
        // exactly once.
        //
        // Below the for-in arm, so `for (id x in c)` still claims its own.
        /* A `return` whose value abandons a `+1` operand had no arm at
         * all, so `return [h take:makeThing()];` leaked outright. It is
         * safe to hoist for the same reason a controlling expression is:
         * evaluated exactly once, and the group the statement is wrapped
         * in is itself inside whatever loop encloses it.
         *
         * The group is an ARC scope (see
         * `render_owning_operand_statement`), so the `return` inside it
         * unwinds and releases the temporary on its way out -- after the
         * retain `render_return_statement` puts on the returned value,
         * which is the ordering the whole shape depends on. */
        "return_statement" if !owning_send_operands(node, ctx).is_empty() => {
            let values = owning_send_operands(node, ctx);
            render_owning_operand_statement(node, ctx, values)
        }
        "if_statement" | "switch_statement"
            if !condition_owning_operands(node, ctx).is_empty() =>
        {
            let values = condition_owning_operands(node, ctx);
            render_owning_operand_statement(node, ctx, values)
        }
        "for_statement" if !for_header_owning_operands(node, ctx).is_empty() => {
            let values = for_header_owning_operands(node, ctx);
            render_owning_operand_statement(node, ctx, values)
        }
        /* A `for` header's own `+1` **declaration** (#376). Below the
         * operand arm above, deliberately: a header whose initialiser
         * both holds an operand and binds ownership keeps the handling it
         * already had rather than getting a second, untested wrapper --
         * and the two do not overlap in practice, since an initialiser
         * that binds ownership is `+1` at its outermost expression while
         * an operand is one nested inside it.
         *
         * The declaration has to *move* rather than be named by a
         * temporary, which is why this is its own renderer and not a
         * widened `for_header_owning_operands`. */
        "for_statement" if for_header_owned_declaration(node, ctx).is_some() => {
            let (init, owned) = for_header_owned_declaration(node, ctx)
                .expect("guarded by the arm above");
            render_for_header_owned_declaration(node, ctx, init, owned)
        }
        "synchronized_statement" => render_synchronized_statement(node, ctx),
        "return_statement" => render_return_statement(node, ctx),
        "compound_statement" if is_autoreleasepool_shape(node) => {
            render_autoreleasepool_statement(node, ctx)
        }
        // Only a block that owns object locals is rewritten; every other one
        // stays byte-identical, so ARC adds no churn where it changes nothing.
        "compound_statement" if declares_owned_local(node, ctx) => {
            render_scoped_block(node, ctx)
        }
        "break_statement" | "continue_statement" if !ctx.arc_scopes.is_empty() => {
            render_loop_jump(node, ctx)
        }
        // A +1 result passed straight as an argument (#328), or used as a
        // send's receiver (#340). Ahead of the discarded-result arm below,
        // because a statement can be both -- `[[Foo new] take:[Bar new]];`
        // abandons its own result *and* hands one over -- and this arm
        // renders the statement through the ordinary dispatch, so that one
        // still gets its turn.
        "expression_statement" if !owning_send_operands(node, ctx).is_empty() => {
            let values = owning_send_operands(node, ctx);
            render_owning_operand_statement(node, ctx, values)
        }
        // A +1 result bound to nothing (#322).
        "expression_statement" if discards_owning_result(node, ctx) => {
            render_discarded_owning_statement(node, ctx)
        }
        "declaration" if is_block_qualified_declaration(node, ctx.src) => {
            hoist_block_var(node, ctx);
            (String::new(), "id".to_string())
        }
        // The same +1 operand in a declaration's initialiser --
        // `int n = [self countOf:[Foo new]];` (#328), or
        // `int n = [[Foo alloc] tag];` (#340). A declaration cannot be
        // wrapped in a block, because that would scope the name it
        // introduces out of the rest of the body, so this one is a group of
        // statements rather than a braced one -- which is legal only where
        // several statements are, hence the parent check. It stays in place
        // rather than being hoisted, since the emitter substitutes over the
        // declaration's own byte range.
        "declaration"
            if node.parent().is_some_and(|parent| parent.kind() == "compound_statement")
                && !owning_send_operands(node, ctx).is_empty() =>
        {
            let values = owning_send_operands(node, ctx);
            render_owning_operand_statement(node, ctx, values)
        }
        /* A call's result carries the callee's declared return type, so a
         * message can be sent straight to it (#355). Until this arm
         * existed, `makeThing()` fell to the default below and was typed
         * `id`, which is not a type any receiver resolution can use:
         * `[makeThing() poke]` was refused -- reporting
         * `class 'OZObject' has no method matching 'poke'`, naming a class
         * the source never writes, because a non-pointer type reads as
         * "some object, cast it to the root pointer" in
         * `render_owning_operand_statement` -- while binding the result to
         * a local first compiled. The callee's signature said `Thing *`
         * the whole time.
         *
         * Only a call through a plain identifier that names a function is
         * answered. A local of the same name shadows the function in C, so
         * `ctx.locals` is checked first; a call through a function pointer,
         * a block variable or a member keeps the old `id` rather than being
         * guessed at.
         *
         * The *text* is built exactly as the default arm builds it --
         * `rebuild_or_text` is that arm's body, factored out -- because
         * nothing about how a call is written changes here. Only its type
         * was missing. */
        "call_expression" if !unhoisted_owning_operands(node, ctx).is_empty() => {
            let values = unhoisted_owning_operands(node, ctx);
            render_comma_operand_expr(node, ctx, values)
        }
        "call_expression" => {
            let text = rebuild_or_text(node, ctx);
            (text, call_result_type(node, ctx).unwrap_or_else(|| "id".to_string()))
        }
        _ => (rebuild_or_text(node, ctx), "id".to_string()),
    }
}

/// An expression's text with every Objective-C construct under it
/// rendered, and nothing else changed -- `render_expr`'s default
/// behaviour, factored out so the `call_expression` arm can reuse it
/// verbatim and differ from the default in the *type* alone.
fn rebuild_or_text(node: Node, ctx: &mut EmitCtx) -> String {
    if !needs_translation(node) {
        return node_text(node, ctx.src).to_string();
    }
    rebuild(node, ctx, &mut |child, ctx| {
        if needs_translation(child) {
            Some(render_expr(child, ctx).0)
        } else {
            None
        }
    })
}

/// The C type a `call_expression` evaluates to, when the callee is a
/// plain identifier naming a function whose declared return type was
/// collected (#355).
///
/// `None` where that is not so, and the caller keeps `id` -- which is
/// what every call expression got before this existed.
fn call_result_type(node: Node, ctx: &EmitCtx) -> Option<String> {
    let mut cursor = node.walk();
    let callee = node.children(&mut cursor).next()?;
    if callee.kind() != "identifier" {
        return None;
    }
    let name = node_text(callee, ctx.src);
    /* A local shadows a file-scope function of the same name in C, so its
     * call is not this function's call. Cheap, and it keeps the lookup
     * from ever being the *wrong* answer rather than merely a missing
     * one. */
    if ctx.locals.contains(name) {
        return None;
    }
    ctx.program.function_return_types.get(name).cloned()
}

pub(crate) struct MessageParts<'a> {
    pub(crate) receiver: Node<'a>,
    pub(crate) selector: String,
    pub(crate) args: Vec<Node<'a>>,
}

/// Is this send's receiver the literal `super`? A super send names one
/// specific implementation, so it is always a direct call -- it must
/// never be routed through the receiver's own class_id switch, which
/// would re-enter whichever override issued the send.
fn is_super_receiver(parts: &MessageParts, ctx: &EmitCtx) -> bool {
    parts.receiver.kind() == "identifier" && node_text(parts.receiver, ctx.src) == "super"
}

pub(crate) fn parse_message<'a>(node: Node<'a>, src: &str) -> MessageParts<'a> {
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
/// `@selector(name)` -> the address of that selector's generated record.
///
/// The record is `const`, so a `SEL` is a pointer into flash and copying
/// one costs a register. Which selectors get a record is decided in
/// `collect`'s prescan rather than here, because
/// `Program::is_dynamically_dispatched` needs the answer before the
/// dispatch tables are generated -- this only has to refuse the cases
/// that have no record to point at.
fn render_selector_literal(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    if !ctx.program.reflection {
        ctx.err(
            node,
            "'@selector(...)' needs reflection, which is off -- set CONFIG_OBJZ_REFLECTION=y (oz2c --reflection)",
        );
        return (node_text(node, ctx.src).to_string(), "SEL".to_string());
    }
    let Some(name) = crate::collect::selector_literal_name(node, ctx.src) else {
        ctx.err(node, "'@selector(...)' needs a selector name");
        return (node_text(node, ctx.src).to_string(), "SEL".to_string());
    };
    // Every implementation is found through `find_defining_method` from
    // some class, so a selector nothing implements has no record and no
    // dispatch function. Passing it on would emit a reference to a symbol
    // that is never generated -- the same link-time failure `[X class]`
    // used to produce (#226).
    let implemented = ctx.program.class_order.iter().any(|c| {
        ctx.program.classes[c]
            .methods
            .iter()
            .any(|m| m.selector == name && !m.is_class_method)
    });
    if !implemented {
        ctx.err(
            node,
            format!(
                "'@selector({})' names no instance method declared by any class in this program",
                name
            ),
        );
        return (node_text(node, ctx.src).to_string(), "SEL".to_string());
    }
    // A selector that can reach a `-performSelector:` needs a wrapper of
    // the uniform shape, and so has to fit one. Which selectors those are
    // is `Program::needs_perform_wrapper`: the literals named at perform
    // sites, or -- if any site takes its `SEL` from a value, making it
    // undecidable -- every reflectively-named selector. One that cannot
    // have a wrapper is refused here rather than given a null `perform`
    // that would fail, or worse quietly answer nil, at run time.
    if ctx.program.needs_perform_wrapper(&name) {
        if let Some(why) = unperformable_reason(ctx.program, &name) {
            ctx.err(
                node,
                format!(
                    "'@selector({})' cannot be performed: {}{}",
                    name,
                    why,
                    if ctx.program.performs_via_value {
                        ". Some '-performSelector:' in this program takes its selector from a value rather than a literal, so nothing can tell which selector reaches it and every selector named by a '@selector(...)' has to be performable"
                    } else {
                        ", and a '-performSelector:' names it"
                    }
                ),
            );
            return (node_text(node, ctx.src).to_string(), "SEL".to_string());
        }
    }
    (format!("(&oz_sel_{})", selector_to_c(&name)), "SEL".to_string())
}

/// Why `selector` cannot be given a uniform-shape `perform` wrapper, or
/// `None` if it can.
///
/// The wrapper is `void *(*)(void *self, void *a0, void *a1)`, so the
/// selector's own arguments have to survive being passed as `void *` and
/// its result has to survive being handed back as one. Object and other
/// pointer types do; an `int` does not, and neither does a struct by
/// value. Real Objective-C's `-performSelector:` has the same restriction
/// -- it is typed `id (*)(id, SEL, ...)` -- but answers a signature
/// mismatch with garbage rather than a diagnostic.
fn unperformable_reason(program: &Program, selector: &str) -> Option<String> {
    let m = program.class_order.iter().find_map(|c| {
        program.classes[c].methods.iter().find(|m| m.selector == selector && !m.is_class_method)
    })?;
    if m.params.len() > 2 {
        return Some(format!(
            "it takes {} arguments, and '-performSelector:' passes at most two",
            m.params.len()
        ));
    }
    for (pname, ptype) in &m.params {
        let rendered = render_param(ptype, pname, program.root_class());
        if !rendered.contains('*') {
            return Some(format!(
                "its '{}' argument is '{}', which is not an object type",
                pname, ptype
            ));
        }
    }
    if m.return_type != "void" && !m.return_type.contains('*') && !m.returns_instancetype {
        return Some(format!(
            "it returns '{}', which is neither void nor an object type",
            m.return_type
        ));
    }
    None
}

/// `@protocol(Name)` -> the name of that protocol's generated conformance
/// bitmap.
///
/// Recording the use here rather than at the `-conformsToProtocol:` call
/// site is what keeps the footprint honest: exactly the protocols some
/// call site actually named get a bitmap, so enabling
/// `CONFIG_OBJZ_INTROSPECTION` and introspecting nothing costs nothing.
fn render_protocol_literal(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let Some(name) = protocol_literal_name(node, ctx.src) else {
        ctx.err(node, "'@protocol(...)' needs a single protocol name");
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    };
    if !ctx.program.protocols.contains_key(&name) {
        ctx.err(
            node,
            format!(
                "'@protocol({})' names no protocol declared in this program",
                name
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }
    ctx.introspection_used.protocols.insert(name.clone());
    (format!("oz_proto_{}", name), "const uint32_t *".to_string())
}

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
pub(crate) fn is_boxed_string_literal(node: Node) -> bool {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|c| c.kind() == "@");
    found
}

/// Desugars a boxed string literal `@"..."` the same way the Python
/// pipeline's oracle does (see `tools/oz_transpile/emit.py`'s
/// `ObjCStringLiteral` handling): NOT a class-method call -- OZString's
/// ivars (`_length`/`_data`) are all compile-time-computable and its
/// `dealloc` is a no-op (see `src/OZString.m`), so the literal desugars
/// directly to a static, immortal `struct OZString` instance plus a
/// cast-to-pointer expression at the use site. There was a third ivar,
/// `_hash`, which this initializer set to `0` and nothing ever read --
/// removed in #371 along with the `int _refcount` in `OZObject` that was
/// dead the same way.
/// Each unique literal gets its own instance (no dedup, unlike the Python
/// oracle -- a spike simplification; duplicates cost `sizeof(struct
/// OZString)` each, 24 bytes on `mps2/an385`, and not correctness). That
/// cost is `.rodata` only since #373 made the instance `const`; before
/// that it was `datas`, so a duplicate was charged against RAM as well --
/// which is what #372 is measured against. A plain (non-`@`) string literal
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
    let prototype = format!("extern const struct OZString {};\n", name);
    // `_meta.immortal = 1` is what keeps this literal alive. It lives in
    // static storage, so `free()`-ing it aborts -- and something does try:
    // `companion`'s release path runs `{class}_oz_free` once a refcount hits
    // zero, and a literal's refcount does reach zero, because a collection
    // that absorbed it (`@[ @"a" ]`, or a dictionary key) releases its
    // elements when it is itself deallocated. `oz_static_release` returns on
    // the immortal bit before it even decrements, matching the real
    // `OZString.m`'s own `-dealloc` ("compile-time constant, never freed")
    // and the oracle's `emit.py` literal, which sets the same bit.
    //
    // This used to set `deallocating = 1` from birth instead, relying on the
    // re-entrancy guard to make release a no-op. That worked, but the field
    // said something false -- `deallocating` means "teardown is running right
    // now", not "never tear down" -- and it let the literal's refcount sink
    // to zero and below on the way (#228).
    //
    // `const`, and deliberately (#373). Nothing writes a literal any more:
    // release returns on the immortal bit before its decrement, and since
    // #373 so does retain, which was the one remaining writer. That is what
    // lets the instance sit in `.rodata` instead of `datas` -- so a literal
    // costs flash only, and none of the RAM it used to hold constants in.
    //
    // The use-site cast below therefore discards `const`. Forming that cast
    // is legal C; writing through it would be undefined, and the only two
    // functions that ever could are the two named above. Do not "fix" the
    // cast by dropping the `const` here: that hands every literal back its
    // RAM. The methods take `struct OZString *` because one layout has to
    // serve literals and heap strings alike, which is the same reason the
    // refcount field survives at all.
    let definition = format!(
        "const struct OZString {} = {{ .base = {{ ._meta = {{ .class_id = OZ_STATIC_CLASS_OZString, .immortal = 1 }}, .oz_refcount = 1 }}, ._length = {}, ._data = {} }};\n",
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

/// Desugars a boxed array literal (`@[e1, e2, ...]`) into a call to
/// `OZArray_oz_initWithItems` (see `companion::render_array_support`) --
/// the same shape as the Python pipeline's `ObjCArrayLiteral` handling,
/// and since OZ-098 the same allocator behind it: the *stack* buffer
/// built here only carries the element pointers into the builder, which
/// copies them into a run of slots taken from the shared
/// `oz_item_pool`.
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
/// call to the `OZDictionary_oz_initWithKeysValues` builder
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
    // An array of objects indexes as ordinary C, and the result is one
    // element -- so the type has to come back as the element type, not as
    // `id` the way `pass_through` reports an unrecognised receiver. Getting
    // that wrong would leave `[_leaves[0] doThing]` dispatching through
    // `id` instead of statically (#287).
    if is_array_type(&recv_type) {
        let (index_text, _) = render_expr(index_node, ctx);
        return (format!("{}[{}]", recv_text, index_text), element_type(&recv_type).to_string());
    }
    // Indexing a C array of plain structs is ordinary C, and its element
    // type is spelled `struct Foo` just like a class's -- so the name has
    // to be checked against the program, or `points[0]` on a
    // `struct point points[3]` would be reported as a class that "does not
    // support subscripting".
    let class = match class_name_from_type(&recv_type) {
        Some(class) if ctx.program.is_class(&class) => class,
        _ => return pass_through(ctx),
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
    // `super[i]` has no meaning to reach for: a subscript's receiver is a
    // collection, and `super` is not one.
    send_to_resolved_class(node, ctx, &class, selector, &recv_text, &[index_text], false)
}

/// Is `node` the literal `super`? `render_expr` renders `super` to `self`
/// (the receiver really is `self`; only the dispatch target differs), so
/// this has to be asked of the node, before that.
fn is_super_identifier(node: Node, src: &str) -> bool {
    node.kind() == "identifier" && node_text(node, src) == "super"
}

/// Splits a `field_expression` into (object, field name), but only when it
/// is dot syntax on an Objective-C object -- `None` for anything that is
/// ordinary C member access and must pass through untouched.
///
/// Two things disqualify it. `a->b` is direct ivar access, which is already
/// valid C against the generated struct and means exactly what it says --
/// true of a *read*, and the reason this returns `None` for it, but not of
/// a store into an owned object ivar, which carries an ownership
/// obligation the plain C store does not discharge. `self->_x = value` is
/// picked up before this by `render_strong_ivar_assign` (see
/// `assigned_ivar_name`, #352); everything else about `->` still passes
/// through here. And
/// `a.b` where `a` is a plain C struct *value* is ordinary member access --
/// `samples/hello_category`'s `struct color`, or the `struct sensor_msg` in
/// `tests/behavior/cases/regression/issue_090_header_preservation.m`. Only
/// an object-typed left side makes the `.` Objective-C's: in C, `.` on a
/// pointer is not legal at all, so there is no ambiguity left to resolve.
fn dot_syntax_parts<'a>(
    node: Node<'a>,
    ctx: &mut EmitCtx,
) -> Option<(Node<'a>, String, String, String)> {
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    if !children.iter().any(|c| c.kind() == ".") {
        return None;
    }
    let object = *children.first()?;
    let field = *children.last()?;
    if field.kind() != "field_identifier" {
        return None;
    }
    let field_name = node_text(field, ctx.src).to_string();
    let (obj_text, obj_type) = render_expr(object, ctx);
    let class = class_name_from_type(&obj_type)?;
    // `struct point` and `struct Widget` are spelled identically; only the
    // program says which is a class. Without this a plain C struct's member
    // access was read as dot syntax and rejected as "'point' has no
    // property or getter named 'x'".
    if !ctx.program.is_class(&class) {
        return None;
    }
    Some((object, obj_text, class, field_name))
}

/// The accessor selector a property is reached through, which `getter=` /
/// `setter=` can rename to anything -- so the field name in source is not
/// necessarily the selector to call.
///
/// Falls back to the plain field name (and its `setX:` form), because
/// Objective-C also accepts dot syntax against a bare getter method with no
/// `@property` behind it at all.
fn accessor_selector(ctx: &EmitCtx, class: &str, field: &str, writing: bool) -> String {
    match ctx.program.find_property(class, field) {
        Some((_, prop)) if writing => prop
            .setter_sel
            .clone()
            .unwrap_or_else(|| crate::collect::default_setter_sel(&prop.name)),
        Some((_, prop)) => prop.getter_sel.clone().unwrap_or_else(|| prop.name.clone()),
        None if writing => crate::collect::default_setter_sel(field),
        None => field.to_string(),
    }
}

/// `obj.prop` -- Objective-C property dot syntax, read form, lowered to the
/// getter call: `[App sharedInstance].heap` becomes
/// `App_heap(App_sharedInstance_cls())`.
///
/// Chaining needs no special handling: `a.b.c` recurses, and the inner
/// call's return type is what resolves `c`'s class, exactly as it would for
/// a chain of message sends.
fn render_field_expression(node: Node, ctx: &mut EmitCtx) -> (String, String) {
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

    let Some((object, obj_text, class, field)) = dot_syntax_parts(node, ctx) else {
        return pass_through(ctx);
    };
    let super_access = is_super_identifier(object, ctx.src);
    let getter = accessor_selector(ctx, &class, &field, false);
    if find_defining_class(ctx.program, &class, &getter, false).is_none() {
        // Reaching a bare ivar through dot syntax is not Objective-C either
        // -- `.` is accessor syntax, and Clang rejects `obj.someIvar` the
        // same way. Rewriting it to `->` would compile, which is precisely
        // why it is not done: it would accept a program the language does
        // not, and quietly bypass whatever the accessor does.
        ctx.err(
            node,
            format!(
                "'{}' has no property or getter named '{}', so '{}' has no meaning on it",
                class,
                field,
                one_line(node_text(node, ctx.src))
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }
    send_to_resolved_class(node, ctx, &class, &getter, &obj_text, &[], super_access)
}

/// The ivar an assignment's left side names, whichever way the author
/// spelled it: `_x` or `self->_x`.
///
/// Both are the same operation, and until #352 only the first reached the
/// strong-store lowering -- this function opened with
/// `if left.kind() != "identifier" { return None; }`, and `self->_x` is a
/// `field_expression`. So the explicit spelling fell through to a plain C
/// store: no retain of the new value, no release of the old. The ivar was
/// left holding a reference nothing had accounted for, the local's own
/// scope-exit release destroyed the object immediately, and the
/// synthesized dealloc later released the freed block a second time. One
/// missing retain, three defects, and `-fsanitize=address` reports
/// `heap-use-after-free` inside `oz_static_release`.
///
/// Restricted to `self`. `other->_x = value` is direct ivar access on
/// *another* object, which needs that object's class to resolve the ivar
/// and its access path, and nothing in the tree writes it; it still falls
/// through, as it did before.
///
/// The read path is untouched and needs no equivalent: `render_field_expression`
/// leaves `a->b` alone deliberately, because as a *read* it is already
/// valid C against the generated struct and means exactly what it says.
/// Only a store carries an ownership obligation.
fn assigned_ivar_name(left: Node, ctx: &EmitCtx) -> Option<String> {
    if left.kind() == "identifier" {
        let name = node_text(left, ctx.src).to_string();
        /* A local of the same name shadows the ivar, exactly as in C. */
        if ctx.locals.contains(&name) {
            return None;
        }
        return Some(name);
    }
    if left.kind() != "field_expression" {
        return None;
    }
    let mut cursor = left.walk();
    let children: Vec<Node> = left.children(&mut cursor).collect();
    /* `self->_x` only: dot syntax on an object is a property store and is
     * handled further down `render_assignment_expression`, which sends the
     * setter -- and a setter already retains. */
    if !children.iter().any(|c| c.kind() == "->") {
        return None;
    }
    let object = children.first()?;
    if object.kind() != "identifier" || node_text(*object, ctx.src) != "self" {
        return None;
    }
    let field = children.last()?;
    if field.kind() != "field_identifier" {
        return None;
    }
    Some(node_text(*field, ctx.src).to_string())
}

/// `_ivar = value` where `_ivar` is a strong object ivar: takes ownership of
/// the new value and gives up the old one, the way assigning to a `__strong`
/// ivar does under ARC. `None` when this is not that.
///
/// oz_static had the release half of strong-ivar ownership without the
/// retain half: `{Class}_oz_release_ivars` releases every owned object ivar
/// when an instance dies, but nothing ever retained what was stored there.
/// `samples/transpiled_led` is a chain of six OZHelpers, each holding the
/// previous one in a strong `_next` ivar assigned straight from a parameter,
/// and it segfaulted -- AddressSanitizer named it exactly:
/// heap-use-after-free in `oz_atomic_dec_and_test`, the object freed once by
/// its owner's `oz_release_ivars` and again by the scope-exit release of the
/// local that created it. Releasing a reference never taken is a double
/// free, so the two halves have to match: retain exactly what dealloc will
/// release, which is why the predicate here is
/// `Program::owned_object_ivar_names` -- the same list that path uses.
///
/// Properties were never affected: a synthesized setter already does
/// retain-new/release-old (`render_synthesized_accessor`). Among *ivars*, only
/// direct assignment was missing it. A plain strong **local** was missing it
/// too, which is a separate storage class and a separate fix
/// (`render_strong_local_assign`, #234) -- worth saying explicitly, because
/// this comment used to read as though locals were already covered.
///
/// A `+1` right-hand side is stored without retaining, because it already
/// carries the reference the ivar is taking over -- retaining it as well
/// would leak, since a temporary has no scope-exit release to balance it.
/// Everything else is borrowed and gets retained; where that value is also
/// an owned local, its own scope-exit release keeps the count right.
///
/// `+1` is `arc::binds_ownership`, so a cast does not hide it:
/// `_kid = (Thing *)[Thing alloc];` used to be read as borrowed and get a
/// retain it had no business having, which left the allocation at +2 with
/// one release ever to come -- a leak (#332).
///
/// Emitted as a comma expression over a temporary rather than several
/// statements, so it stays usable wherever an assignment was, and in the
/// same order the synthesized setter uses: assign, retain new, release old.
/// That order is what makes self-assignment (`_x = _x`) safe -- releasing
/// first could free the value being stored.
fn render_strong_ivar_assign(
    node: Node,
    left: Node,
    right: Node,
    ctx: &mut EmitCtx,
) -> Option<(String, String)> {
    let name = assigned_ivar_name(left, ctx)?;
    if !ctx.program.owned_object_ivar_names(&ctx.class_name).contains(&name) {
        return None;
    }
    let path = ctx.program.ivar_access_path(&ctx.class_name, &name)?;
    let root = ctx.program.root_class()?.to_string();

    let takes_ownership = crate::arc::binds_ownership(right, ctx.src, ctx.program, &ctx.program.owning_methods);
    let (value, value_ty) = render_expr(right, ctx);

    let (line, col) = line_col(ctx.src, node.start_byte());
    ctx.block_counter += 1;
    let prev = format!("_oz_prev_L{}_C{}_{}", line, col, ctx.block_counter);
    ctx.pre_stmts.push(format!(
        "struct {root} *{prev} = (struct {root} *)(self->{path});",
        root = root,
        prev = prev,
        path = path
    ));

    let retain = if takes_ownership {
        String::new()
    } else {
        format!("oz_static_retain((struct {root} *)(self->{path})), ", root = root, path = path)
    };
    // The comma expression yields the stored value only where something can
    // use it. As a bare statement -- which is nearly always -- a trailing
    // `self->_x` is a read whose result is discarded, and Clang says so:
    // "expression result unused" [-Wunused-value]. Zephyr builds with
    // -Werror, so that is a build failure, not just noise.
    let yields = if node
        .parent()
        .is_some_and(|parent| parent.kind() == "expression_statement")
    {
        String::new()
    } else {
        format!(", self->{path}", path = path)
    };
    let expr = format!(
        "(self->{path} = {value}, {retain}oz_static_release({prev}){yields})",
        path = path,
        value = value,
        retain = retain,
        prev = prev,
        yields = yields
    );
    let ty = if value_ty == "id" { format!("struct {} *", root) } else { value_ty };
    Some((expr, ty))
}

/// Assignment to a strong object *local*: release what it held, so the
/// variable holds at most one live object at a time.
///
/// This is what real ARC does at every store to a strong variable, and its
/// absence was the reason `staticbar` had to reject an ordinary loop:
///
/// ```objc
/// Counter *c;
/// for (int i = 0; i < 100; i++) {
///         c = [Counter alloc];   /* previous c released here */
/// }
/// ```
///
/// Without the release each iteration abandoned a live object, so 100
/// iterations needed 100 slab slots while `pools::count_sites` had counted
/// the one allocation site once. The slab ran out and the next send wrote
/// through a null receiver -- an MPU fault on target, and a *silent* one,
/// since nothing about it fails to compile. With the release, one slot is
/// genuinely correct and the shape needs no diagnostic at all.
///
/// The ordering is `render_strong_ivar_assign`'s, for the same reason:
/// assign, retain new, release old. Releasing first could free the very
/// value being stored, which is what makes self-assignment (`c = c`) safe.
/// A `+1` right-hand side is stored without retaining -- it already carries
/// the reference the variable is taking over, and a temporary has no
/// scope-exit release to balance a second one.
///
/// Membership in `arc_managed_locals` is what guarantees the release is
/// sound: every value such a local can hold is owned, so there is never a
/// reference released that was not taken. See `managed_object_locals`.
///
/// Takes no `node`, unlike `render_strong_ivar_assign`: that one needs the
/// assignment's position to name a temporary, and this one deliberately
/// emits no temporary at all.
/// Is `name` a file-scope object variable -- a strong slot that no scope
/// owns?
///
/// Identified rather than tabulated: `emit::file_scope_vars` has already
/// put every one of them into `ctx.scope` with its C type (that is how a
/// send to one resolves its receiver), and a file-scope name is by
/// definition absent from `ctx.locals`. An ivar is in `scope` too, so it
/// has to be excluded explicitly -- an *owned* one never reaches here,
/// since `render_strong_ivar_assign` runs first, but an
/// `__unsafe_unretained` one does and must keep its plain store.
fn is_file_scope_object(name: &str, ctx: &EmitCtx) -> bool {
    if ctx.locals.contains(name) {
        return false;
    }
    if ctx.program.ivar_access_path(&ctx.class_name, name).is_some() {
        return false;
    }
    ctx.scope.get(name).and_then(|ty| class_name_from_type(ty)).is_some()
}

fn render_strong_local_assign(
    left: Node,
    right: Node,
    ctx: &mut EmitCtx,
) -> Option<(String, String)> {
    if left.kind() != "identifier" {
        return None;
    }
    let name = node_text(left, ctx.src).to_string();
    /* Three kinds of strong slot, one lowering. A managed *local* is also
     * released when its scope ends; the other two are not, and that is the
     * only difference between them -- so the store is identical and lives
     * here rather than being written twice (#359). */
    let is_slot = ctx.arc_managed_locals.contains(&name)
        || ctx.arc_managed_slots.contains(&name)
        || is_file_scope_object(&name, ctx);
    if !is_slot {
        return None;
    }
    let root = ctx.program.root_class()?.to_string();
    let kind = classify_store(&name, right, ctx.src, ctx.program);
    if kind == LocalStore::Unsupported {
        return None;
    }
    let (value, _value_ty) = render_expr(right, ctx);

    // No temporary, deliberately. An earlier version captured the previous
    // value into a `ctx.pre_stmts` local, which is drained by whichever
    // *top-level* statement is being rendered -- so for an assignment inside
    // a loop the capture was hoisted above the `for`, read `c` once while it
    // was still nil, and every iteration then released nil. The loop leaked
    // exactly as before, and the generated C looked plausible. Naming `c`
    // directly inside the comma expression is both simpler and correct: the
    // comma operator sequences left to right, so a release written before
    // the assignment observes the old value.
    let expr = match kind {
        // A `+1` right-hand side that does not mention the variable: release
        // first, then assign. Releasing *before* the allocation is what lets
        // one slab slot serve the whole loop -- the slot goes back to the
        // slab and the very next allocation can take it again. Allocating
        // first would need two slots live at once. The right-hand side is
        // known not to read the variable (`classify_store`), so freeing it
        // first cannot pull the ground from under the value being computed.
        LocalStore::Owning => format!(
            "(oz_static_release((struct {root} *)({name})), {name} = {value})",
            root = root,
            name = name,
            value = value
        ),
        // A plain identifier: retain new, release old, assign -- the order
        // `render_strong_ivar_assign` uses and for the same reason, that it
        // makes self-assignment (`c = c`) safe. Naming the value twice is
        // free of consequence only because it is an identifier, which is
        // exactly what `classify_store` checked.
        LocalStore::BorrowedIdent => format!(
            "(oz_static_retain((struct {root} *)({value})), \
             oz_static_release((struct {root} *)({name})), {name} = {value})",
            root = root,
            name = name,
            value = value
        ),
        LocalStore::Unsupported => unreachable!("returned above"),
    };
    // The comma expression already yields the assigned value, so unlike the
    // ivar path there is no trailing read to suppress for `-Wunused-value`.
    let ty = ctx.scope.get(&name).cloned().unwrap_or_else(|| format!("struct {} *", root));
    Some((expr, ty))
}

/// `_leaves[i] = value;` where `_leaves` is an owned array of objects:
/// release what the slot held, then store, so the array owns its elements
/// the way a strong ivar owns its one (#287).
///
/// Without this the store is plain C and the slot's previous value is
/// simply dropped -- a leak on every overwrite, and one that
/// `{Class}_oz_release_ivars` cannot make up for, since by `-dealloc` the
/// overwritten references are already unreachable.
///
/// Ordering and the comma expression are `render_strong_local_assign`'s,
/// for its reasons: releasing *before* a `+1` right-hand side lets one slab
/// slot serve a whole loop, and naming the target twice is only safe
/// because both the index and the value are checked to be side-effect free
/// first. No temporary, deliberately -- `ctx.pre_stmts` is drained by the
/// enclosing *top-level* statement, so a temporary written for a store
/// inside a loop is hoisted above it and reads the slot once, before the
/// loop, which is exactly the bug that comment records.
///
/// Three shapes are accepted, and anything else is a located error rather
/// than a silent plain-C store:
///
///   - a `+1` value (`[[Leaf alloc] init]`) -- release the slot, then store
///   - a plain identifier -- retain, release the slot, then store
///   - `nil` -- release the slot and clear it
fn render_strong_array_element_assign(
    node: Node,
    left: Node,
    right: Node,
    ctx: &mut EmitCtx,
) -> Option<(String, String)> {
    if left.kind() != "subscript_expression" {
        return None;
    }
    let mut cursor = left.walk();
    let parts: Vec<Node> = left.children(&mut cursor).collect();
    let open = parts.iter().position(|c| c.kind() == "[")?;
    let close = parts.iter().position(|c| c.kind() == "]")?;
    let recv_node = parts.first().copied().filter(|_| open > 0)?;
    let index_node = parts.get(open + 1).copied().filter(|_| open + 1 < close)?;

    /* Only an *ivar* array is owned. A local array of objects has no
     * scope-exit release to pair with, and a parameter's storage belongs to
     * the caller.
     *
     * Both spellings of the ivar reach this, through the same extractor
     * `render_strong_ivar_assign` uses: `_arr[i] = v` and
     * `self->_arr[i] = v`. Only the bare one did until #360, because this
     * required the subscript's receiver to be an `identifier` and
     * `self->_arr` is a `field_expression` -- so the explicit spelling
     * fell through to a plain C store, took no retain, and released
     * nothing, leaving the element dangling once the local's scope ended.
     *
     * Third site of one cause: #351 keyed on the returned name, #352 on
     * the shape of a scalar ivar store's left side, this on the shape of
     * an array store's receiver. The lesson each time is that routing
     * every spelling through one function is what stops the next site
     * appearing, so this reuses `assigned_ivar_name` rather than adding a
     * `field_expression` arm of its own.
     *
     * The emitted target is unaffected either way: it is rebuilt from
     * `ivar_access_path` below, so both spellings produce the identical
     * `self->_arr[i]`. */
    let ivar = assigned_ivar_name(recv_node, ctx)?;
    let recv_type = ctx.scope.get(&ivar)?.clone();
    if !is_array_type(&recv_type) {
        return None;
    }
    let elem = element_type(&recv_type).to_string();
    /* An array of ints owns nothing. */
    if class_name_from_type(&elem).is_none() && elem.trim() != "void *" {
        return None;
    }
    if ctx.program.array_extent_of(&ctx.class_name, &ivar).is_none() {
        return None;
    }
    if !ctx.program.owned_object_ivar_names(&ctx.class_name).iter().any(|n| *n == ivar) {
        /* `__unsafe_unretained`, or a type Clang did not call an owned
         * object: the slot is a borrow, and releasing a borrow is the
         * double free the qualifier exists to prevent. */
        return None;
    }

    let root = ctx.program.root_class()?.to_string();

    /* The target is named twice by the comma expression, so the index is
     * evaluated twice. Only a literal or a plain identifier is provably the
     * same both times -- the same rule, and the same reason, as the
     * compound-assignment restriction on a dot-syntax receiver. */
    let index_ok = matches!(index_node.kind(), "number_literal" | "identifier");
    if !index_ok {
        ctx.err(
            node,
            format!(
                "the index of an owned array element store is evaluated twice \
                 (to release the old element and then assign), so it must be a \
                 literal or a plain variable -- '{}' is neither. Read it into a \
                 local first",
                one_line(node_text(index_node, ctx.src))
            ),
        );
        return Some((node_text(node, ctx.src).to_string(), elem));
    }
    let (index_text, _) = render_expr(index_node, ctx);
    let path = ctx.program.ivar_access_path(&ctx.class_name, &ivar)?;
    let target = format!("self->{}[{}]", path, index_text);

    if is_null_initializer(right, ctx.src) {
        let expr = format!(
            "(oz_static_release((struct {root} *)({target})), {target} = ((void *)0))",
            root = root,
            target = target
        );
        return Some((expr, elem));
    }

    let kind = classify_store(&ivar, right, ctx.src, ctx.program);
    if kind == LocalStore::Unsupported {
        ctx.err(
            node,
            format!(
                "'{}' stores into an owned array element from an expression this \
                 backend cannot balance: it is neither a `+1` value, a plain \
                 variable, nor nil. Assign it to a local first, then store the local",
                one_line(node_text(node, ctx.src))
            ),
        );
        return Some((node_text(node, ctx.src).to_string(), elem));
    }
    let (value, _) = render_expr(right, ctx);

    let expr = match kind {
        LocalStore::Owning => format!(
            "(oz_static_release((struct {root} *)({target})), {target} = {value})",
            root = root,
            target = target,
            value = value
        ),
        LocalStore::BorrowedIdent => format!(
            "(oz_static_retain((struct {root} *)({value})), \
             oz_static_release((struct {root} *)({target})), {target} = {value})",
            root = root,
            target = target,
            value = value
        ),
        LocalStore::Unsupported => unreachable!("returned above"),
    };
    Some((expr, elem))
}

/// Refuse to store a reference ARC manages into a plain **C struct**
/// field, rather than emitting the plain store that silently frees it
/// (#359).
///
/// The three strong slots oz_static tracks -- an ivar, a managed local,
/// and a file-scope or `static` slot -- all reach the lowerings above. A C
/// struct's field reaches none of them, so the store was plain C and
/// whatever ARC was managing on the right-hand side was released when its
/// scope ended, leaving the field pointing at freed memory.
///
/// Supporting it is not a small change: the field's type is not resolved
/// today -- which is why sending a message *to* one is already a located
/// error (#355) -- and a strong field would additionally have to be
/// released when the struct itself dies, which nothing tracks. ARC does
/// support this (Clang reports such a field as `__strong` and generates
/// destroy helpers for the struct), so this is a subset boundary rather
/// than a semantic disagreement, and it belongs on the error side of it:
/// a located refusal beats a silent wrong free.
///
/// Only a reference ARC would release is refused. That is the exact set
/// that produces the wrong free:
///
///   - a `+1` expression (`b.held = [[Thing alloc] init];`), and
///   - an identifier naming a managed local or slot (`b.held = a;`),
///     which is the shape the audit actually caught, since a bare
///     identifier is borrowed *by shape* and passes `binds_ownership`.
///
/// A genuinely borrowed store -- a parameter, an unretained ivar -- is
/// left alone: nothing releases it, so the field is an unowned reference
/// and that is the author's business, exactly as it is in C.
fn reject_owning_store_into_c_struct(node: Node, left: Node, right: Node, ctx: &mut EmitCtx) {
    if left.kind() != "field_expression" {
        return;
    }
    /* Dot syntax on an object is a property store, handled below by the
     * setter path; `self->_ivar` was taken by the ivar lowering already. */
    if dot_syntax_parts(left, ctx).is_some() {
        return;
    }
    if assigned_ivar_name(left, ctx).is_some() {
        return;
    }
    let managed = crate::arc::binds_ownership(
        right,
        ctx.src,
        ctx.program,
        &ctx.program.owning_methods,
    ) || {
        let behind = crate::arc::value_behind_casts(right, ctx.src);
        behind.kind() == "identifier" && {
            let name = node_text(behind, ctx.src).to_string();
            ctx.arc_managed_locals.contains(&name) || ctx.arc_managed_slots.contains(&name)
        }
    };
    if !managed {
        return;
    }
    /* Two different targets reach here and they point at different fixes,
     * so they get different messages: an ivar of *another* object, where
     * the receiver is object-typed, and a plain C struct's field, where it
     * is not. */
    let mut cursor = left.walk();
    let children: Vec<Node> = left.children(&mut cursor).collect();
    let receiver_class = children
        .first()
        .map(|object| render_expr_type_only(*object, ctx))
        .and_then(|ty| class_name_from_type(&ty))
        .filter(|class| ctx.program.is_class(class));

    match receiver_class {
        Some(class) => ctx.err(
            node,
            format!(
                "storing a reference ARC manages into another object's ivar is not \
                 supported; `{class}` would have to take ownership of it, and only a \
                 store through `self` does that. The value is released when its scope \
                 ends, which would leave the ivar dangling -- assign it through a \
                 setter or a method on `{class}` instead"
            ),
        ),
        None => ctx.err(
            node,
            "storing a reference ARC manages into a plain C struct field is not \
             supported; the field's ownership cannot be tracked, so the value would be \
             released when its scope ends and the field left dangling. Store it in an \
             ivar, or declare the field __unsafe_unretained to say the struct does not \
             own it"
                .to_string(),
        ),
    }
}

/// The static type `render_expr` would report for `node`, without keeping
/// the rendered text.
///
/// A separate helper because rendering has side effects on the context
/// (`pre_stmts`, `block_counter`), and a *diagnostic* must not leave any
/// behind -- the message is the whole output of this path.
fn render_expr_type_only(node: Node, ctx: &mut EmitCtx) -> String {
    let saved_pre = std::mem::take(&mut ctx.pre_stmts);
    let saved_counter = ctx.block_counter;
    let (_text, ty) = render_expr(node, ctx);
    ctx.pre_stmts = saved_pre;
    ctx.block_counter = saved_counter;
    ty
}

/// Assignment, handled here for three reasons: a property dot-syntax *target*
/// has to become the setter call rather than an assignment to a function
/// call, a strong object ivar has to take ownership of what it is given
/// (`render_strong_ivar_assign`), and a strong object *local* has to release
/// what it held (`render_strong_local_assign`). Every other assignment passes
/// through as the C it already is.
///
/// A compound assignment (`+=`, `<<=`, ...) has to read the property and
/// write it back, which mentions the receiver twice -- so it is only
/// accepted when the receiver is a plain identifier (or `self`), where
/// evaluating it twice provably cannot differ. `[obj thing].count += 1`
/// stays a hard error instead of silently sending `thing` twice.
fn render_assignment_expression(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
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

    let (Some(left), Some(op), Some(right)) =
        (children.first().copied(), children.get(1).copied(), children.last().copied())
    else {
        return pass_through(ctx);
    };
    if children.len() < 3 {
        return pass_through(ctx);
    }
    if node_text(op, ctx.src) == "=" {
        if let Some(rendered) = render_strong_ivar_assign(node, left, right, ctx) {
            return rendered;
        }
        if let Some(rendered) = render_strong_local_assign(left, right, ctx) {
            return rendered;
        }
        if let Some(rendered) = render_strong_array_element_assign(node, left, right, ctx) {
            return rendered;
        }
        reject_owning_store_into_c_struct(node, left, right, ctx);
    }
    if left.kind() != "field_expression" {
        return pass_through(ctx);
    }
    let operator = node_text(op, ctx.src).to_string();
    let Some((object, obj_text, class, field)) = dot_syntax_parts(left, ctx) else {
        return pass_through(ctx);
    };

    let setter = accessor_selector(ctx, &class, &field, true);
    if find_defining_class(ctx.program, &class, &setter, false).is_none() {
        let readonly = ctx
            .program
            .find_property(&class, &field)
            .is_some_and(|(_, prop)| prop.is_readonly);
        ctx.err(
            node,
            if readonly {
                format!(
                    "'{}.{}' is a readonly property, so '{}' cannot assign to it",
                    class,
                    field,
                    one_line(node_text(node, ctx.src))
                )
            } else {
                format!(
                    "'{}' has no property or setter named '{}', so '{}' has no meaning on it",
                    class,
                    field,
                    one_line(node_text(node, ctx.src))
                )
            },
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }

    let super_access = is_super_identifier(object, ctx.src);
    let (right_text, _) = render_expr(right, ctx);
    if operator == "=" {
        return send_to_resolved_class(node, ctx, &class, &setter, &obj_text, &[right_text], super_access);
    }

    // Compound: read, combine, write back -- so the receiver appears twice.
    if object.kind() != "identifier" {
        ctx.err(
            node,
            format!(
                "'{}' needs to read '{}' and write it back, which would evaluate the receiver '{}' twice -- assign through a local instead",
                one_line(node_text(node, ctx.src)),
                field,
                one_line(node_text(object, ctx.src))
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }
    let getter = accessor_selector(ctx, &class, &field, false);
    if find_defining_class(ctx.program, &class, &getter, false).is_none() {
        ctx.err(
            node,
            format!(
                "'{}' needs to read '{}' first, but '{}' has no property or getter of that name",
                one_line(node_text(node, ctx.src)),
                field,
                class
            ),
        );
        return (node_text(node, ctx.src).to_string(), "id".to_string());
    }
    let (read, _) = send_to_resolved_class(node, ctx, &class, &getter, &obj_text, &[], super_access);
    // `x += y` is `x = x + y`: drop the trailing `=` to get the operator.
    let binary_op = operator.trim_end_matches('=').to_string();
    let combined = format!("{} {} ({})", read, binary_op, right_text);
    send_to_resolved_class(node, ctx, &class, &setter, &obj_text, &[combined], super_access)
}

/// One instance send whose receiver's class is already resolved, routed by
/// the same rule as `render_message`'s resolved-receiver branch: a direct
/// call when no subclass overrides the selector, the `class_id` switch when
/// one does (see `Program::has_overriding_subclass`).
///
/// Used by desugarings that synthesize a send rather than translating a
/// literal `[recv sel:...]` -- subscripting and property dot syntax.
/// Deliberately does not handle class methods, which need care
/// `render_message` already takes.
///
/// `super_send` carries the one thing the receiver *text* cannot: `super`
/// renders to `self`, so by the time there is a string left there is no way
/// to tell the two apart, and they dispatch differently. See the two places
/// it is consulted below; `render_message` applies the same two rules for a
/// literal `[super sel]`.
fn send_to_resolved_class(
    node: Node,
    ctx: &mut EmitCtx,
    class: &str,
    selector: &str,
    recv_text: &str,
    arg_texts: &[String],
    super_send: bool,
) -> (String, String) {
    let root = ctx.program.root_class().unwrap_or("OZSRoot").to_string();
    // A `super` access names one specific implementation by definition, so
    // it must stay a direct call. Routing it through the receiver's own
    // class_id would re-enter the override that issued it -- for a property
    // getter, a subclass override reading `super.thing` would call itself
    // forever.
    if !super_send && ctx.program.has_overriding_subclass(class, selector) {
        reject_ambiguous_dispatch(node, ctx, Some(class), selector);
        return dynamic_dispatch_call(ctx.program, &root, selector, recv_text, arg_texts);
    }
    let Some(defining) = find_defining_class(ctx.program, class, selector, false) else {
        reject_ambiguous_dispatch(node, ctx, Some(class), selector);
        return dynamic_dispatch_call(ctx.program, &root, selector, recv_text, arg_texts);
    };
    let (ret_ty, returns_instancetype) = method_return_type(ctx.program, &defining, selector, false)
        .unwrap_or_else(|| ("void".to_string(), false));
    let mut call_args = vec![format!("(struct {} *)({})", defining, recv_text)];
    call_args.extend(arg_texts.iter().cloned());
    let call =
        format!("{}({})", method_fn_name(&defining, selector, false), call_args.join(", "));
    // For a `super` access the real receiver is still `self`, so an
    // `instancetype` result covaries with this class, not with the
    // superclass the call was resolved against.
    let covariant_target = if super_send { ctx.class_name.clone() } else { class.to_string() };
    if returns_instancetype && defining != covariant_target {
        (
            format!("(struct {} *)({})", covariant_target, call),
            format!("struct {} *", covariant_target),
        )
    } else {
        (call, ret_ty)
    }
}

/// `@synchronized(obj) { body }` lowered to a scoped critical section over
/// `obj`'s *own* lock:
///
/// ```c
/// { /* @synchronized(obj) */
///     struct OZObject *_oz_sync_obj_... = (struct OZObject *)(obj);
///     int _oz_sync_held_... = (_oz_sync_obj_...->oz_sync_owner != oz_current_thread());
///     oz_spinlock_key_t _oz_sync_key_... = oz_spin_key_none();
///     if (_oz_sync_held_...) {
///         _oz_sync_key_... = oz_spin_lock(&_oz_sync_obj_...->oz_sync_lock);
///         _oz_sync_obj_...->oz_sync_owner = oz_current_thread();
///     }
///     oz_static_retain(_oz_sync_obj_...);
///     ... body ...
///     oz_static_release(_oz_sync_obj_...);
///     if (_oz_sync_held_...) {
///         _oz_sync_obj_...->oz_sync_owner = (void *)0;
///         oz_spin_unlock(&_oz_sync_obj_...->oz_sync_lock, _oz_sync_key_...);
///     }
/// }
/// ```
///
/// The lock is a field of the object (`SYNC_LOCK_FIELD` in the root struct,
/// present only when the program uses `@synchronized`), so two threads
/// synchronizing on the same object contend on the same lock. The receiver is
/// bound to a temporary and evaluated exactly once -- it is named four times
/// here, and `@synchronized([App sharedInstance])` must not send the message
/// four times.
///
/// **This used to be a lock declared inside the block, on the caller's own
/// stack, fresh per call**, matching the Python pipeline's per-block
/// `OZSpinLock` (`emit.py::_emit_synchronized_stmt`). Two threads then locked
/// two different locks, so it bought an interrupt-disabled critical section
/// and no mutual exclusion keyed on `obj` at all. It looked correct because
/// `k_spin_lock` calls `arch_irq_lock()` unconditionally, which on a single
/// core does serialize the section -- and every board in use was single-core.
/// Measured on two cores it was indistinguishable from no lock:
/// `count=2015 expected=4000` against `2023` unlocked (`samples/smp_shared`,
/// gap W of the retired PARITY.md; see docs/STATUS.md).
///
/// `oz_sync_owner` is what makes the per-object lock safe, and it is not a
/// recursive lock -- a `k_spinlock` cannot be one. A re-entrant
/// `@synchronized` on the same object *does not attempt the second acquire*:
/// it sees itself as owner and skips both lock and unlock. `held` is a
/// per-block local, so nesting unwinds correctly at any depth with no counter,
/// since inner blocks never acquired. Without this, the oracle's own
/// `tests/behavior/cases/synchronized/nested.m` shape -- two receivers that
/// may alias, as `[n runNested:n]` does -- would deadlock on hardware while
/// passing on host, where `oz_spin_lock` is a no-op.
///
/// That is checked rather than reasoned about since #278:
/// `just test-spin-validate` enables `CONFIG_SPIN_VALIDATE`, under which a
/// second acquire fails Zephyr's own `z_spin_lock_valid()`. Removing the
/// owner check makes `samples/smp_shared` (two cores) and
/// `samples/pool_demo` (one core, nesting across a method boundary) both
/// report `ASSERTION FAIL [z_spin_lock_valid(l)] ... Invalid spinlock`.
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
/// The root-struct field that `@synchronized` locks. One per object, so two
/// threads synchronizing on the same object contend on the same lock -- which
/// a per-block lock on each caller's own stack could never do.
pub(crate) const SYNC_LOCK_FIELD: &str = "oz_sync_lock";

/// The root-struct field recording which thread holds `oz_sync_lock`, so a
/// re-entrant `@synchronized` on the same object skips the acquire instead of
/// deadlocking on a spinlock it already holds. Zero when the lock is free,
/// which is why `oz_current_thread()` must never return NULL.
pub(crate) const SYNC_OWNER_FIELD: &str = "oz_sync_owner";

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
    let key = format!("_oz_sync_key_{}", suffix);

    // Held across the body so the object can't be deallocated mid-section,
    // mirroring the retain/release the oracle's OZSpinLock does in its
    // -initWithObject:/-dealloc pair.
    // The receiver is bound to a temporary and evaluated exactly once. It
    // has to be: the lock, the retain, the release and the unlock all name
    // it, and `@synchronized([App sharedInstance]) { ... }` must not send
    // the message four times. The previous per-block form evaluated it
    // twice, which was already one too many.
    let obj_var = format!("_oz_sync_obj_{}", suffix);
    let held = format!("_oz_sync_held_{}", suffix);
    let bind = format!("struct {} *{} = (struct {} *)({});", root, obj_var, root, obj_text);
    let retain = format!("oz_static_retain({});", obj_var);
    // Only the block that actually acquired the lock releases it. `held` is a
    // per-block local, so nesting to any depth unwinds correctly without a
    // counter: the inner blocks never acquired and never unlock.
    let cleanup = format!(
        "oz_static_release({obj});\n\
         \tif ({held}) {{\n\
         \t\t{obj}->{owner_field} = (void *)0;\n\
         \t\toz_spin_unlock(&{obj}->{lock_field}, {key});\n\
         \t}}",
        obj = obj_var,
        held = held,
        owner_field = SYNC_OWNER_FIELD,
        lock_field = SYNC_LOCK_FIELD,
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
             \t{bind}\n\
             \tint {held} = ({obj}->{owner_field} != oz_current_thread());\n\
             \toz_spinlock_key_t {key} = oz_spin_key_none();\n\
             \tif ({held}) {{\n\
             \t\t{key} = oz_spin_lock(&{obj}->{lock_field});\n\
             \t\t{obj}->{owner_field} = oz_current_thread();\n\
             \t}}\n\
             \t{retain}\n\
             \t{body}\n\
             \t{cleanup}\n\
             }}",
            bind = bind,
            held = held,
            key = key,
            obj = obj_var,
            owner_field = SYNC_OWNER_FIELD,
            lock_field = SYNC_LOCK_FIELD,
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
///
/// Pending ARC releases reach the same temporary, and are the far more
/// common way to get here -- `@synchronized` only named the shape first,
/// and the temporary's name still says `sync` for that reason alone.
/// Either way it is typed from `ctx.method_return_type`, which whoever
/// renders the enclosing body has to have recorded: a method in
/// `render_method_definition`, a free function in `walk_top_level`'s
/// `function_definition` arm (#336), a block literal in `render_block`
/// (#339) -- which also has to put the enclosing body's type back
/// afterwards, since it borrows that body's `EmitCtx`.
fn render_return_statement(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    // A local being returned hands its ownership to the caller, so it is
    // the one thing a return must not release.
    //
    // Read from behind whatever parentheses and non-bridging casts the
    // return is written behind (#332). `return (Thing *)t;` used to find no
    // identifier at all, so the returned local was released on the way out
    // and the caller was handed a freed pointer -- a use-after-free where
    // the uncast `return t;` is correct, and the cast changes the static
    // type and nothing about who owns the reference.
    // `arc::return_hands_back_ownership` peels with the same helper and has
    // to: whichever local this decides not to release is the one whose
    // ownership it tells the caller to take over.
    let mut cursor0 = node.walk();
    let returned_children: Vec<Node> = node.children(&mut cursor0).collect();
    let returned_name = returned_children
        .iter()
        .find(|c| c.kind() != "return" && c.kind() != ";")
        .map(|c| crate::arc::value_behind_casts(*c, ctx.src))
        .filter(|c| c.kind() == "identifier")
        .map(|c| node_text(c, ctx.src).to_string());
    // ...and the returned *name* is not always the name that owns it.
    //
    // `Thing *b = a; return b;` hands the caller `a`'s reference, so
    // releasing `a` here drops the only one there is -- the whole of #351,
    // and a use-after-free rather than a leak. What must be kept is the
    // local the returned name *aliases*, which `arc::alias_chain` finds by
    // following plain-identifier initialisers. `return_hands_back_ownership`
    // walks the same chain, so the two still agree on which local's
    // ownership passes to the caller.
    //
    // Nothing is retained for this: the alias and the owner are the same
    // reference, so keeping the owner instead of the alias is exact and
    // costs no refcount traffic at all. That matters more than it looks --
    // measured on ARM at -O2, a retain/release pair around this shape is
    // 8 instructions to 12 with the ops out of line, 62 with them inlined,
    // and GCC elides none of it even under whole-program LTO: an atomic RMW
    // may not be removed, and the decrement gates a call to dealloc. Clang
    // gets away with retain-on-every-binding only because ObjCARCOpt knows
    // objc_retain/objc_release are refcount intrinsics. Here the eliding
    // has to happen in oz2c, so an exact answer is the cheap one.
    let kept = returned_name.as_deref().map(|name| owner_of_returned(node, name, ctx));
    let arc_releases = releases_for_all_scopes(ctx, kept.as_deref());
    let mut needs_retain = enclosing_function_body(node).is_some_and(|body| {
        crate::arc::return_needs_retain(
            node,
            body,
            ctx.src,
            ctx.program,
            &ctx.program.owning_methods,
        )
    });
    /* A returned expression whose *evaluation* hoists a `+1` operand is
     * the same shape one statement earlier: the operand's temporary is
     * released before the return, and the value may be that temporary
     * (#355 follow-up). `arc::return_hands_back_ownership` reports the
     * function `+1` on the same predicate, so the caller releases what is
     * retained here.
     *
     * Guarded on the return type being a pointer, this position's own
     * check -- `return [h count:makeThing()];` in an `int` method hoists
     * an operand too, and retaining an `int` is not a thing. */
    if !needs_retain && ctx.method_return_type.contains('*') {
        if let Some(value) = returned_children
            .iter()
            .find(|c| c.kind() != "return" && c.kind() != ";")
        {
            needs_retain = crate::arc::hoists_owning_operand(
                *value,
                ctx.src,
                ctx.program,
                &ctx.program.owning_methods,
            );
        }
    }

    // Outside any @synchronized, behave exactly as the catch-all in
    // `render_expr` would: byte-identical when nothing needs translating.
    if ctx.sync_cleanups.is_empty() && arc_releases.is_empty() && !needs_retain {
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

    let mut cleanup_lines: Vec<String> = arc_releases;
    cleanup_lines.extend(ctx.sync_cleanups.iter().rev().cloned());
    let cleanups = cleanup_lines.join("\n\t");

    let mut cursor = node.walk();
    let value = node.children(&mut cursor).find(|c| c.kind() != "return" && c.kind() != ";");

    match value {
        None if cleanups.is_empty() => ("return;".to_string(), "id".to_string()),
        None => (format!("{}\n\treturn;", cleanups), "id".to_string()),
        Some(value) => {
            let (value_text, _) = render_expr(value, ctx);
            ctx.block_counter += 1;
            let (line, col) = line_col(ctx.src, node.start_byte());
            let tmp = format!("_oz_sync_ret_L{}_C{}_{}", line, col, ctx.block_counter);
            let ret_ty = ctx.method_return_type.clone();
            // The returned value is retained where its provenance cannot
            // be established and another local's `+1` is about to be
            // released against it -- `arc::return_needs_retain` decides,
            // and `arc::return_hands_back_ownership` reports the same
            // function as `+1` from the same call, so the caller releases
            // what is retained here (#351).
            //
            // Cast back to the return type because `oz_static_retain`
            // answers in the root class's pointer type, the same round trip
            // every other retain-bearing expression here makes.
            let value_text = if needs_retain && class_name_from_type(&ret_ty).is_some() {
                match ctx.program.root_class() {
                    Some(root) => format!(
                        "({ty})oz_static_retain((struct {root} *)({value}))",
                        ty = ret_ty,
                        root = root,
                        value = value_text
                    ),
                    None => value_text,
                }
            } else {
                value_text
            };
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

    // A pool block is an ordinary scope as far as ownership goes, so it does
    // the same ARC bookkeeping as `render_scoped_block` -- see `arc_enter`
    // for what went wrong while it did not.
    arc_enter(ctx, node);
    let mut ended_with_jump = false;
    let mut rendered_stmts: Vec<(String, &str)> = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        let rendered = render_expr(*stmt, ctx).0;
        let combined = if ctx.pre_stmts.is_empty() {
            rendered
        } else {
            let pre = ctx.pre_stmts.join("\n\t");
            ctx.pre_stmts.clear();
            format!("{}\n\t{}", pre, rendered)
        };
        rendered_stmts.push((combined, node_text(*stmt, ctx.src)));
        arc_note(*stmt, ctx);
        ended_with_jump = is_jump_statement(*stmt);
    }
    let releases = arc_exit(ctx, ended_with_jump);

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
    for line in &releases {
        out.push('\t');
        out.push_str(line);
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

/// The `-performSelector:` variants, in the order of the arguments they
/// pass. Must agree with `collect::prescan_reflection`'s own list, which
/// is what decides whether wrappers get generated at all.
const PERFORM_SELECTORS: &[&str] = &[
    "performSelector:",
    "performSelector:withObject:",
    "performSelector:withObject:withObject:",
];

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
    // `+class` on a literal class name is a compile-time constant, and
    // `-class` on a value is the `class_id` bitfield every object already
    // carries -- so neither needs a class object, and both are free.
    //
    // This has to preempt the ordinary class-method path below. `+class`
    // is declared once on the root class, so `find_defining_class` routes
    // `[Widget class]` to `OZObject_class_cls()`: the receiver's class is
    // dropped (making `[Widget class]` and `[Gadget class]` the same
    // expression) and no such function is ever generated, so the build
    // failed at link time with an undefined symbol rather than at
    // transpile time with a located message. Nothing in the corpus writes
    // `[X class]`, which is why it went unnoticed (#226).
    if parts.selector == "class" && parts.args.is_empty() {
        if let Some(cls) = recv_type.strip_prefix("class:") {
            return (format!("OZ_STATIC_CLASS_{}", cls), "Class".to_string());
        }
        return (format!("oz_class_of({})", recv_text), "Class".to_string());
    }
    // Exact class equality, so it needs no ancestry walk -- unlike
    // `-isKindOfClass:`, which `render_introspection` gates behind
    // `--introspection` because it generates a table. `oz_class_of`
    // yields `Nil` for a null receiver, which no class ever equals, so a
    // message to nil answers NO here without a separate guard.
    if parts.selector == "isMemberOfClass:" && parts.args.len() == 1 {
        return (
            format!("(oz_class_of({}) == ({}))", recv_text, arg_texts[0]),
            "BOOL".to_string(),
        );
    }
    // The two introspection selectors that need a generated table: an
    // ancestry walk over the superclass chain, and a per-protocol
    // conformance bitmap. Both are gated on `--introspection`
    // (`CONFIG_OBJZ_INTROSPECTION`) -- and when it is off they stay hard
    // located errors naming the option, so a build never quietly loses
    // them. Class *identity* above needs no table and so no gate.
    if parts.selector == "isKindOfClass:" && parts.args.len() == 1 {
        if !ctx.program.introspection {
            ctx.err(
                node,
                "'-isKindOfClass:' needs introspection, which is off -- set CONFIG_OBJZ_INTROSPECTION=y (oz2c --introspection). '-isMemberOfClass:' is always available if exact class equality will do",
            );
            return (node_text(node, ctx.src).to_string(), "BOOL".to_string());
        }
        ctx.introspection_used.kind_of = true;
        return (
            format!("oz_is_kind_of(oz_class_of({}), ({}))", recv_text, arg_texts[0]),
            "BOOL".to_string(),
        );
    }
    // `-respondsToSelector:` and the `-performSelector:` family, behind
    // `--reflection` (`CONFIG_OBJZ_REFLECTION`). The argument is an
    // ordinary `SEL` expression -- a `@selector(...)` literal or anything
    // holding one -- so there is nothing to restrict here: C's own type
    // checking covers a non-SEL argument, which is the payoff for `SEL`
    // being a real type rather than a compile-time-only spelling.
    if parts.selector == "respondsToSelector:" && parts.args.len() == 1 {
        if !ctx.program.reflection {
            ctx.err(
                node,
                "'-respondsToSelector:' needs reflection, which is off -- set CONFIG_OBJZ_REFLECTION=y (oz2c --reflection)",
            );
            return (node_text(node, ctx.src).to_string(), "BOOL".to_string());
        }
        return (
            format!("oz_responds({}, oz_class_of({}))", arg_texts[0], recv_text),
            "BOOL".to_string(),
        );
    }
    if PERFORM_SELECTORS.contains(&parts.selector.as_str())
        && parts.args.len() == parts.selector.matches(':').count()
    {
        if !ctx.program.reflection {
            ctx.err(
                node,
                format!(
                    "'-{}' needs reflection, which is off -- set CONFIG_OBJZ_REFLECTION=y (oz2c --reflection)",
                    parts.selector
                ),
            );
            return (node_text(node, ctx.src).to_string(), "id".to_string());
        }
        // Absent arguments are nil, which is what Objective-C passes when
        // a selector takes more than the `-performSelector:` variant
        // supplies. The wrapper drops the ones its selector does not want.
        let nil = "((void *)0)".to_string();
        let a0 = arg_texts.get(1).cloned().unwrap_or_else(|| nil.clone());
        let a1 = arg_texts.get(2).cloned().unwrap_or(nil);
        return (
            format!(
                "oz_perform({}, (struct {} *)({}), (void *)({}), (void *)({}))",
                arg_texts[0], root, recv_text, a0, a1
            ),
            "id".to_string(),
        );
    }
    if parts.selector == "conformsToProtocol:" && parts.args.len() == 1 {
        if !ctx.program.introspection {
            ctx.err(
                node,
                "'-conformsToProtocol:' needs introspection, which is off -- set CONFIG_OBJZ_INTROSPECTION=y (oz2c --introspection)",
            );
            return (node_text(node, ctx.src).to_string(), "BOOL".to_string());
        }
        // `render_protocol_literal` has already resolved the argument to
        // the bitmap's name and recorded the use; a non-literal argument
        // is refused by the static bar, since a protocol has no value
        // representation to pass.
        return (
            format!("oz_conforms(oz_class_of({}), {})", recv_text, arg_texts[0]),
            "BOOL".to_string(),
        );
    }
    // `+allocWithHeap:` is declared once on the root class, but it has to
    // allocate `sizeof(struct {receiver})` and stamp the receiver's own
    // class_id -- so, exactly like `+alloc`, it resolves to the *receiver's*
    // generated allocator rather than to the declaring class's. Dispatching
    // it as an ordinary class method would call
    // `OZObject_allocWithHeap__cls`, which allocates an OZObject-sized
    // block: `samples/heap_alloc` did precisely that, and it linked to
    // nothing at all because no such function is generated.
    if parts.selector == "allocWithHeap:" && parts.args.len() == 1 {
        if let Some(cls) = recv_type.strip_prefix("class:") {
            let cls = cls.to_string();
            if !ctx.program.heap_support {
                ctx.err(
                    node,
                    format!(
                        "'{}' needs heap support, which is off -- pass --heap-support (and build with -DOZ_HEAP_SUPPORT) to enable '+allocWithHeap:'",
                        one_line(node_text(node, ctx.src))
                    ),
                );
                return (node_text(node, ctx.src).to_string(), format!("struct {} *", cls));
            }
            return (
                format!(
                    "{cls}_oz_alloc_with_heap((struct {root} *)({heap}))",
                    cls = cls,
                    root = root,
                    heap = arg_texts[0]
                ),
                format!("struct {} *", cls),
            );
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
            {
                reject_ambiguous_dispatch(node, ctx, None, &parts.selector);
                dynamic_dispatch_call(ctx.program, &root, &parts.selector, &recv_text, &arg_texts)
            }
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
                {
                reject_ambiguous_dispatch(node, ctx, None, &parts.selector);
                dynamic_dispatch_call(ctx.program, &root, &parts.selector, &recv_text, &arg_texts)
            }
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
                {
                reject_ambiguous_dispatch(node, ctx, None, &parts.selector);
                dynamic_dispatch_call(ctx.program, &root, &parts.selector, &recv_text, &arg_texts)
            }
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
/// through the `_meta.class_id` switch -- used whenever the receiver's
/// static type doesn't pin down which class's implementation to call
/// directly, whether because it's genuinely unresolvable (a bare `id`)
/// or because it's typed as the root/a protocol, standing in for "any
/// conforming object."
/// Refuse a dynamically dispatched send whose reachable implementations
/// disagree about ownership.
///
/// Reached from every site that routes through the `class_id` switch, so
/// no such send can escape the check. The alternative was to leak -- which
/// is what the analysis answers for an ambiguous send, and the safe
/// direction -- but a selector whose ownership depends on which subclass
/// or conformer turns up cannot be called correctly by anyone, so leaving
/// it callable only defers the problem to a leak nobody attributes.
///
/// Only object-returning selectors are checked. Ownership is meaningless
/// for a `void` or scalar result, and refusing those would reject ordinary
/// polymorphism -- `-poke` overridden by three subclasses is exactly what
/// dynamic dispatch is for.
fn reject_ambiguous_dispatch(
    node: Node,
    ctx: &mut EmitCtx,
    receiver_class: Option<&str>,
    selector: &str,
) {
    let ret_ty = ctx
        .program
        .dynamic_dispatch_methods()
        .into_iter()
        .find(|m| m.selector == selector && !m.is_class_method)
        .map(|m| m.return_type);
    let returns_object = ret_ty.as_deref().is_some_and(|ty| {
        class_name_from_type(ty).is_some() || ty.trim() == "id" || ty.trim() == "void *"
    });
    if !returns_object {
        return;
    }
    if let crate::arc::DispatchOwnership::Ambiguous { owning, borrowed } =
        crate::arc::dispatch_ownership(
            ctx.program,
            &ctx.program.owning_methods,
            receiver_class,
            selector,
            false,
        )
    {
        ctx.err(
            node,
            format!(
                "'{selector}' is dispatched at run time here, and its reachable \
                 implementations disagree about ownership: '{owning}' hands back a \
                 reference the caller must release, '{borrowed}' hands back one it \
                 keeps owning. No caller can be correct for both -- a `+1` result \
                 must be released exactly once and a `+0` one never. Make them agree, \
                 or call through a receiver typed as the class you mean"
            ),
        );
    }
}

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

/// Guess a hoisted block's C return type from its body: any
/// `return_statement` carrying a value -> `int`, none -> `void`.
///
/// The **last** of the three sources `render_block` tries, and the only one
/// that guesses. It runs when the author wrote no return type
/// (`^(int x) { ... }`) *and* the declaration around the literal names none
/// either -- see `block_return_type` for the two that come first. Until #303
/// it was the only source, so `^uint32_t(int seed) { ... }` also came out
/// `int`.
///
/// `int` is a guess, not an inference: this spike has no general expression
/// typing (an arithmetic expression elsewhere resolves to the opaque `id`
/// static type -- see `render_expr`'s catch-all), so the returned
/// expression is not consulted at all. It is load-bearing rather than
/// merely tolerated -- `tests/behavior/cases/blocks/non_capturing_basic.m`,
/// `block_with_static_var.m`, `tests/adapted/llvm_rewriter/block_rewrite.m`
/// and `samples/transpiled_blocks` all rely on it -- which is why #303 left
/// it in place instead of rejecting a value-returning block with no
/// declared type.
///
/// A block that returns something else and is reached by neither earlier
/// source still comes out `int`, and GCC reports the mismatch on generated
/// code. The way out is to write the return type, which is now carried.
///
/// Does not descend into a nested `block_literal` (a separate
/// scope/function of its own).
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

/// The `parameter_list` of a `block_literal`, wherever the grammar put it.
///
/// Two places, decided by whether a return type was written:
///
/// ```text
/// ^(int seed) { ... }           block_literal -> parameter_list
/// ^uint32_t(int seed) { ... }   block_literal -> type_name
///                                 -> abstract_function_declarator -> parameter_list
/// ^void *(int seed) { ... }     block_literal -> type_name
///                                 -> abstract_pointer_declarator
///                                   -> abstract_function_declarator -> parameter_list
/// ```
///
/// Only the first was looked for until #303, so an explicit return type
/// lost the whole parameter list and the block was hoisted `(void)` --
/// leaving the body's references to its own parameters undeclared. That is
/// the half of #303 that made it a silent wrong answer rather than a
/// missing feature: nothing in oz2c reported anything, and the error came
/// from GCC, about a signature the author never wrote.
///
/// Searched by descent rather than by a fixed path, so the pointer-return
/// nesting above needs no separate case.
fn block_parameter_list(node: Node) -> Option<Node> {
    fn find(n: Node) -> Option<Node> {
        if n.kind() == "parameter_list" {
            return Some(n);
        }
        let mut cursor = n.walk();
        let children: Vec<Node> = n.children(&mut cursor).collect();
        children.into_iter().find_map(find)
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    // The body is a sibling of both shapes above and can hold a nested
    // literal with a parameter list of its own, which is not this one.
    children.into_iter().filter(|c| c.kind() != "compound_statement").find_map(find)
}

/// A hoisted block's C return type, from the best source available (#303).
///
/// In order, because each is more authoritative than the next:
///
/// 1. **What the author wrote on the block.** `^uint32_t(int seed) { ... }`
///    -- the shape Objective-C already provides for saying what a block
///    returns. Carried through `collect::render_type`, so it agrees with
///    every other type position: `id` -> `void *`, a known class ->
///    `struct Name *`, anything else (a typedef like `uint32_t`) verbatim.
/// 2. **The declaration the literal initializes.**
///    `static unsigned (^sq)(int) = ^(int x) { ... };` -- the type belongs
///    to the variable, not the literal, but it is the same type and it is
///    right there in the enclosing `declaration`.
/// 3. **A guess from the body** -- `infer_block_return_type`, which is
///    where every block landed before this.
///
/// What is deliberately *not* here is the shape #303 was filed for:
/// `.fn = OZFN(^(int seed) { ... })` in a designated initializer, whose
/// type is a field of a C struct. oz_static cannot reach it and no amount
/// of work here changes that -- see the note on `render_block`. Such a
/// block still gets source 3, but it keeps its parameters now, and writing
/// the return type on the literal (source 1) is the fix for it.
fn block_return_type(node: Node, body: Option<Node>, ctx: &EmitCtx) -> String {
    let known: HashSet<String> = ctx.program.classes.keys().cloned().collect();

    let mut cursor = node.walk();
    let type_name = node.children(&mut cursor).find(|c| c.kind() == "type_name");
    if let Some(type_name) = type_name {
        // Pruned, or the parameters' own stars are counted as the return
        // type's -- `^void *(char *s)` would come out `void **`.
        let (text, stars) =
            crate::collect::extract_type_and_stars_to_declarator(type_name, ctx.src);
        if !text.is_empty() {
            return crate::collect::render_type(&text, stars, &known).trim().to_string();
        }
    }

    if let Some((text, stars)) = declared_block_pointer_type(node, ctx.src) {
        return crate::collect::render_type(&text, stars, &known).trim().to_string();
    }

    body.map(infer_block_return_type).unwrap_or("void").to_string()
}

/// The return type of the block-pointer variable this literal initializes,
/// if that is where it sits: `static unsigned (^sq)(int) = ^(int x) {...}`
/// -> `("unsigned", 0)`.
///
/// ```text
/// declaration
///   storage_class_specifier    "static"
///   sized_type_specifier       "unsigned"   <- the return type
///   init_declarator
///     function_declarator
///       parenthesized_declarator
///         block_pointer_declarator           <- what makes it a block
///       parameter_list
///     block_literal                          <- `node`
/// ```
///
/// The type specifier is a sibling of the `init_declarator`, so this walks
/// *up* from the literal to the `declaration` and then reads its declared
/// type. Guarded on a `block_pointer_declarator` actually being present:
/// without that check, any literal inside any initializer would take the
/// enclosing declaration's type, which for `.fn = OZFN(^...)` inside
/// `static struct holder h = { ... }` would confidently return
/// `struct holder` -- worse than guessing.
///
/// The type text comes from the `declaration` (with the `init_declarator`
/// pruned, so the parameter list cannot contribute). The stars do *not*:
/// in `static void *(^f)(int)` the `*` sits inside the `init_declarator`,
/// in a `pointer_declarator` wrapping the `function_declarator`, so the
/// declarator chain is walked separately for them by
/// `declarator_return_stars` -- stopping at the `function_declarator`,
/// past which any star belongs to a parameter.
fn declared_block_pointer_type(node: Node, src: &str) -> Option<(String, usize)> {
    let init_declarator = node.parent().filter(|p| p.kind() == "init_declarator")?;
    let declaration = init_declarator.parent().filter(|p| p.kind() == "declaration")?;

    fn has_block_pointer(n: Node) -> bool {
        if n.kind() == "block_pointer_declarator" {
            return true;
        }
        if n.kind() == "block_literal" {
            return false;
        }
        let mut cursor = n.walk();
        let children: Vec<Node> = n.children(&mut cursor).collect();
        children.into_iter().any(has_block_pointer)
    }
    if !has_block_pointer(init_declarator) {
        return None;
    }

    let (type_text, _) = crate::collect::extract_type_and_stars_to_declarator(declaration, src);
    if type_text.is_empty() {
        return None;
    }

    Some((type_text, crate::collect::declarator_return_stars(init_declarator)))
}

/// A cast expression, which needs handling for two separate reasons:
///
///   - an ARC bridging qualifier (`__bridge`, `__bridge_transfer`,
///     `__bridge_retained`) has to be dropped. It means nothing without
///     ARC and is not a C keyword, so left in place it is a compile error
///     (`use of undeclared identifier '__bridge'`). The real
///     `src/OZTimer.m` casts this way, so without this OZTimer cannot be
///     transpiled at all. Rebuilding the type through
///     `collect::extract_type_and_stars` drops it for free -- that
///     function collects only type specifiers, never a `type_qualifier`.
///     The oracle drops it too: its committed
///     `tests/zephyr/generated/OZTimer_ozm.c:28` renders that same cast
///     as plain `(void *)expBlock`.
///   - the cast's target type has to be *reported*, so a send against a
///     cast receiver resolves. `[((OZQ31 *)obj) int32Value]` otherwise
///     fails with "cannot statically resolve the receiver type ...
///     (receiver type is 'id')", since every expression not specifically
///     handled reports the opaque `id`. The oracle gets this for free from
///     Clang's own types; here the declared type is right there in the
///     cast, which is the one place a bare `id` can be narrowed back to a
///     class without inference.
///
/// A known class name in the cast is rendered `struct Name *`, matching
/// how the same name is rendered in every other type position
/// (`collect::render_type`).
fn render_cast_expression(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let descriptor = children.iter().find(|c| c.kind() == "type_descriptor").copied();
    let value = children
        .iter()
        .rev()
        .find(|c| c.kind() != ")" && c.kind() != "(" && c.kind() != "type_descriptor")
        .copied();

    let (Some(descriptor), Some(value)) = (descriptor, value) else {
        // Not the shape this handles (e.g. a compound literal); fall back
        // to the generic rebuild the catch-all would have done.
        let rebuilt = rebuild(node, ctx, &mut |child, ctx| {
            if needs_translation(child) {
                Some(render_expr(child, ctx).0)
            } else {
                None
            }
        });
        return (rebuilt, "id".to_string());
    };

    let (type_text, stars) = crate::collect::extract_type_and_stars(descriptor, ctx.src);
    let known: HashSet<String> = ctx.program.classes.keys().cloned().collect();
    let c_type = crate::collect::render_type(&type_text, stars, &known);

    let (value_text, _) = render_expr(value, ctx);
    (format!("({})({})", c_type.trim(), value_text), c_type)
}

/// Non-capturing block literal -> hoisted static C function; the block
/// expression itself is replaced with a reference to that function.
/// (Capturing blocks were already rejected by the static-bar scan.)
///
/// The signature is assembled from `block_parameter_list` and
/// `block_return_type` rather than read off one fixed child, because the
/// grammar moves the parameter list when a return type is written and the
/// return type has three possible sources -- see both for the detail.
///
/// **Not** among those sources: the C struct field a designated
/// initializer assigns the block to, which is the shape #303 was filed
/// for (`.fn = OZFN(^(int seed) { ... })`, against Zephyr's
/// `bt_conn_auth_cb.app_passkey`). It is out of reach twice over, and
/// neither is an implementation gap. The field's type lives in a
/// `#include`d pure-C header, which `imports` deliberately leaves verbatim
/// rather than splicing -- so the struct never enters the CST at all. And
/// the Clang AST cannot answer either, because `OZFN` expands to `0` on the
/// Objective-C side (a static initializer needs a null pointer constant),
/// so there is no block at that position for Clang to type. Writing the
/// return type on the literal is the fix for that shape, which is why
/// carrying it matters.
fn render_block(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    /* The position in the file the author wrote, when there is a source
     * map to ask (#305). The merged-buffer position is what this used to
     * be, and it named nowhere: `px-keyboard/src/PXLEDController.m` is 210
     * lines long and shipped an `oz_block_L3271_C38_1`, whose 3271 is a
     * line of the 3,829-line spliced buffer. The *column* was right all
     * along -- splicing moves lines, never a byte within one -- which is
     * why only the line moves here.
     *
     * Falling back to the merged position rather than dropping the
     * numbers: with directives off there is nothing better to say, and a
     * symbol has to be named something stable. */
    let (line, col) = ctx
        .lines
        .position(node.start_byte())
        .unwrap_or_else(|| line_col(ctx.src, node.start_byte()));
    ctx.block_counter += 1;
    let name = format!("oz_block_L{}_C{}_{}", line, col, ctx.block_counter);

    let found_plist = block_parameter_list(node);
    let params = match found_plist {
        // An `id` parameter is spelled as the root class pointer, matching
        // the function-pointer *type* this block will be assigned or passed
        // to. This is one of the positions that have to agree on that
        // spelling -- `render_param` carries the roster and the reasoning --
        // and it is the one every other position is measured against, since
        // this signature is what the hoisted function actually has:
        // `samples/transpiled_generics` passes
        // `^(id obj, unsigned int idx, BOOL *stop) { ... }` to
        // `-enumerateObjectsUsingBlock:`, and with only the parameter type
        // lowered the call stopped compiling on the function pointer's type.
        //
        // Driven off the CST rather than the parameter list's text, so only
        // an `id` the grammar reads as a *type* is rewritten. A flat-text
        // sweep could not tell that from a parameter merely *named* `id`,
        // and rewrote both -- #317. That name is reserved now
        // (`staticbar::check_reserved_names`), so this cannot be reached with
        // one; keying on the CST means it emits correct C rather than two
        // stacked type specifiers if it ever is.
        //
        // A *class*-typed parameter is promoted to `struct Name *` by the
        // same `class_tag_edits` every other patched-text signature uses.
        // This list is copied out of the author's source, and until #326 it
        // was copied with the bare Objective-C spelling intact, so
        // `void (^b)(Widget *) = ^(Widget *w) { ... };` hoisted
        // `void oz_block_...(Widget *w)` -- `error: must use 'struct' tag to
        // refer to type 'Widget'`, no valid C at all. The declarator side
        // (`render_block_type_param_list`) had promoted it all along, so the
        // two sides also disagreed on the type. Independent of the `id`
        // lowering above and applied unconditionally: it needs no root class,
        // only the class table.
        Some(plist) => {
            let mut edits = class_tag_edits(plist, ctx.src, ctx.program);
            if let Some(root) = ctx.program.root_class() {
                rewrite_id_types(plist, ctx.src, 0, &format!("struct {} *", root), &mut edits);
            }
            apply_edits(ctx.src, plist.start_byte(), plist.end_byte(), &edits)
        }
        None => "(void)".to_string(),
    };

    let mut cursor2 = node.walk();
    let body = node.children(&mut cursor2).find(|c| c.kind() == "compound_statement");
    let ret_ty = block_return_type(node, body, ctx);
    // The block's own return type is what a `return` inside it has to be
    // evaluated into on the cleanup path, so record it before the body is
    // rendered and put the enclosing body's back afterwards (#339).
    //
    // The two other positions that record this -- `render_method_definition`
    // and `walk_top_level`'s `function_definition` arm -- each own a fresh
    // `EmitCtx` and so can simply assign. A block literal is rendered
    // *inside* an enclosing body's context, on that body's `EmitCtx`, so
    // without the restore the next `return` in the enclosing body, after
    // the literal, would take the block's type.
    //
    // Setting it for a `void` block is correct too: a valueless `return`
    // takes `render_return_statement`'s `None` arm and builds no temporary
    // at all.
    let enclosing_return_type = std::mem::replace(&mut ctx.method_return_type, ret_ty.clone());
    /* The enclosing body's pending `@synchronized` unlocks are *its* to
     * run, for the same reason its ARC scopes are (#342): a `return`
     * inside this literal is rendered while they are still stacked, and
     * `render_return_statement` would put them in the hoisted function --
     * whose text names the `struct OZSpinLock *` temporary the enclosing
     * body declared, and which does not hold the lock in the first place.
     * The block runs at its call site, which may be inside or outside the
     * critical section; either way, unlocking on its way out is wrong.
     *
     * Cleared rather than boundary-marked like `ArcScope::is_block_body`,
     * because `ctx.sync_cleanups` is a flat list of *text* with no scope
     * structure to mark. Restored after, on the same reasoning as
     * `method_return_type` above: this is the enclosing body's `EmitCtx`,
     * so a `@synchronized` still open after the literal must keep owing
     * its unlock. */
    let enclosing_sync_cleanups = std::mem::take(&mut ctx.sync_cleanups);
    let body_text = match body {
        Some(body) => {
            // Block bodies use the same flat scope as their enclosing
            // method/function (a known spike simplification). That is
            // about *names*: a block body sees the enclosing body's
            // locals, which is what makes an undeclared-identifier error
            // impossible to get from the scope map alone. It is no longer
            // true of anything the enclosing body has left *pending* --
            // its return type (#339), its ARC scopes and its
            // `@synchronized` unlocks (#342, both above) each have a
            // boundary here, because the hoisted function is a different
            // function and the enclosing body's locals are not its own.
            collect_local_decls(body, ctx);
            render_body_with_comments(body, ctx)
        }
        None => "{\n}".to_string(),
    };
    ctx.method_return_type = enclosing_return_type;
    ctx.sync_cleanups = enclosing_sync_cleanups;

    // `(void)param;` for the block's own unused parameters. This function and
    // its signature are both synthesized here, so unlike a plain C function's
    // body there is no author's text being edited.
    let body_text = match found_plist {
        Some(plist) => {
            let names = parameter_list_names(plist, ctx.src);
            let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            let acks = acks_for_names(&body_text, &refs);
            let resume = match body {
                Some(body) => ctx.lines.resume_after_brace(
                    ctx.src,
                    body.start_byte(),
                    body.end_byte(),
                ),
                None => String::new(),
            };
            splice_after_open_brace(&body_text, &acks, &resume)
        }
        None => body_text,
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
    // The banner says where the literal was, not what enclosed it: since
    // #272 a literal at *file scope* is hoisted too (a block variable's
    // initializer, `static void (^g)(int) = ^(int v){ ... };`), and there is
    // no enclosing method to name. It read "hoisted out of its enclosing
    // method" until then, which was true of every caller at the time and
    // became false for the new one -- the shape of stale claim docs/STATUS.md
    // keeps recording, in generated output this time. Since #305 the
    // position it states is the `.m`'s, like the name's.
    //
    // The directive goes below the banner and above the signature, so the
    // hoisted function's own line is the line of the block literal it came
    // from -- which is what a backtrace naming this symbol should resolve
    // to. Its body needs no anchor of its own (`body_anchor`): the
    // synthesized signature is one line and starts at the literal's own
    // line, so a verbatim body's lines line up from here by themselves,
    // and a rendered one carries a directive per statement.
    let definition = format!(
        "/* block at {}:{} -- synthesized function, hoisted from a block literal */\n{}{} {}{} {}\n",
        line,
        col,
        ctx.lines.before(node.start_byte()),
        ret_ty,
        name,
        params,
        body_text
    );
    ctx.hoisted_blocks.push((prototype, definition));
    (name.clone(), "id".to_string())
}

/// Render a `compound_statement` body. If nothing inside needed
/// translation, returned byte-identical to the original -- with a leading
/// `#line` directive, which is the only thing that ever precedes it.
/// Otherwise the whole body is reformatted one-statement-per-line,
/// tab-indented: a translated statement gets its original (collapsed to one
/// line) as a `/* ... */` comment above it and a `#line` directive between
/// the two, naming where the statement was written (#305); an untouched
/// statement gets the directive and then itself. This trades exact preservation of the
/// original body's own formatting (blank lines, inline comments between
/// statements) for consistent, predictable output once a body is already
/// being annotated -- a deliberate simplification, not an oversight.
/// Nested statements (inside an if/for/etc) are still translated by the
/// ordinary recursive mechanism -- they are not re-commented at every
/// nesting level; the comment on the enclosing top-level statement is
/// what points back to source.
/// The object locals a `declaration` statement introduces that this block
/// will own, if any.
///
/// A local is owned only when its initializer is provably +1 (see
/// `arc::binds_ownership`); a borrowed reference is left alone, because
/// releasing one is a double free. `__unsafe_unretained` opts out
/// explicitly, matching what the qualifier means everywhere else.
///
/// An intervening cast does not change the answer -- it changes the static
/// type, not who owns the reference -- so `Thing *t = (Thing *)[Thing
/// alloc];` is released here just as the uncast spelling is. That is
/// `binds_ownership` rather than `is_owning_expr`, and the difference is
/// load-bearing: `(Thing *)[u init]` hands back `u`'s own +1, and
/// releasing it here as well frees one pointer twice (#332).
fn owned_locals_of(decl: Node, ctx: &EmitCtx) -> Vec<String> {
    owned_locals_of_in(decl, decl.parent(), ctx)
}

/// Is `name` released by hand somewhere under `root`?
///
/// oz_static supports manual retain/release as a feature of its own (see
/// `behavior_memory`), and a variable cannot be managed both ways: adding an
/// automatic release to code that already releases is a double free. So ARC
/// defers to the author wherever the author took control.
///
/// The oracle never has to make this choice -- its sources are compiled with
/// `-fobjc-arc`, under which an explicit `release` is a compile error, and
/// indeed no `.m` under tests/behavior/cases/ contains one. oz_static
/// accepts both styles, so it has to decide, and deferring is the only
/// option that cannot corrupt memory.
///
/// The search covers `root`'s whole subtree, so a release in a nested
/// `if`/loop counts. A release in a *sibling* scope after the declaring
/// block has ended would be missed, but such code cannot be reached anyway
/// -- the variable is out of scope there.
fn released_by_hand(name: &str, root: Node, src: &str) -> bool {
    if root.kind() == "message_expression" {
        let mut cursor = root.walk();
        let parts: Vec<Node> = root
            .children(&mut cursor)
            .filter(|c| c.kind() != "[" && c.kind() != "]")
            .collect();
        if parts.len() == 2
            && &src[parts[1].byte_range()] == "release"
            && &src[parts[0].byte_range()] == name
        {
            return true;
        }
    }
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    children.into_iter().any(|child| released_by_hand(name, child, src))
}

fn owned_locals_of_in(decl: Node, search_root: Option<Node>, ctx: &EmitCtx) -> Vec<String> {
    if node_text(decl, ctx.src).contains("__unsafe_unretained") {
        return Vec::new();
    }
    /* A `static` object local is a strong slot that outlives the scope, so
     * the scope owes it nothing -- the store into it is what manages it
     * (`arc_managed_slots`). Releasing it here destroyed the object on the
     * way out of the first call and released the freed block again on the
     * next one (#359). Checked before the managed-set branch below, which
     * would otherwise claim it from its initializer's shape. */
    if is_static_declaration(decl, ctx.src) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cursor = decl.walk();
    let children: Vec<Node> = decl.children(&mut cursor).collect();
    for child in children {
        if child.kind() != "pointer_declarator" && child.kind() != "init_declarator" {
            continue;
        }
        // Anything ARC manages as a strong local owns whatever it ends up
        // holding (see `managed_object_locals`), so its final value needs the
        // same scope-exit release an owning initializer gets. Without this the
        // last object assigned in a loop would never be freed --
        // release-on-overwrite only covers the ones that were overwritten.
        //
        // This covers all three declaration forms the managed set admits: a
        // bare `Counter *c;`, an explicit `Counter *c = nil;`, and an owning
        // `Counter *c = [Counter alloc];`. Testing the set first rather than
        // the initializer's shape is what makes the `nil` form work -- its
        // initializer is not owning, so the check below would skip it.
        {
            let name = crate::collect::find_declared_name(child, ctx.src);
            if !name.is_empty() && ctx.arc_managed_locals.contains(&name) {
                out.push(name);
                continue;
            }
        }
        if child.kind() != "init_declarator" {
            continue;
        }
        let mut c2 = child.walk();
        let parts: Vec<Node> = child.children(&mut c2).collect();
        // `init_declarator` is `<declarator> = <value>`; the value is
        // whatever follows the `=`.
        let eq = parts.iter().position(|n| n.kind() == "=");
        let Some(value) = eq.and_then(|i| parts.get(i + 1)).copied() else {
            continue;
        };
        if !crate::arc::binds_ownership(value, ctx.src, ctx.program, &ctx.program.owning_methods) {
            continue;
        }
        /* Only a pointer slot can hold the reference, and without this the
         * scope released through an **integer** (#380). `binds_ownership`
         * looks through a non-bridging cast (#332), deliberately, so
         * `int n = (int)makeThing();` reached here and emitted
         * `oz_static_release((struct OZObject *)(n))` -- the cast in the
         * release is what let it compile, and then the decrement landed
         * at whatever address the integer held. On a 64-bit host the
         * pointer is truncated to 32 bits first, so it is a wrong store;
         * on the 32-bit targets that ship the integer happens to hold the
         * whole pointer, which is why no board gate saw it.
         *
         * The third position to need this check locally rather than by
         * widening `binds_ownership` -- `arc::hoists_owning_operand`'s
         * callers (#375) and the `for`-header arm (#376) were the first
         * two. Widening that predicate is still wrong for the reason it
         * always was: it is asked about expressions whose value may not
         * be an object at all. */
        if !crate::arc::declares_pointer(child) {
            continue;
        }
        let name = crate::collect::find_declared_name(child, ctx.src);
        if name.is_empty() {
            continue;
        }
        if search_root.is_some_and(|root| released_by_hand(&name, root, ctx.src)) {
            continue;
        }
        out.push(name);
    }
    out
}

/// The names this declaration binds that must be **retained** because
/// their initialiser's value may be one of the `+1` operand temporaries
/// the same statement is about to release.
///
/// This is the use-after-free half of the operand machinery, and it
/// segfaulted rather than leaked. `Thing *z = [h take:makeThing(1)];`
/// hoists `makeThing(1)` into a temporary and releases it at the end of
/// the statement -- but `-take:` hands its argument straight back, so `z`
/// *is* that temporary, and the release frees the object `z` names. The
/// next `[z n]` reads freed memory; measured as signal 11 on the host, in
/// both the argument spelling above and the receiver one
/// (`Thing *z = [makeThing(1) itself];`). `Thing *s = [[Builder new]
/// result];` is the same shape in ordinary code.
///
/// A retain is what ARC itself emits when a value lands in a `__strong`
/// slot, and it is the answer here for a reason worth recording: it makes
/// `z` an *owned local*, so every question about `z` afterwards is
/// answered by machinery that already exists and is already tested --
/// scope-exit release, release-on-reassignment, `return` unwinding
/// (#342), and the return-escape retain (#351). Deferring the
/// temporary's own release to scope exit was the other candidate and does
/// not close the escape: `return z;` would still hand back a pointer the
/// scope frees on its way out, because `arc::owned_local_live_at` reads
/// the *source* and cannot see a synthesized name.
///
/// Restricted to slots that could actually alias:
///
///   - an **object pointer** only. `int n = [h sum:makeThing(1)];` cannot
///     name the temporary, so it keeps the tight statement-end release
///     and pays nothing. This matters beyond tidiness: a slab holds one
///     slot per *allocation site* (see `pools`), so holding a value
///     longer than the statement can exhaust a pool that a
///     statement-scoped release would have recycled.
///   - an initialiser that is **not already owning**. One that is has
///     taken over a `+1` of its own and is an owned local already;
///     retaining it would be the leak this function exists to avoid.
///   - not `static`, not `__unsafe_unretained`, and not released by hand
///     -- the three exclusions `owned_locals_of_in` makes, for its
///     reasons (#359, and ARC deferring to an author who took control).
fn retained_bindings(decl: Node, ctx: &EmitCtx) -> Vec<String> {
    if node_text(decl, ctx.src).contains("__unsafe_unretained") {
        return Vec::new();
    }
    if is_static_declaration(decl, ctx.src) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cursor = decl.walk();
    let children: Vec<Node> = decl.children(&mut cursor).collect();
    for child in children {
        if child.kind() != "init_declarator" {
            continue;
        }
        let mut c2 = child.walk();
        let parts: Vec<Node> = child.children(&mut c2).collect();
        let eq = parts.iter().position(|n| n.kind() == "=");
        let Some(value) = eq.and_then(|i| parts.get(i + 1)).copied() else {
            continue;
        };
        if crate::arc::binds_ownership(value, ctx.src, ctx.program, &ctx.program.owning_methods) {
            continue;
        }
        /* The shared predicate, so the emitter's retain and the analysis's
         * `+1` classification cannot drift apart -- see
         * `arc::hoists_owning_operand`, which records what that drift
         * cost. */
        if !crate::arc::hoists_owning_operand(
            value,
            ctx.src,
            ctx.program,
            &ctx.program.owning_methods,
        ) {
            continue;
        }
        let name = crate::collect::find_declared_name(child, ctx.src);
        if name.is_empty() {
            continue;
        }
        /* This position's own type check: only a class pointer can name
         * the temporary. `int m = [h sum:makeThing()];` keeps the tight
         * statement-end release and pays nothing -- which matters beyond
         * tidiness, since a slab holds one slot per *allocation site*
         * (see `pools`) and holding a value past its statement can
         * exhaust a pool a statement-scoped release would have recycled. */
        let is_object = ctx.scope.get(&name).is_some_and(|ty| class_name_from_type(ty).is_some());
        if !is_object || !crate::arc::declares_pointer(child) {
            continue;
        }
        if released_by_hand(&name, decl.parent().unwrap_or(decl), ctx.src) {
            continue;
        }
        out.push(name);
    }
    out
}

/// `oz_static_release` for each name, innermost scope first.
fn release_lines(names: &[String], ctx: &EmitCtx) -> Vec<String> {
    let root = ctx.program.root_class().unwrap_or("OZObject").to_string();
    names
        .iter()
        .map(|name| format!("oz_static_release((struct {} *)({}));", root, name))
        .collect()
}

/// Which local a `return`'s name actually owns: itself, if it is one of
/// the scopes' owned locals, else the nearest local it aliases that is.
///
/// Falls back to the name as written when the chain reaches nothing owned,
/// which keeps the byte-identical path for every return that was already
/// correct -- a returned name that owns nothing releases nothing on its
/// own account either way.
///
/// The enclosing body is needed to read declarations from, and a `return`
/// knows it only by walking up: the nearest `compound_statement` with no
/// `compound_statement` above it inside this function. `alias_chain` reads
/// declarations anywhere within, so handing it the outermost body is what
/// lets `Thing *b = a;` in the body be found from a `return b;` nested in
/// an `if`.
fn owner_of_returned(ret: Node, name: &str, ctx: &EmitCtx) -> String {
    let owned_here = |candidate: &str| {
        ctx.arc_scopes.iter().any(|scope| scope.owned.iter().any(|owned| owned == candidate))
    };
    if owned_here(name) {
        return name.to_string();
    }
    let Some(body) = enclosing_function_body(ret) else {
        return name.to_string();
    };
    crate::arc::alias_chain(body, ctx.src, name)
        .into_iter()
        .find(|aliased| owned_here(aliased))
        .unwrap_or_else(|| name.to_string())
}

/// The outermost `compound_statement` enclosing `node` within its function
/// or block literal -- the body every declaration in scope was written in.
///
/// Stops at a `block_literal`, for the reason `ArcScope::is_block_body`
/// carries: a block's body is a separate function, and the enclosing
/// body's declarations are not its locals (#342).
fn enclosing_function_body(node: Node) -> Option<Node> {
    let mut outermost = None;
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "block_literal" {
            break;
        }
        if parent.kind() == "compound_statement" {
            outermost = Some(parent);
        }
        current = parent.parent();
    }
    outermost
}

/// Releases owed by the scopes a `return` leaves, innermost first,
/// skipping `keep` -- the local being returned, whose ownership passes to
/// the caller.
///
/// "The scopes it leaves" is every live one, out to and *including* the
/// nearest block literal's body. It stops there because a block literal
/// is rendered on its enclosing body's `EmitCtx`, so the enclosing body's
/// scopes are still on the stack while the block's `return` is rendered
/// -- and releasing them here emitted the enclosing method's locals
/// inside the hoisted function, naming locals it does not have (#342).
/// See `ArcScope::is_block_body`.
///
/// `releases_up_to_jump_target` below draws the analogous boundary for
/// `break`/`continue`. The two are deliberately separate walks rather
/// than one parameterised by a predicate: a `return` crosses a loop
/// boundary (it leaves the loop *and* the function) while a `break` does
/// not, so they stop at different marks and only look alike.
fn releases_for_all_scopes(ctx: &EmitCtx, keep: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    for scope in ctx.arc_scopes.iter().rev() {
        for name in scope.owned.iter().rev() {
            if Some(name.as_str()) != keep {
                names.push(name.clone());
            }
        }
        if scope.is_block_body {
            break;
        }
    }
    release_lines(&names, ctx)
}

/// Releases owed by the scopes a `break`/`continue` leaves: from the
/// innermost out to and including the nearest loop body. Scopes outside the
/// loop survive it, so their locals must not be touched.
fn releases_up_to_jump_target(node: Node, ctx: &EmitCtx) -> Vec<String> {
    /* What this jump actually leaves. `break` leaves the nearest
     * enclosing loop *or switch*, whichever comes first; `continue`
     * leaves only a loop, and crosses any switch on the way -- which is
     * the whole distinction an `is_loop_body` flag could not express. */
    let breaking = node.kind() == "break_statement";
    let mut target = node.parent();
    while let Some(candidate) = target {
        let leaves = match candidate.kind() {
            "for_statement" | "while_statement" | "do_statement" => true,
            "switch_statement" => breaking,
            _ => false,
        };
        if leaves {
            break;
        }
        target = candidate.parent();
    }
    /* No enclosing loop or switch at all: not valid C, and nothing to
     * release on the way out of a construct that is not there. */
    let Some(target) = target else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for scope in ctx.arc_scopes.iter().rev() {
        /* Strictly past the construct's own start, so the scope wrapping
         * it is left alone while every scope opened inside it is
         * released. */
        if scope.start_byte <= target.start_byte() {
            break;
        }
        for name in scope.owned.iter().rev() {
            names.push(name.clone());
        }
    }
    release_lines(&names, ctx)
}

/// Does this block declare an object local it would own? Only such blocks
/// need rewriting; every other one is left byte-identical, so ARC costs no
/// churn in the generated output.
fn declares_owned_local(body: Node, ctx: &EmitCtx) -> bool {
    let mut cursor = body.walk();
    let children: Vec<Node> = body.children(&mut cursor).collect();
    children
        .into_iter()
        .any(|child| child.kind() == "declaration" && !owned_locals_of(child, ctx).is_empty())
}

/// Is this compound statement a block literal's body?
///
/// Read off the CST, so no push site has to be told what it is
/// rendering: `render_body_with_comments` is reached
/// for a method body, a free function's body and a block literal's body
/// alike, and only the tree distinguishes them.
fn is_block_body(body: Node) -> bool {
    body.parent().is_some_and(|parent| parent.kind() == "block_literal")
}

/// `break`/`continue`, preceded by the releases owed by every scope the
/// jump leaves.
///
/// Without this a loop-local allocated each iteration is leaked on the way
/// out, which is exactly what
/// `tests/behavior/cases/arc/break_releases_loop_local.m` detects: it breaks
/// out of a loop holding the only block of a one-block slab, then allocates
/// again and checks the allocation succeeded.
fn render_loop_jump(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let releases = releases_up_to_jump_target(node, ctx);
    let keyword = node_text(node, ctx.src);
    if releases.is_empty() {
        return (keyword.to_string(), "void".to_string());
    }
    (format!("{}\n\t{}", releases.join("\n\t"), keyword), "void".to_string())
}

/// The expression of an `expression_statement` whose +1 result is bound to
/// nothing, or None when the statement keeps nothing to release.
///
/// A statement is the one place a reference can be created and abandoned
/// in the same breath. `arc.rs` knows `-copy`, `-new`, `+alloc`,
/// `+allocWithHeap:` and every analysed factory return +1; every path that
/// *binds* such a result already releases it -- a local at its scope's end
/// (`release_lines`), a strong local or ivar on the next store
/// (`render_strong_local_assign`), a `return` on its way out
/// (`render_return_statement`) -- and a result bound to nothing had no
/// release at all, so `[t copy];` on its own line leaked a Thing every
/// time it ran (#322).
///
/// Not a `-performSelector:` concern, though that is where the Clang
/// warning pointed: a direct send leaked identically, which is why the
/// answer lives at the statement and not in the reflection path.
///
/// What comes back is the abandoned *value*, not the statement's
/// expression: `(void)[t copy];` discards the same reference the bare
/// `[t copy];` does, and it is the send that gets released, not the
/// `(void)` announcing the discard (#327). `arc::discarded_owning_value`
/// owns both halves of that.
fn discarded_owning_expr<'a>(stmt: Node<'a>, ctx: &EmitCtx) -> Option<Node<'a>> {
    let mut cursor = stmt.walk();
    let value = stmt.children(&mut cursor).find(|c| c.kind() != ";")?;
    crate::arc::discarded_owning_value(value, ctx.src, ctx.program, &ctx.program.owning_methods)
}

fn discards_owning_result(stmt: Node, ctx: &EmitCtx) -> bool {
    discarded_owning_expr(stmt, ctx).is_some()
}

/// Release the abandoned +1 at the end of the full expression, which is
/// what ARC's `objc_release` on an unused result does.
///
/// The release wraps the value rather than going through a temporary: the
/// send is evaluated exactly once, in the same statement, and there is no
/// local for a later jump to have to unwind past. `oz_static_release` is
/// null-safe, so an allocation that found no free slab slot needs no guard
/// of its own.
fn render_discarded_owning_statement(node: Node, ctx: &mut EmitCtx) -> (String, String) {
    let Some(value) = discarded_owning_expr(node, ctx) else {
        return (node_text(node, ctx.src).to_string(), "void".to_string());
    };
    let root = ctx.program.root_class().unwrap_or("OZObject").to_string();
    let (rendered, _) = render_expr(value, ctx);
    (
        format!("oz_static_release((struct {} *)({}));", root, rendered),
        "void".to_string(),
    )
}

/// Which position a +1 operand was written in.
///
/// The temporary's name says so -- `_oz_arg_...` or `_oz_recv_...` --
/// because a reader of the generated C has to be able to tell which
/// reference is being released, and because the two are decided by
/// different predicates: `arc::owning_argument_value` asks only whether
/// the reference is new, while `arc::receiver_owning_value` also has to
/// consult the selector (#340).
#[derive(Clone, Copy, PartialEq, Eq)]
enum OperandPosition {
    Argument,
    Receiver,
}

impl OperandPosition {
    fn prefix(self) -> &'static str {
        match self {
            OperandPosition::Argument => "_oz_arg",
            OperandPosition::Receiver => "_oz_recv",
        }
    }
}
/// The `+1` operands of this send or call that **nothing will hoist**, so
/// the expression itself has to account for them.
///
/// Empty for every shape a statement arm already took: `arg_temps` holds
/// what `render_owning_operand_statement` evaluated, and
/// `collect_owning_operands_in` declines exactly what this picks up, so an
/// operand is claimed by one mechanism or the other and never both.
fn unhoisted_owning_operands<'a>(node: Node<'a>, ctx: &EmitCtx) -> Vec<Node<'a>> {
    let mut out: Vec<Node<'a>> = Vec::new();
    if node.kind() == "message_expression" {
        if let Some(value) = crate::arc::receiver_owning_value(
            node,
            ctx.src,
            ctx.program,
            &ctx.program.owning_methods,
        ) {
            out.push(value);
        }
        for arg in parse_message(node, ctx.src).args {
            if let Some(value) = crate::arc::owning_argument_value(
                arg,
                ctx.src,
                ctx.program,
                &ctx.program.owning_methods,
            ) {
                out.push(value);
            }
        }
    }
    if node.kind() == "call_expression" {
        if let Some(args) = node.child_by_field_name("arguments") {
            let mut cursor = args.walk();
            let arg_nodes: Vec<Node<'a>> = args
                .children(&mut cursor)
                .filter(|c| !matches!(c.kind(), "(" | ")" | ","))
                .collect();
            for arg in arg_nodes {
                if let Some(value) = crate::arc::owning_argument_value(
                    arg,
                    ctx.src,
                    ctx.program,
                    &ctx.program.owning_methods,
                ) {
                    out.push(value);
                }
            }
        }
    }
    out.retain(|value| !ctx.arg_temps.contains_key(&value.id()));
    out
}

/// Render a send or call whose `+1` operands cannot be hoisted, holding
/// and releasing each one **inside the expression**:
///
/// ```c
/// (_oz_ce_1 = makeThing(), _oz_cv_2 = Thing_n(_oz_ce_1),
///  oz_static_release((struct OZObject *)(_oz_ce_1)), _oz_cv_2)
/// ```
///
/// A comma expression is the only shape that puts the release where the
/// source puts the evaluation, which is what every position in #376
/// needs: hoisting a temporary beside the statement allocates once where
/// the source allocates per iteration, or on a branch the source never
/// takes.
///
/// The temporaries are **declared** through `ctx.pre_stmts` and assigned
/// here. That split is the whole trick, and it is why this needs no new
/// hoisting machinery: a declaration without an initialiser evaluates
/// nothing, so lifting it above the statement -- even above a loop --
/// costs nothing and reorders nothing. Only the assignment and the
/// release have to stay inside the expression, and a comma expression
/// keeps them there.
///
/// A `void`-valued send yields no value temporary, and the comma
/// expression's own value is then the release, which is `void`. Legal C,
/// and correct: nothing consumes it.
///
/// Operands are released in reverse, so a nested one outlives the operand
/// built from it -- the order `render_owning_operand_statement` uses.
fn render_comma_operand_expr(
    node: Node,
    ctx: &mut EmitCtx,
    values: Vec<Node>,
) -> (String, String) {
    let root = ctx.program.root_class().unwrap_or("OZObject").to_string();
    let mut assigns: Vec<String> = Vec::with_capacity(values.len());
    let mut releases: Vec<String> = Vec::with_capacity(values.len());
    let mut held: Vec<usize> = Vec::with_capacity(values.len());
    for value in values {
        let (text, value_ty) = render_expr(value, ctx);
        /* Same reading as `render_owning_operand_statement`: the
         * expression's own type where it is a pointer, and the root
         * pointer otherwise, since `id` is the one spelling that is not a
         * C type and nothing else non-pointer can be an object. */
        let (ty, init) = if value_ty.ends_with('*') {
            (value_ty, text)
        } else {
            (format!("struct {} *", root), format!("(struct {} *)({})", root, text))
        };
        let (line, col) = line_col(ctx.src, value.start_byte());
        ctx.block_counter += 1;
        let tmp = format!("_oz_ce_L{}_C{}_{}", line, col, ctx.block_counter);
        ctx.pre_stmts.push(format!("{}{};", ty, tmp));
        assigns.push(format!("{} = {}", tmp, init));
        releases.push(format!("oz_static_release((struct {} *)({}))", root, tmp));
        ctx.arg_temps.insert(value.id(), (tmp, ty));
        held.push(value.id());
    }
    let (rendered, rendered_ty) = if node.kind() == "message_expression" {
        render_message(node, ctx)
    } else {
        let text = rebuild_or_text(node, ctx);
        (text, call_result_type(node, ctx).unwrap_or_else(|| "id".to_string()))
    };
    for id in &held {
        ctx.arg_temps.remove(id);
    }
    releases.reverse();

    let mut parts = assigns;
    if rendered_ty == "void" {
        parts.push(rendered);
        parts.extend(releases);
        return (format!("({})", parts.join(", ")), "void".to_string());
    }
    let (line, col) = line_col(ctx.src, node.start_byte());
    ctx.block_counter += 1;
    let value_tmp = format!("_oz_cv_L{}_C{}_{}", line, col, ctx.block_counter);
    ctx.pre_stmts.push(format!("{} {};", rendered_ty.trim_end(), value_tmp));
    parts.push(format!("{} = {}", value_tmp, rendered));
    parts.extend(releases);
    parts.push(value_tmp.clone());
    (format!("({})", parts.join(", ")), rendered_ty)
}


/// Every +1 operand of this statement's message sends whose reference
/// nothing else will release, ordered so a nested one comes first (#328,
/// #340).
///
/// What comes back are the *values* to release, read out from behind any
/// casts by `arc`, paired with the position they were written in -- the
/// same nodes the temporaries will take their value from, so what is held
/// and what is released cannot disagree.
///
/// Only *message* operands, and that boundary is the safe half of the
/// change rather than an omission. A message send reaches transpiled
/// Objective-Z, where a callee that keeps the reference retains it (a
/// synthesized setter, `render_strong_ivar_assign`), so dropping the
/// caller's `+1` afterwards leaves the object held by whoever kept it. A
/// plain C function has no such obligation and cannot retain: releasing
/// after `oz_queue_push(q, [Foo new])` would hand the queue a dangling
/// pointer, which is the direction `arc.rs` exists to avoid. So a C call --
/// including a variadic one like `OZLog("%@", [Foo new])` -- is left alone
/// and still leaks. A *receiver* has the same obligation for the same
/// reason: a method that keeps `self` past its own return has to retain it,
/// exactly as one that keeps an argument does.
/// The `+1` operands of an `if`'s or `switch`'s **controlling
/// expression**, and of nothing else in the statement.
///
/// These two positions asked the ownership question nowhere, so
/// `if ([makeThing() n] > 100)` and `switch ([makeThing() n])` leaked
/// outright (#355 follow-up). They are safe to hoist for the reason a
/// `for` **initialiser** is and its condition is not: a controlling
/// expression is evaluated exactly once per execution of the statement,
/// and the group `render_owning_operand_statement` wraps the statement in
/// is itself inside whatever loop encloses it -- so the allocation still
/// happens once per iteration, and the release with it.
///
/// The branches are deliberately not read. A `+1` in the *consequence* is
/// that statement's own business and reaches this machinery through its
/// own arm; hoisting it here would evaluate it whether the branch is
/// taken or not, which is the defect `for_header_owning_operands` avoids
/// by the same restriction.
///
/// `while`, `do`-`while` and the `for` condition/update look identical
/// and are **not** here, for the one reason that matters: they are
/// evaluated more than once, so a hoisted temporary would allocate once
/// where the source allocates every time round. They carry the release
/// inside the expression instead -- see `render_comma_operand_expr`, and
/// `conditionally_evaluated` for the predicate that separates them from
/// the two positions above.
fn condition_owning_operands<'a>(
    node: Node<'a>,
    ctx: &EmitCtx,
) -> Vec<(Node<'a>, OperandPosition)> {
    let mut values: Vec<(Node<'a>, OperandPosition)> = Vec::new();
    if let Some(condition) = node.child_by_field_name("condition") {
        collect_owning_operands(condition, ctx, &mut values);
    }
    values.sort_by_key(|(value, _)| value.end_byte());
    values
}

/// Is this `+1` operand evaluated **conditionally or repeatedly** relative
/// to the statement that would otherwise hoist it into a temporary?
///
/// The single question behind every remaining shape in #376, and it is
/// needed in both directions:
///
///   - where nothing hoists, the operand leaks. `while ([makeThing() n] >
///     100)` creates a `+1` per iteration and abandons every one of them.
///   - where a statement arm *does* hoist, hoisting is **eager**.
///     `if (x && [makeThing() n] > 0)` was a leak until #355's `if` arm
///     reached it, and is now balanced but allocates whether `x` is true
///     or not -- so the short circuit no longer holds. On a one-slot pool
///     that can exhaust the slab from a branch the source never takes,
///     and it is observable outright whenever the factory has side
///     effects.
///
/// Both are answered by declining to hoist and emitting the release
/// *inside* the expression instead (`render_comma_operand_expr`), so the
/// allocation happens exactly where the source evaluates it and the
/// release goes with it.
///
/// What counts is only what lies between the operand and its statement:
///
///   - the right operand of `&&` or `||` -- evaluated only if the left
///     permits it;
///   - either arm of a `? :` -- one of the two is not evaluated;
///   - a `while`/`do` condition, and a `for` condition or **update** --
///     evaluated once per iteration.
///
/// A `for` **initialiser** is deliberately absent: it runs exactly once,
/// which is why `for_header_owning_operands` hoists it and is right to.
/// A loop *body* is absent for a different reason -- the walk stops at the
/// enclosing statement, and a statement inside the body is itself hoisted
/// per iteration, which is already correct.
fn conditionally_evaluated(operand: Node, stmt: Node) -> bool {
    let mut child = operand;
    while let Some(parent) = child.parent() {
        let crossed = match parent.kind() {
            /* Only the *right* operand short-circuits; the left is always
             * evaluated, so an operand under it is not conditional. */
            "binary_expression" => {
                matches!(operator_text(parent), Some("&&") | Some("||"))
                    && parent
                        .child_by_field_name("right")
                        .is_some_and(|right| covers(right, child))
            }
            "conditional_expression" => !parent
                .child_by_field_name("condition")
                .is_some_and(|cond| covers(cond, child)),
            "while_statement" | "do_statement" => parent
                .child_by_field_name("condition")
                .is_some_and(|cond| covers(cond, child)),
            "for_statement" => {
                let in_initializer = parent
                    .child_by_field_name("initializer")
                    .is_some_and(|init| covers(init, child));
                !in_initializer
                    && parent
                        .child_by_field_name("body")
                        .is_none_or(|body| !covers(body, child))
            }
            _ => false,
        };
        if crossed {
            return true;
        }
        if parent.id() == stmt.id() {
            return false;
        }
        child = parent;
    }
    false
}

/// Does `outer` contain `inner`, or is it `inner`?
fn covers(outer: Node, inner: Node) -> bool {
    outer.id() == inner.id()
        || (outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte())
}

/// A binary expression's operator, as written.
fn operator_text<'a>(node: Node<'a>) -> Option<&'a str> {
    node.child_by_field_name("operator").map(|op| op.kind())
}

fn owning_send_operands<'a>(
    stmt: Node<'a>,
    ctx: &EmitCtx,
) -> Vec<(Node<'a>, OperandPosition)> {
    let mut values: Vec<(Node<'a>, OperandPosition)> = Vec::new();
    collect_owning_operands(stmt, ctx, &mut values);
    /* By where each expression *ends*: a nested operand ends before the
     * one containing it, so it is evaluated into its temporary first and
     * the outer initialiser names that temporary rather than allocating
     * again. Siblings end in source order, so they keep it -- and a
     * receiver ends before any of its own send's arguments, which is the
     * order Objective-C evaluates them in. */
    values.sort_by_key(|(value, _)| value.end_byte());
    values
}

/// The +1 operands of a `for` header's **initialiser**, and of nothing
/// else in the header (#341).
///
/// `child_by_field_name("initializer")` and no other child, deliberately.
/// An initialiser runs **once**, which is the whole reason its allocation
/// may be lifted above the loop; the condition runs before every iteration
/// and the update after every one, so lifting either would allocate once
/// where the source allocates every time round -- handing the same object
/// to every evaluation and changing what the program does rather than only
/// where it frees. Both of those still leak, and
/// `a_for_conditions_owning_operand_is_not_hoisted` is what keeps them from
/// being swept in by a later widening.
///
/// Note this is the *opposite* of the constraint that shaped #328, which
/// avoided `ctx.pre_stmts` precisely because hoisting an allocation out of
/// a loop **body** would run it once instead of per iteration
/// (`an_owning_argument_inside_a_loop_allocates_per_iteration`). A header
/// initialiser is the one place in a loop where hoisting is the correct
/// answer, which is why this is a separate arm and not a widened guard on
/// the declaration one.
fn for_header_owning_operands<'a>(
    node: Node<'a>,
    ctx: &EmitCtx,
) -> Vec<(Node<'a>, OperandPosition)> {
    let mut values: Vec<(Node<'a>, OperandPosition)> = Vec::new();
    if let Some(init) = node.child_by_field_name("initializer") {
        collect_owning_operands(init, ctx, &mut values);
    }
    values.sort_by_key(|(value, _)| value.end_byte());
    values
}

/// The `for` header **declaration** whose own initialiser is `+1`, and
/// the names it binds -- `None` for every other header (#376).
///
/// This is the one shape in that issue evaluated exactly *once*, so
/// nothing about when it allocates was ever wrong. What was wrong is
/// where the name lives: `owned_locals_of` is reached from `arc_note`,
/// for a `declaration` whose parent is a `compound_statement`, and a
/// header declaration's parent is the `for_statement`. So no scope could
/// see `t` in
///
/// ```objc
/// for (Thing *t = makeThing(); i < 1; i++) { [t n]; }
/// ```
///
/// and the reference was created and abandoned, once per execution of the
/// loop -- measured as `n=1`, `nil`, `nil` against a one-slot slab.
///
/// Both guards are load-bearing, and they are the same pair every other
/// binding position reads:
///
///   - `owned_locals_of_in` for provenance, so a **borrowed** initialiser
///     (`for (Thing *t = [owned itself]; ...)`) is left alone -- releasing
///     one is a use-after-free on whatever still names the object. It
///     also brings the three exclusions it makes anywhere else: a
///     `static` slot, `__unsafe_unretained`, and a name the author
///     already releases by hand. The search root is the `for_statement`,
///     so a manual `[t release]` in the loop *body* counts.
///   - `arc::declares_pointer` for the type, so a plain
///     `for (int i = 0; ...)` -- by far the most common thing this is
///     asked about -- comes out byte-identical. The type check is left to
///     the position that needs it for the reason #351 records: widening
///     `binds_ownership` instead would have a scope release an `int`.
fn for_header_owned_declaration<'a>(
    node: Node<'a>,
    ctx: &EmitCtx,
) -> Option<(Node<'a>, Vec<String>)> {
    let init = node.child_by_field_name("initializer")?;
    if init.kind() != "declaration" || !crate::arc::declares_pointer(init) {
        return None;
    }
    let owned = owned_locals_of_in(init, init.parent(), ctx);
    if owned.is_empty() {
        return None;
    }
    Some((init, owned))
}

/// Lift a `for` header's owning declaration into a wrapping group, so
/// there is a scope that can see the name and release it (#376).
///
/// ```c
/// {
///         struct Thing *t = makeThing();
///         for (; i < 1; i++) { ... }
///         oz_static_release((struct OZObject *)(t));
/// }
/// ```
///
/// The header is **rewritten** rather than the statement wrapped, which
/// is the difference from `render_owning_operand_statement`: an operand
/// can stay where it is and be named by a temporary, but a declaration's
/// *name* has to move for anything to be able to release it. Bracing
/// scopes nothing out -- a header declaration was already scoped to the
/// loop -- and the empty initialiser it leaves behind is what keeps the
/// allocation happening exactly once, where the source put it.
///
/// Two things it must get right, each with a test of its own in
/// `tests/for_header_ownership.rs`:
///
///   - **The group is a real ARC scope.** Without that a `return` inside
///     the loop jumps straight past the trailing release, so the leak is
///     back on exactly the path an early exit takes.
///   - **Its `start_byte` is the `for_statement`'s own.**
///     `releases_up_to_jump_target` releases the scopes that began
///     *strictly inside* the construct being left, so a group starting at
///     the same byte as the loop is left alone by a `break` out of it --
///     which is required, since `t` must live until the loop exits and
///     the trailing release is what frees it. One byte later, or given
///     the body's offset, and the release would run twice.
///
/// The loop itself is rebuilt by `rebuild`, with the initialiser child
/// replaced by the bare `;` the header still needs, so every other byte
/// of the header and body -- spacing, comments, an unexpanded macro --
/// survives exactly as the ordinary `rebuild_or_text` path would have
/// left it. The lifted declaration goes back through `render_expr`, so
/// its declared type is lowered by the same code that lowers every other
/// declaration.
fn render_for_header_owned_declaration(
    node: Node,
    ctx: &mut EmitCtx,
    init: Node,
    owned: Vec<String>,
) -> (String, String) {
    let (declaration, _) = render_expr(init, ctx);
    ctx.arc_scopes.push(ArcScope {
        owned: owned.clone(),
        start_byte: node.start_byte(),
        is_block_body: false,
    });
    let loop_text = rebuild(node, ctx, &mut |child, ctx| {
        if child.id() == init.id() {
            /* The declaration carried the header's first `;` with it, so
             * the empty initialiser has to put one back. */
            return Some(";".to_string());
        }
        if needs_translation(child) {
            Some(render_expr(child, ctx).0)
        } else {
            None
        }
    });
    ctx.arc_scopes.pop();
    /* Reverse order, so a later name outlives the ones declared before
     * it -- the same order `arc_exit` releases a block's locals in. */
    let releases =
        release_lines(&owned.iter().rev().cloned().collect::<Vec<_>>(), ctx);
    let lines: Vec<&String> =
        std::iter::once(&declaration).chain(std::iter::once(&loop_text)).chain(releases.iter()).collect();
    (braced_group(&lines), "void".to_string())
}

fn collect_owning_operands<'a>(
    node: Node<'a>,
    ctx: &EmitCtx,
    out: &mut Vec<(Node<'a>, OperandPosition)>,
) {
    collect_owning_operands_in(node, node, ctx, out)
}

/// The recursive half, carrying the statement the hoist would attach to so
/// that `conditionally_evaluated` has something to measure against.
///
/// An operand it declines is not forgotten -- `render_expr` picks it up and
/// emits the release inside the expression instead
/// (`render_comma_operand_expr`). Declining here is what makes the two
/// mechanisms exclusive: whichever one takes an operand, the other must not
/// also.
fn collect_owning_operands_in<'a>(
    stmt: Node<'a>,
    node: Node<'a>,
    ctx: &EmitCtx,
    out: &mut Vec<(Node<'a>, OperandPosition)>,
) {
    /* A `block_literal`'s body is a separate function that runs later and
     * however many times it is called. Hoisting an operand out of it
     * would allocate once, here, where the source allocates per call. */
    if node.kind() == "block_literal" {
        return;
    }
    if node.kind() == "message_expression" {
        /* The receiver first, so it keeps its place under the sort even
         * against an argument that starts at the same byte. */
        if let Some(value) = crate::arc::receiver_owning_value(
            node,
            ctx.src,
            ctx.program,
            &ctx.program.owning_methods,
        ) {
            if !ctx.arg_temps.contains_key(&value.id())
                && !conditionally_evaluated(value, stmt)
            {
                out.push((value, OperandPosition::Receiver));
            }
        }
        for arg in parse_message(node, ctx.src).args {
            let Some(value) = crate::arc::owning_argument_value(
                arg,
                ctx.src,
                ctx.program,
                &ctx.program.owning_methods,
            ) else {
                continue;
            };
            /* Already held in a temporary by this same renderer, which is
             * what lets its recursive render fall through to the ordinary
             * dispatch instead of back into this arm. */
            if !ctx.arg_temps.contains_key(&value.id())
                && !conditionally_evaluated(value, stmt)
            {
                out.push((value, OperandPosition::Argument));
            }
        }
    }
    /* A **plain C call**'s arguments, which asked the ownership question
     * nowhere at all: `keep(makeThing())` handed over a +1 and no one
     * released it. Measured rather than reasoned -- one slab slot per
     * allocation site means the second `keep(makeThing(...))` in a
     * program got `nil` and the third did too, so the consequence on a
     * device is a program that quietly stops allocating (#355 follow-up).
     *
     * The free-function twin of the `message_expression` arm above, and
     * the same asymmetry as #326, #336 and #367 in a fifth place: a
     * method's argument list and a function's are two separate walks over
     * one question, and only one of them was written.
     *
     * There is no receiver half here, and no consuming-function half
     * either: oz_static has no way to say a C function takes ownership,
     * so every one of them borrows -- which is exactly what ARC assumes
     * of an unannotated C function too, and what
     * `ownership_matrix.rs`'s `cArgument` shape pins down. */
    if node.kind() == "call_expression" {
        if let Some(args) = node.child_by_field_name("arguments") {
            let mut cursor = args.walk();
            let arg_nodes: Vec<Node<'a>> = args
                .children(&mut cursor)
                .filter(|c| !matches!(c.kind(), "(" | ")" | ","))
                .collect();
            for arg in arg_nodes {
                let Some(value) = crate::arc::owning_argument_value(
                    arg,
                    ctx.src,
                    ctx.program,
                    &ctx.program.owning_methods,
                ) else {
                    continue;
                };
                if !ctx.arg_temps.contains_key(&value.id())
                    && !conditionally_evaluated(value, stmt)
                {
                    out.push((value, OperandPosition::Argument));
                }
            }
        }
    }
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    for child in children {
        collect_owning_operands_in(stmt, child, ctx, out);
    }
}

/// Hold each +1 argument in a temporary, send the message, then release --
/// the call site's own reference given up once the callee has had its
/// chance to keep it (#328).
///
/// This is the shape the issue left open, chosen over the alternative of
/// having a synthesized strong setter *consume* a `+1` argument instead of
/// retaining it. Consuming is cheaper -- no temporary, no release -- but it
/// is only ever correct for a callee that stores the argument, so it does
/// nothing for `[self doThing:[Foo new]];` where `-doThing:` merely
/// borrows; it changes that setter's contract, so every call site passing a
/// *borrowed* value would then owe a retain; and it needs the callee's
/// identity, which a dynamically dispatched send does not have. Releasing
/// at the call site needs none of that and is right for any callee, so it
/// generalises where consuming does not. Consuming stays available later as
/// an optimisation for the synthesized-setter case specifically.
///
/// The counts work out because the setter's retain is left exactly as it
/// was: `Foo_new()` is +1, the setter's `oz_static_retain` makes it +2, and
/// the release here brings it back to +1 -- held by the ivar, which is what
/// releases it at `-dealloc`. A borrowed argument reaches none of this and
/// keeps the retain it always got.
///
/// Emitted as a braced group rather than through `ctx.pre_stmts`, and the
/// reason is the one `render_strong_local_assign` records: `pre_stmts` are
/// drained by the enclosing *top-level* statement, so a temporary written
/// for a send inside a loop is hoisted above it -- and hoisting an
/// *allocation* out of a loop would run it once and release it once while
/// the body used it every iteration. A block is self-contained, and legal
/// wherever a statement is, so an unbraced `if (x) [self setFoo:[Foo
/// new]];` needs no special case.
///
/// Since #341 the statement wrapped can be a whole `for` loop, which is
/// why the group's own indentation walks the statement's lines rather than
/// prefixing only the first. That case is also the one exception to the
/// paragraph above: a `for` header's initialiser runs *once*, so lifting
/// its allocation above the loop is correct -- see
/// `for_header_owning_operands`, which is careful to read the initialiser
/// and not the condition or the update.
///
/// The statement itself is then rendered by the ordinary dispatch, with
/// `ctx.arg_temps` standing in for the arguments (see `render_expr`), so
/// nothing here has to know how a send is called -- direct, dynamic, or
/// desugared -- and a statement that *also* discards a +1 result still
/// reaches the #322 arm.
///
/// `oz_static_release` is null-safe, so an allocation that found no free
/// slab slot needs no guard of its own.
fn render_owning_operand_statement(
    node: Node,
    ctx: &mut EmitCtx,
    values: Vec<(Node, OperandPosition)>,
) -> (String, String) {
    let root = ctx.program.root_class().unwrap_or("OZObject").to_string();
    let mut decls: Vec<String> = Vec::with_capacity(values.len());
    let mut releases: Vec<String> = Vec::with_capacity(values.len());
    let mut held: Vec<usize> = Vec::with_capacity(values.len());
    let mut names: Vec<String> = Vec::with_capacity(values.len());
    for (value, position) in values {
        let (text, value_ty) = render_expr(value, ctx);
        /* The expression's own type, so the argument the send receives is
         * the type it always was and no caller of `arg_texts` has to be
         * told anything. `id` is the one spelling that is not a C type;
         * anything else that is not a pointer cannot be an object at all,
         * and the root pointer a release needs is the safe reading. */
        let (ty, init) = if value_ty.ends_with('*') {
            (value_ty, text)
        } else {
            (format!("struct {} *", root), format!("(struct {} *)({})", root, text))
        };
        let (line, col) = line_col(ctx.src, value.start_byte());
        ctx.block_counter += 1;
        let tmp = format!("{}_L{}_C{}_{}", position.prefix(), line, col, ctx.block_counter);
        decls.push(format!("{}{} = {};", ty, tmp, init));
        releases.push(format!("oz_static_release((struct {} *)({}));", root, tmp));
        names.push(tmp.clone());
        ctx.arg_temps.insert(value.id(), (tmp, ty));
        held.push(value.id());
    }
    /* The braced form is a scope, so it is registered as one -- otherwise
     * a `return` inside the statement jumps straight past the releases
     * below and the temporaries leak. That is not hypothetical: the arm
     * that brought `if` here makes
     * `if ([makeThing() n] > 100) { return; }` reachable, which is about
     * as ordinary as this construct gets, and the same hole was already
     * open under #341's `for` wrapper.
     *
     * Neither boundary flag is set, and both readings are deliberate:
     *
     *   - not a loop body, so a `break` or `continue` *inside* a wrapped
     *     `for` unwinds to the loop's own body scope and stops there,
     *     leaving these temporaries alone. They belong to the group
     *     outside the loop, and the trailing releases still run when it
     *     finishes.
     *   - not a block body, so a `return` does unwind through them --
     *     which is the whole point.
     *
     * The unbraced `declaration` form needs none of this: no jump can
     * occur part-way through a single declaration. */
    let scoped = node.kind() != "declaration";
    if scoped {
        ctx.arc_scopes.push(ArcScope {
            owned: names.clone(),
            /* The wrapped statement's own start, so a `break` out of a
             * wrapped `for` or `switch` does *not* reach these: the group
             * begins at the same byte as the construct being left, not
             * inside it, which is the boundary
             * `releases_up_to_jump_target` tests. The trailing releases
             * still run when the construct finishes. */
            start_byte: node.start_byte(),
            is_block_body: false,
        });
    }
    let (rendered, _) = render_expr(node, ctx);
    if scoped {
        ctx.arc_scopes.pop();
    }
    for id in &held {
        ctx.arg_temps.remove(id);
    }
    /* Released in the reverse of the order they were taken, so a nested
     * operand outlives the one built from it. */
    releases.reverse();
    /* ...and not at all when the wrapped statement *is* a jump: the
     * `return` already released them on its way out, through the scope
     * pushed above, and a second copy after it is unreachable. The test
     * is deliberately `is_jump_statement` on the statement itself and not
     * "contains a jump": an `if` whose branch returns still needs these
     * on the path where the branch is not taken. Same rule as
     * `arc_exit`'s `ended_with_jump`. */
    if is_jump_statement(node) {
        releases.clear();
    }
    /* A slot this statement binds may *be* one of the temporaries about to
     * be released, so it takes a `+1` of its own first -- see
     * `retained_bindings`, and note the retain has to be emitted ahead of
     * the releases, not after. The name then becomes an owned local of the
     * enclosing scope unless something already claims it, which is what
     * makes every later question about it (scope exit, reassignment,
     * `return`) fall to machinery that already exists. */
    let mut retains: Vec<String> = Vec::new();
    if node.kind() == "declaration" {
        let already_owned = owned_locals_of(node, ctx);
        for name in retained_bindings(node, ctx) {
            retains.push(format!(
                "oz_static_retain((struct {} *)({}));",
                root, name
            ));
            if !already_owned.contains(&name) {
                if let Some(scope) = ctx.arc_scopes.last_mut() {
                    scope.owned.push(name);
                }
            }
        }
    }
    let lines: Vec<&String> = decls
        .iter()
        .chain(std::iter::once(&rendered))
        .chain(retains.iter())
        .chain(releases.iter())
        .collect();
    /* A declaration is a bare group: bracing it would scope the name it
     * introduces out of the rest of the body. */
    if node.kind() == "declaration" {
        return (
            lines.iter().map(|line| line.as_str()).collect::<Vec<_>>().join("\n\t"),
            "void".to_string(),
        );
    }
    (braced_group(&lines), "void".to_string())
}

/// Wrap already-rendered statements in a braced group, one level deeper.
///
/// Shared by the two renderers that wrap a statement in a scope of their
/// own -- `render_owning_operand_statement` and
/// `render_for_header_owned_declaration` -- because getting the
/// indentation right is subtler than it looks and getting it right twice
/// is how the two drift apart.
///
/// Every line of each entry is indented, not only its first. A
/// declaration or a release is one line, but the *statement* need not be:
/// since #341 it can be a whole `for` loop, and prefixing only its
/// opening line left the loop body at its original depth -- one level
/// shallower than the brace it now sits inside, which reads as if the
/// body had escaped the group.
///
/// One extra tab on the continuation lines and two on the first, because
/// the two start from different depths: a rendered statement's opening
/// line begins at the statement token with no indentation of its own,
/// while the lines after it are passed through carrying the author's,
/// already at the depth the statement had before this group was wrapped
/// around it.
fn braced_group(lines: &[&String]) -> String {
    let mut out = String::from("{\n");
    for line in lines {
        for (i, text) in line.lines().enumerate() {
            if !text.is_empty() {
                out.push_str(if i == 0 { "\t\t" } else { "\t" });
                out.push_str(text);
            }
            out.push('\n');
        }
    }
    out.push_str("\t}");
    out
}

/// A nested block that owns object locals: render its statements, then
/// release what it owns on the way out.
///
/// This is oz_static's ARC. The oracle does the same job by tracking
/// `ctx.scope_vars` across its whole statement emitter
/// (`emit.py::_emit_scope_releases`); here it is attached to the block that
/// actually owns the locals, so a block owning none is untouched.
///
/// A block ending in a jump gets no trailing releases -- the jump already
/// emitted them, and code after it would be unreachable anyway.
/// Enter an ARC scope for the block about to be rendered.
///
/// The three `arc_*` helpers exist so that every block renderer does the
/// same bookkeeping. They were factored out after `@autoreleasepool` was
/// found to do none of it: its arm sits before the ARC one in
/// `render_expr`'s match, so a pool block that declared an owned local got
/// the pool renderer and never the releases. `samples/heap_alloc` leaked
/// every object it allocated that way -- and it says so in its own expected
/// output, which no compile or link could have checked.
fn arc_enter(ctx: &mut EmitCtx, body: Node) {
    ctx.arc_scopes.push(ArcScope::for_body(body));
}

/// Record whatever owned locals `stmt` just declared.
fn arc_note(stmt: Node, ctx: &mut EmitCtx) {
    if stmt.kind() != "declaration" {
        return;
    }
    let owned = owned_locals_of(stmt, ctx);
    if let Some(scope) = ctx.arc_scopes.last_mut() {
        scope.owned.extend(owned);
    }
}

/// Leave the scope, returning the releases it owes -- none when the block
/// ended in a jump, which released on its way out (`render_loop_jump` /
/// `render_return_statement`).
fn arc_exit(ctx: &mut EmitCtx, ended_with_jump: bool) -> Vec<String> {
    let scope = ctx.arc_scopes.pop().unwrap_or_default();
    if ended_with_jump {
        return Vec::new();
    }
    release_lines(&scope.owned.iter().rev().cloned().collect::<Vec<_>>(), ctx)
}

/// Did this statement leave the block by jumping?
fn is_jump_statement(node: Node) -> bool {
    matches!(
        node.kind(),
        "return_statement" | "break_statement" | "continue_statement" | "goto_statement"
    )
}

fn render_scoped_block(body: Node, ctx: &mut EmitCtx) -> (String, String) {
    let mut cursor = body.walk();
    let children: Vec<Node> = body.children(&mut cursor).collect();
    if children.len() < 2 {
        return (node_text(body, ctx.src).to_string(), "void".to_string());
    }
    let stmts = &children[1..children.len() - 1];

    arc_enter(ctx, body);
    let mut out = String::from("{\n");
    let mut ended_with_jump = false;
    for stmt in stmts {
        let rendered = render_expr(*stmt, ctx).0;
        if !ctx.pre_stmts.is_empty() {
            let pre = ctx.pre_stmts.join("\n\t");
            ctx.pre_stmts.clear();
            out.push('\t');
            out.push_str(&pre);
            out.push('\n');
        }
        out.push('\t');
        out.push_str(&rendered);
        out.push('\n');
        arc_note(*stmt, ctx);
        ended_with_jump = is_jump_statement(*stmt);
    }
    for line in arc_exit(ctx, ended_with_jump) {
        out.push('\t');
        out.push_str(&line);
        out.push('\n');
    }
    out.push('}');
    (out, "void".to_string())
}

fn render_body_with_comments(body: Node, ctx: &mut EmitCtx) -> String {
    let mut cursor = body.walk();
    let children: Vec<Node> = body.children(&mut cursor).collect();
    if children.len() < 2 {
        return node_text(body, ctx.src).to_string();
    }
    let stmts = &children[1..children.len() - 1];

    ctx.arc_scopes.push(ArcScope::for_body(body));
    /* (rendered text, the original it came from, its byte offset) -- the
     * offset is what a `#line` directive for this statement is resolved
     * from, and it has to be captured here while the node is in hand. */
    let mut rendered_stmts: Vec<(String, &str, usize)> = Vec::with_capacity(stmts.len());
    let mut ended_with_jump = false;
    for stmt in stmts {
        let rendered = render_expr(*stmt, ctx).0;
        let combined = if ctx.pre_stmts.is_empty() {
            rendered
        } else {
            let pre = ctx.pre_stmts.join("\n\t");
            ctx.pre_stmts.clear();
            format!("{}\n\t{}", pre, rendered)
        };
        rendered_stmts.push((combined, node_text(*stmt, ctx.src), stmt.start_byte()));
        if stmt.kind() == "declaration" {
            let owned = owned_locals_of(*stmt, ctx);
            if let Some(scope) = ctx.arc_scopes.last_mut() {
                scope.owned.extend(owned);
            }
        }
        ended_with_jump = matches!(
            stmt.kind(),
            "return_statement" | "break_statement" | "continue_statement" | "goto_statement"
        );
    }
    let scope = ctx.arc_scopes.pop().unwrap_or_default();
    let trailing: Vec<String> = if ended_with_jump {
        Vec::new()
    } else {
        release_lines(&scope.owned.iter().rev().cloned().collect::<Vec<_>>(), ctx)
    };
    if trailing.is_empty()
        && rendered_stmts.iter().all(|(rendered, original, _)| rendered == original)
    {
        /* Byte-identical, and *no* directive: a verbatim body needs one
         * anchor on its brace and then nothing (its lines are the
         * source's, so they follow on their own), but this function
         * cannot place it. Its result is spliced in by callers that are
         * not at the start of a line -- `for (...) <body>` is the shape
         * that proved it, where the anchor became
         * `for (...) #line 29 "main.m"` and GCC answered "stray '#' in
         * program". A directive may be indented but may not share a line.
         *
         * So the anchor is the caller's to emit, and only the callers that
         * know they are at line start do (`render_method_definition`, the
         * top-level `function_definition` arm) -- see `body_anchor`. A
         * nested body inherits the enclosing statement's position, which
         * is what every other nested construct here already does. */
        return node_text(body, ctx.src).to_string();
    }

    let mut out = String::from("{\n");
    for (rendered, original, offset) in &rendered_stmts {
        /* Below the `/* original */` comment, not above it: the directive
         * names the line the *next* emitted line is, and the line that has
         * to be the statement's is the statement's, not its comment's. */
        if rendered == original {
            out.push_str(&ctx.lines.before(*offset));
            out.push('\t');
            out.push_str(original);
        } else {
            out.push_str("\t/* ");
            out.push_str(&one_line(original));
            out.push_str(" */\n");
            out.push_str(&ctx.lines.before(*offset));
            out.push('\t');
            out.push_str(rendered);
        }
        out.push('\n');
    }
    for line in &trailing {
        out.push('\t');
        out.push_str(line);
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
///   - a bare class name (`OZHeap *_heap;`) gains its `struct` tag. The
///     generated struct for a class is `struct Name`, never a typedef, so
///     the untagged spelling is `error: must use 'struct' tag to refer to
///     type 'OZHeap'`. Every other type position already routes through
///     `collect::render_type` for this; an ivar declaration is copied
///     through as text, so it has to be done here. `struct OZHeap
///     *_heap;` is left alone -- its name sits under a `struct_specifier`
///     rather than being a direct child, so it never matches.
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
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    collect_ivar_lowering_edits(instance_variable, ctx, origin, &mut edits);
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (range, replacement) in edits {
        text.replace_range(range, &replacement);
    }
    // A stripped qualifier leaves behind the whitespace it sat in.
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace(" ;", ";")
}

/// Seed a plain C function's parameters into `ctx`, the way
/// `render_method_definition` seeds a method's (#250).
///
/// Without this a free function's scope was `file_scope_vars` and nothing
/// else, so `[w n]` on a `Widget *w` parameter was rejected as an `id`
/// receiver while the identical method `- (int)read:(Widget *)w` resolved.
/// Same class of omission as gap Q, where the static bar turned out never to
/// scan a free function at all: the free-function path kept getting a reduced
/// version of what a method body gets.
///
/// Every parameter is inserted, not only the object-typed ones, because that
/// is what a method does and the two paths drifting is what produces this
/// shape of bug (#246, gap R).
///
/// Adding them to `ctx.locals` cannot make ARC release a borrowed parameter:
/// `managed_object_locals` looks for `declaration` nodes *inside the body*,
/// and a parameter is a `parameter_declaration` outside it.
fn collect_function_params(func_node: Node, ctx: &mut EmitCtx) {
    let known: std::collections::HashSet<String> = ctx.program.classes.keys().cloned().collect();
    let mut lists = Vec::new();
    find_parameter_lists(func_node, &mut lists);
    // The first list is the function's own: `find_parameter_lists` stops
    // descending once it matches, so a function-pointer parameter's own
    // parameter list is never mistaken for it.
    let Some(plist) = lists.first() else {
        return;
    };
    let mut cursor = plist.walk();
    for child in plist.children(&mut cursor) {
        if child.kind() != "parameter_declaration" {
            continue;
        }
        let (type_text, stars) = crate::collect::extract_type_and_stars(child, ctx.src);
        let c_type = crate::collect::render_type(&type_text, stars, &known);
        let name = crate::collect::find_declared_name(child, ctx.src);
        if !name.is_empty() {
            ctx.scope.insert(name.clone(), c_type);
            ctx.locals.insert(name);
        }
    }
}

/// Every `parameter_list` under `node`.
fn find_parameter_lists<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    if node.kind() == "parameter_list" {
        out.push(node);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_parameter_lists(child, out);
    }
}

/// Is `node` a bare `id` standing in *type* position?
///
/// Both node kinds a bare `id` can appear as are matched: `type_identifier`
/// where the grammar reads it as an ordinary type name, and
/// `typedefed_specifier` where it reads it as a typedef reference. Asking
/// the CST rather than the text is what keeps a parameter *typed* `id`
/// distinct from one merely *named* it (#317).
fn is_bare_id_type(node: Node, src: &str) -> bool {
    if matches!(node.kind(), "type_identifier" | "typedefed_specifier")
        && node_text(node, src).trim() == "id"
    {
        return true;
    }
    /* `id<Proto>` is the same type for lowering purposes -- a protocol
     * qualification constrains what may be assigned to it and says nothing
     * about its representation, so it lowers to the root class pointer
     * exactly as a bare `id` does. The grammar files it as a
     * `generic_specifier` with `id` as the base, so the strict text test
     * above missed it and `void f(id<Marker> m)` reached GCC verbatim:
     * `expected ')'` (#367).
     *
     * Answered here rather than at the one call site that reported it,
     * because every position that lowers an `id` should lower both
     * spellings -- a block literal's parameter list asks this same
     * predicate, and would otherwise have kept the same hole. */
    if node.kind() == "typedefed_specifier" {
        /* The grammar gives `id<Marker>` a `typedefed_specifier` holding an
         * `id` node and a `protocol_reference_list`, so the whole-text
         * comparison above sees `id<Marker>` and not `id`. Ask for the `id`
         * child instead. */
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        return children.iter().any(|child| child.kind() == "id")
            && children.iter().any(|child| child.kind() == "protocol_reference_list");
    }
    false
}

/// Does a bare `id` type appear anywhere at or under `node`?
fn contains_bare_id_type(node: Node, src: &str) -> bool {
    if is_bare_id_type(node, src) {
        return true;
    }
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).any(|child| contains_bare_id_type(child, src));
    found
}

/// Rewrite every bare `id` type name under `node` to `replacement`.
///
/// A declarator's own `*` is a separate token and is left alone, so `id *`
/// becomes `struct Root **` as it should.
pub(crate) fn rewrite_id_types(
    node: Node,
    src: &str,
    origin: usize,
    replacement: &str,
    edits: &mut Vec<(Range<usize>, String)>,
) {
    if is_bare_id_type(node, src) {
        edits.push((node.start_byte() - origin..node.end_byte() - origin, replacement.to_string()));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        rewrite_id_types(child, src, origin, replacement, edits);
    }
}

fn collect_ivar_lowering_edits(
    node: Node,
    ctx: &mut EmitCtx,
    origin: usize,
    edits: &mut Vec<(Range<usize>, String)>,
) {
    if node.kind() == "struct_declaration" {
        // Only a *direct* type_identifier child is an untagged type name;
        // in `struct OZHeap *x` the name hangs off a `struct_specifier`.
        let mut cursor = node.walk();
        let bare = node
            .children(&mut cursor)
            .find(|c| c.kind() == "type_identifier")
            .map(|c| (c.byte_range(), node_text(c, ctx.src).to_string()));
        if let Some((range, name)) = bare {
            if ctx.program.is_class(&name) {
                edits.push((
                    range.start - origin..range.end - origin,
                    format!("struct {}", name),
                ));
            }
        }

        // A function-pointer ivar's own parameter list: an `id` there is
        // spelled as the root class pointer rather than left to the `id`
        // typedef.
        //
        // The field's type is what external C code has to match when it
        // assigns to the field, so it has to be the honest one. `OZDefer`'s
        // ivar is `void (^_block)(id)`, and with `id` left as a typedef for
        // `void *` the field came out `void (*)(void *)` -- so assigning an
        // ordinary `void (*)(struct OZObject *)` function to it was
        // "incompatible function pointer types", which is exactly what
        // `tests/behavior/cases/foundation/defer_block_ivar`'s driver does.
        // The Python backend's field type is `void (*)(struct OZObject *)`
        // too, since its own `id` typedef is `struct OZObject *`.
        //
        // Not in `collect::render_type`, which keeps resolving a *method's*
        // `id` to `void *`: a method's arguments pass through oz_static's own
        // casts at every call site, and `void *` is what lets a concrete
        // class pointer reach an `id` parameter without one. Making `id`
        // itself the root pointer everywhere was tried and is worse -- it
        // turns the ordinary Objective-C idiom of passing `Foo *` where `id`
        // is expected into a warning, in code that has no call site to cast
        // at either.
        //
        // The parameter list hangs off this declaration, not off the
        // `block_pointer_declarator`, whose only children are the `^` and
        // the field name.
        if let Some(root) = ctx.program.root_class() {
            let replacement = format!("struct {} *", root);
            let mut lists = Vec::new();
            find_parameter_lists(node, &mut lists);
            for list in lists {
                rewrite_id_types(list, ctx.src, origin, &replacement, edits);
            }
        }
    }
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
                edits.push((node.start_byte() - origin..node.end_byte() - origin, String::new()));
            }
            return;
        }
        "block_pointer_declarator" => {
            let mut c = node.walk();
            let caret = node.children(&mut c).find(|n| n.kind() == "^").map(|n| n.byte_range());
            if let Some(caret) = caret {
                edits.push((caret.start - origin..caret.end - origin, "*".to_string()));
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
            // The whole superclass chain, not just this class's own
            // methods: an inherited implementation satisfies a protocol
            // requirement (#307). Reading `info.methods` alone made
            // `ObjectProtocol` unadoptable, since every method it
            // declares is defined once, on the root class.
            let implemented = program.implements_selector(
                &name,
                &required.selector,
                required.is_class_method,
            );
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
            // `struct oz_metadata` is the PAL's own type
            // (`platform/oz_platform_types.h`): a packed bitfield holding
            // class_id, heap_allocated, deallocating and immortal. Using it
            // rather than a hand-rolled set of `uint8_t` siblings costs
            // nothing to adopt, is what the Python backend's root struct
            // already does, and folds four flags into the four bytes one of
            // them used to take on its own.
            //
            // It also settles a naming question the two backends had
            // answered differently for no reason: three of the behavior
            // corpus's drivers assert on `obj->base._meta.class_id`, and no
            // `#define` can rewrite `a._meta.b` into a flat `a.oz_b` -- the
            // names are separate tokens joined by `.`. They were
            // unbuildable purely because of the spelling.
            //
            // `oz_refcount` stays a sibling, exactly as in the oracle's
            // own root struct: it is `oz_atomic_t`, not a bitfield, and
            // every driver reaches it through `__objc_refcount_get` anyway.
            // Spelled in full here because `OZObject.h` used to declare an
            // `int _refcount` beside it that nothing read, and this comment
            // was easy to mistake for a defence of that one (#371).
            let mut f = String::from(
                "\tstruct oz_metadata _meta; /* synthesized: class_id, and the deallocating/heap/immortal flags */\n\
                 \toz_atomic_t oz_refcount; /* synthesized: retain count */\n",
            );
            // Shared lock for every atomic property in the program --
            // reached from any class via `Program::ivar_access_path`'s
            // ordinary "base." hop-chain, same as any inherited ivar.
            if program.has_atomic_property() {
                f.push_str(
                    "\toz_spinlock_t oz_prop_lock; /* synthesized: guards atomic property access */\n",
                );
            }
            // One lock per object, so `@synchronized(obj)` excludes on `obj`
            // rather than on a lock the caller happened to have on its own
            // stack -- which excluded nothing between cores. Zero-initialized
            // for free: `{Class}_oz_alloc` memsets the whole object, a static
            // boxed literal is zero-initialized by C, and `oz_spin_init` is
            // itself a memset. Costs nothing on a single-core target, where
            // `struct k_spinlock` has no members at all.
            if program.uses_synchronized {
                f.push_str(
                    "\toz_spinlock_t oz_sync_lock; /* synthesized: guards @synchronized(self) */\n\
                     \tvoid *oz_sync_owner; /* synthesized: thread holding oz_sync_lock, 0 when free */\n",
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
                // `@public`/`@private`/`@protected`/`@package` each get their
                // own `instance_variable` wrapper holding nothing but a
                // `visibility_specification`. They are ObjC access control
                // with no C equivalent, and copied through they are a syntax
                // error in the generated struct -- "type name requires a
                // specifier or qualifier", which samples/hello_category's Car
                // hit. Dropping them leaves every field reachable, which the
                // generated C already was: nothing enforced visibility once
                // the struct became plain C.
                if child_by_kind_local(child, "visibility_specification").is_some() {
                    continue;
                }
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
    let mut emitted: std::collections::HashSet<String> = raw_ivar_names.clone();
    for prop in &info.properties {
        if let Some(ivar) = &prop.ivar_name {
            if !raw_ivar_names.contains(ivar) {
                ivars_text.push_str(&format!(
                    "\t{} {}; /* synthesized: backs property '{}' */\n",
                    prop.c_type, ivar, prop.name
                ));
                emitted.insert(ivar.clone());
            }
        }
    }
    // An ivar declared in the `@implementation` block rather than the
    // `@interface` (valid modern Objective-C, and what
    // `samples/hello_category`'s Car does) was collected onto the class but
    // is not in *this* node's text, since that text is the interface. Add
    // whatever the class owns that has not been emitted yet, or the struct
    // silently lacks the field and every use is "use of undeclared
    // identifier".
    for (ivar, c_type) in &info.own_ivars {
        if emitted.contains(ivar) {
            continue;
        }
        // `oz_prop_lock` and friends are synthesized onto the root class by
        // `collect::resolve_properties` and already emitted above as part of
        // its tracking fields; re-emitting one here is a duplicate member.
        // User ivars are `_`-prefixed by convention, so the `oz_` namespace
        // is unambiguous.
        if ivar.starts_with("oz_") {
            continue;
        }
        // C spells an array's extent after the name, so it cannot ride in
        // the type -- it comes from `array_extents`, or the field silently
        // becomes a scalar while every use of it keeps its subscript
        // (#287). An ivar declared in the `@interface` never reaches here:
        // `lower_ivar_decl` copies that declaration through verbatim,
        // extent included, which is why only this path was ever wrong.
        let extent = info.array_extents.get(ivar).map(String::as_str).unwrap_or("");
        ivars_text.push_str(&format!(
            "\t{} {}{}; /* from the @implementation block */\n",
            c_type, ivar, extent
        ));
        emitted.insert(ivar.clone());
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
            decls.push_str(&render_prototype(&name, &sig, ctx.program.root_class()));
        }
    }
    for m in &info.methods {
        if !declared.contains(&(m.selector.clone(), m.is_class_method)) {
            decls.push_str(&render_prototype(&name, m, ctx.program.root_class()));
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
        let slots = ctx.pools.for_class(&name);
        let owned_ivars = ctx.program.owned_object_ivars(&name);
        let (alloc_free, extra_proto) = if name == "OZArray" {
            (
                crate::companion::render_array_support(
                    &name,
                    &root,
                    slots,
                    &owned_ivars,
                    ctx.program.heap_support,
                    ctx.pools.item_slots(),
                ),
                format!("struct {name} *{name}_oz_initWithItems(void **src, unsigned int count);\n", name = name),
            )
        } else if name == "OZDictionary" {
            (
                crate::companion::render_dict_support(
                    &name,
                    &root,
                    slots,
                    &owned_ivars,
                    ctx.program.heap_support,
                    ctx.pools.item_slots(),
                ),
                format!(
                    "struct {name} *{name}_oz_initWithKeysValues(void **keys, void **values, unsigned int count);\n",
                    name = name
                ),
            )
        } else {
            (
                crate::companion::render_alloc_free(
                    &name,
                    &root,
                    slots,
                    &owned_ivars,
                    ctx.program.heap_support,
                    ctx.program
                        .class_conforms_to(&name, crate::companion::SINGLETON_PROTOCOL),
                ),
                String::new(),
            )
        };
        (format!("{}{}\n{}{}{}", open_banner, struct_text, extra_proto, decls, close_banner), alloc_free)
    }
}

pub(crate) fn render_prototype(
    class_name: &str,
    m: &crate::model::MethodSig,
    root: Option<&str>,
) -> String {
    // Answered at the call site, never emitted as a function -- so a
    // prototype here would declare a symbol that is defined nowhere. That
    // is precisely how `+class` used to fail: declared by `OZObject.h`,
    // called as `OZObject_class_cls()`, defined by nothing, undefined at
    // link time (#226).
    if crate::staticbar::INTRINSIC_SELECTORS.contains(&m.selector.as_str()) {
        return String::new();
    }
    let mut params = String::new();
    if !m.is_class_method {
        params.push_str(&format!("struct {} *self", class_name));
    }
    for (pname, ptype) in &m.params {
        if !params.is_empty() {
            params.push_str(", ");
        }
        params.push_str(&render_param(ptype, pname, root));
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
            decls.push_str(&render_prototype(name, &sig, program.root_class()));
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
///
/// **The setter retains its argument, and #328 deliberately left that
/// alone.** A strong setter that *consumed* a `+1` argument instead would
/// be cheaper for `[self setFoo:[Foo new]];` -- no temporary, no release
/// -- but it changes this contract: every call site passing a *borrowed*
/// value would then owe a retain, and knowing which is which needs the
/// callee's identity, which a dynamically dispatched send does not have.
/// It also does nothing for a callee that only borrows. So the caller
/// releases its own reference after the send
/// (`render_owning_operand_statement`) and this stays retain-new,
/// release-old, which leaves the object at +1 held by the ivar.
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
        (sel, "void".to_string(), format!("struct {} *self, {}", class_name, render_param(c_type, &prop.name, program.root_class())))
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
        sig_params.push_str(&render_param(ptype, pname, ctx.program.root_class()));
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

    // Needed by `render_return_statement` to type the temporary a `return`
    // on the cleanup path evaluates into. The `function_definition` arm in
    // `walk_top_level` records the same thing for a free function --
    // separately, because nothing here is shared with that path (#336).
    ctx.method_return_type = ret_ty.clone();

    // Whether the body was really translated. A body the static bar rejected
    // is passed through as its original text and the whole transpile is going
    // to fail, so there is nothing to tidy and no output anyone will compile.
    let mut translated = body.is_none();
    let body_text = match body {
        Some(body) => {
            let class_info = ctx.program.classes[class_name].clone();
            let reject_diags = crate::staticbar::check_method_body(
                body, ctx.src, ctx.program, &class_info, &sig.params, &sig.selector,
            );
            if !reject_diags.is_empty() {
                ctx.diags.extend(reject_diags);
                node_text(body, ctx.src).to_string()
            } else {
                translated = true;
                collect_local_decls(body, ctx);
                render_body_with_comments(body, ctx)
            }
        }
        None => "{\n}".to_string(),
    };

    // `(void)x;` for each parameter the body never mentions -- see
    // `unused_param_acks`. Only for a body oz_static itself produced: a plain C
    // function's body is the author's own text, patched in place, and adding
    // acknowledgements to code someone wrote is not this pass's business.
    let body_text = if translated {
        let acks = unused_param_acks(&body_text, &sig.params, sig.is_class_method);
        let resume = match body {
            Some(body) => {
                ctx.lines.resume_after_brace(ctx.src, body.start_byte(), body.end_byte())
            }
            None => String::new(),
        };
        splice_after_open_brace(&body_text, &acks, &resume)
    } else {
        body_text
    };

    // One directive for the definition, between the signature comment and
    // the signature itself, so the function's own line is the line of the
    // `- (void)foo` that produced it (#305). Each statement of the body
    // then carries its own -- `render_body_with_comments` -- except a
    // verbatim body, which gets one anchor on its brace instead.
    let anchor = match body {
        Some(body) => body_anchor(body, &body_text, ctx),
        None => String::new(),
    };
    format!(
        "/* {} */\n{}{} {}({})\n{}{}\n",
        one_line(&header),
        ctx.lines.before(node.start_byte()),
        ret_ty,
        fn_name,
        sig_params,
        anchor,
        body_text
    )
}

pub struct EmitOutput {
    pub source_c: String,
    pub companion_h: String,
    pub companion_c: String,
    pub diagnostics: Vec<Diagnostic>,
}

/// Single-translation-unit assembler: the whole program as one `source_c`,
/// plus the shared companion pair. This is the form `transpile()` exposes
/// and the one the Rust suite drives.
///
/// It used to be a top-level walk of its own, which is how it managed to
/// disagree with the shipped one four times (#254). It is now the same
/// `walk_top_level` with a different assembly, so a node kind cannot be
/// handled there and not here.
///
/// One synthetic origin covers the whole text: at this level there are no
/// `#import`s resolved into the source and so no header/implementation
/// provenance to distinguish, which is why nothing is passed for
/// `header_ranges`. Every construct therefore lands under the same stem,
/// and the two buckets become an ordering rather than two files --
/// declarations first, bodies after, which is what C requires of a single
/// translation unit anyway.
pub fn emit(
    source: &str,
    program: &Program,
    pools: &crate::pools::PoolSizes,
    repaired_semicolons: &[usize],
    lines: &LineDirectives,
) -> EmitOutput {
    let origins = [("main".to_string(), 0..source.len())];
    let walked = walk_top_level(source, program, pools, &origins, &[], repaired_semicolons, lines);

    // One stem in practice, but driven off `stem_order` rather than the
    // maps' own iteration order, which a `HashMap` does not promise.
    let per_stem = |m: &HashMap<String, Vec<(String, String)>>| -> Vec<(String, String)> {
        walked.stem_order.iter().filter_map(|s| m.get(s)).flatten().cloned().collect()
    };
    let statics = per_stem(&walked.hoisted_statics_by_stem);
    let blocks = per_stem(&walked.hoisted_blocks_by_stem);
    let strings = per_stem(&walked.hoisted_strings_by_stem);

    let mut out = String::from(
        "/* Auto-generated by oz_static -- do not edit */\n#include \"oz_static_dispatch.h\"\n\n",
    );

    /* Attribution of everything synthesized here belongs to this file, not
     * to whatever `.m` line the section before it ended on -- see
     * `LineDirectives::reset`. `emit()` has one synthetic origin ("main"),
     * so that is the stem every section of its single output belongs to. */
    let reset = |out: &mut String| lines.reset(out, "main", "c");

    // A promoted `__block` local is a self-contained
    // `static TYPE name [= init];` line, so unlike the blocks and literals
    // below it needs no prototype/definition split: it only has to precede
    // every reference to it, which living up here guarantees.
    if !statics.is_empty() {
        reset(&mut out);
        out.push_str("/* __block-qualified locals, promoted to file scope */\n");
        for (_, decl) in &statics {
            out.push_str(decl);
            out.push('\n');
        }
        out.push('\n');
    }

    // Prototypes ahead of every call site, definitions once at the very
    // end. A hoisted block or boxed literal can be used by a class that
    // appears earlier in the text than the type its own definition needs
    // (`struct OZString` is defined at OZString's `@interface`), so the
    // definition cannot go where the prototype does.
    if !blocks.is_empty() {
        reset(&mut out);
        out.push_str("/* non-capturing blocks, hoisted from block literals -- prototypes (defined below, after every class) */\n");
        for (prototype, _) in &blocks {
            out.push_str(prototype);
        }
        out.push('\n');
    }
    if !strings.is_empty() {
        reset(&mut out);
        out.push_str("/* boxed string literals, hoisted -- extern forward declarations (defined below, after every class) */\n");
        for (prototype, _) in &strings {
            out.push_str(prototype);
        }
        out.push('\n');
    }

    /* Each section is preceded by a reset rather than the whole run of
     * them, because a section can *end* inside a method body -- and so on
     * a `.m` line -- and the next one is generated code again. With
     * directives off `reset` writes nothing and this is the `join` it
     * replaced, byte for byte. */
    for stem in &walked.stem_order {
        if let Some(sections) = walked.headers.get(stem) {
            for (i, section) in sections.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                reset(&mut out);
                out.push_str(section);
            }
            out.push('\n');
        }
    }
    for stem in &walked.stem_order {
        if let Some(sections) = walked.bodies.get(stem) {
            for (i, section) in sections.iter().enumerate() {
                if i > 0 {
                    out.push_str("\n\n");
                }
                reset(&mut out);
                out.push_str(section);
            }
            out.push('\n');
        }
    }

    if !blocks.is_empty() {
        out.push('\n');
        reset(&mut out);
        out.push_str("/* non-capturing blocks, hoisted from block literals */\n");
        for (_, definition) in &blocks {
            out.push_str(definition);
            out.push('\n');
        }
    }
    if !strings.is_empty() {
        out.push('\n');
        reset(&mut out);
        out.push_str("/* boxed string literals, hoisted -- static struct OZString instances */\n");
        for (_, definition) in &strings {
            out.push_str(definition);
        }
    }

    let (companion_h, companion_c) = crate::companion::render(
        program,
        &walked.hoisted_structs,
        &walked.hoisted_enums,
        &walked.hoisted_forward_decls,
        &walked.hoisted_c_structs,
        pools,
        &crate::imports::collect_system_includes(source),
        &walked.introspection_used,
    );

    EmitOutput { source_c: out, companion_h, companion_c, diagnostics: walked.diags }
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

/// Everything the top-level walk produces, before either assembler has
/// decided where to put it.
///
/// Each construct is bucketed by which origin file it came from and by
/// whether it is interface-shaped (struct + prototypes, no bodies --
/// exactly what `class_interface` renders as) or implementation-shaped
/// (method bodies -- exactly what `class_implementation` renders as). What
/// an assembler then does with a bucket is a placement decision: one file
/// per origin (`emit_split`) or one translation unit (`emit`).
struct TopLevel {
    /// Origin stems in first-seen (textual) order.
    stem_order: Vec<String>,
    headers: HashMap<String, Vec<String>>,
    bodies: HashMap<String, Vec<String>>,
    /// Stems whose generated `.h` must include another stem's `.h`, because
    /// a class there embeds a non-root superclass by value.
    extra_includes: HashMap<String, HashSet<String>>,
    hoisted_blocks_by_stem: HashMap<String, Vec<(String, String)>>,
    hoisted_strings_by_stem: HashMap<String, Vec<(String, String)>>,
    hoisted_statics_by_stem: HashMap<String, Vec<(String, String)>>,
    /// Destined for the shared companion header rather than any one
    /// origin's -- see `companion::render`.
    hoisted_structs: Vec<(String, String)>,
    hoisted_enums: Vec<String>,
    hoisted_forward_decls: Vec<String>,
    hoisted_c_structs: Vec<String>,
    /// Which origin owns each class's declaration.
    class_to_stem: HashMap<String, String>,
    /// See `IntrospectionUse`.
    introspection_used: IntrospectionUse,
    diags: Vec<Diagnostic>,
}

/// The one walk over the top-level nodes, and so the one place a node kind
/// is handled (#254).
///
/// There used to be two: this one and a second inside `emit()`, each with
/// its own match on `node.kind()`. They disagreed about what valid output
/// looks like four separate times -- gap R (#240), #246, #250 and #251 --
/// and none of those was a forgotten case so much as two places answering
/// the same question (*is this a local? is this an object declaration? does
/// this type need a tag?*) with nothing forcing them to answer alike. The
/// asymmetry bit in both directions, so neither walk was simply the more
/// complete one: #246 was `emit()` missing a `declaration` arm outright,
/// while gap C's seventh cause was the split walk *dropping* a top-level
/// struct that `emit()` kept by not touching it.
///
/// Both entry points now call this, and differ only in how they assemble
/// what it returns. Adding a node kind here reaches both by construction,
/// which is the property the four fixes above each restored by hand.
///
/// `origins` is `imports::ResolvedSource::origins`: an ordered list of
/// `(stem, byte_range)` covering every byte of `source` (the same stem
/// may appear more than once, non-contiguously). `emit()` passes a single
/// synthetic origin covering the whole text.
/// Replace whole-identifier occurrences of `from` with `to`.
///
/// Not `str::replace`: one hoisted name can be a prefix of another --
/// `_oz_str_L1_C1_1` sits inside `_oz_str_L1_C1_11` -- so a substring
/// rewrite would corrupt the longer symbol while renaming the shorter.
fn replace_ident(text: &str, from: &str, to: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(rel) = text[at..].find(from) {
        let start = at + rel;
        let end = start + from.len();
        let boundary = |b: u8| !(b.is_ascii_alphanumeric() || b == b'_');
        let ok = (start == 0 || boundary(bytes[start - 1]))
            && (end >= bytes.len() || boundary(bytes[end]));
        out.push_str(&text[at..start]);
        out.push_str(if ok { to } else { from });
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

/// Collapse identical boxed string literals within each origin, so one
/// `struct OZString` instance serves every occurrence of the same string
/// in that translation unit (#372).
///
/// **Why this is a correctness change and not an optimisation.**
/// Objective-C guarantees that identical literals in a translation unit
/// are the same object, and `-isEqual:` opens with
/// `if (self == anObject) { return YES; }` (`src/OZString.m`). With one
/// instance per *occurrence* that branch missed for two spellings of the
/// same string, so the transpiler diverged from the language on a point a
/// program can reasonably rely on. The bytes are a footnote: measured
/// across every sample in the tree it is 3 instances, 72 bytes of
/// `.rodata` -- and px-keyboard, the only real application, contains no
/// boxed literal at all.
///
/// **Scope is the origin, deliberately.** That is exactly what the
/// language promises, and it is all that is available: the instances live
/// in per-origin generated files with external linkage, so sharing one
/// across origins would need an owning file plus `extern` references from
/// the others. It would also buy nothing measurable -- a cross-origin
/// duplicate can only occur inside one program built from several `.m`
/// files, and the only such program in the tree has no literals. Every
/// sample and every corpus case is a single-file program.
///
/// **Why the rename reaches four buckets and not just the bodies.** A
/// literal's symbol is referenced wherever the expression that produced
/// it was emitted, and that is not only a method body: a block hoisted
/// out of a method carries its own text, a `__block` static can be
/// initialised from one, and a `static inline` helper in the generated
/// header can contain one too. Renaming only `bodies` would leave a
/// dangling reference to a definition this function just dropped.
fn dedup_string_literals(
    strings: &mut HashMap<String, Vec<(String, String)>>,
    bodies: &mut HashMap<String, Vec<String>>,
    headers: &mut HashMap<String, Vec<String>>,
    blocks: &mut HashMap<String, Vec<(String, String)>>,
    statics: &mut HashMap<String, Vec<(String, String)>>,
) {
    for (stem, lits) in strings.iter_mut() {
        // Key on the definition with its own symbol name removed: two
        // literals of the same string differ in nothing else, `._length`
        // and `._data` both being derived from the content.
        let mut kept: HashMap<String, String> = HashMap::new();
        let mut renames: Vec<(String, String)> = Vec::new();
        let mut survivors: Vec<(String, String)> = Vec::new();
        for (prototype, definition) in lits.iter() {
            let Some(name) = literal_symbol(prototype) else {
                survivors.push((prototype.clone(), definition.clone()));
                continue;
            };
            let key = replace_ident(definition, &name, "@");
            match kept.get(&key) {
                Some(first) => renames.push((name, first.clone())),
                None => {
                    kept.insert(key, name);
                    survivors.push((prototype.clone(), definition.clone()));
                }
            }
        }
        if renames.is_empty() {
            continue;
        }
        *lits = survivors;
        let apply = |text: &str| {
            let mut t = text.to_string();
            for (from, to) in &renames {
                t = replace_ident(&t, from, to);
            }
            t
        };
        if let Some(v) = bodies.get_mut(stem) {
            for b in v.iter_mut() {
                *b = apply(b);
            }
        }
        if let Some(v) = headers.get_mut(stem) {
            for b in v.iter_mut() {
                *b = apply(b);
            }
        }
        for map in [&mut *blocks, &mut *statics] {
            if let Some(v) = map.get_mut(stem) {
                for (a, b) in v.iter_mut() {
                    *a = apply(a);
                    *b = apply(b);
                }
            }
        }
    }
}

/// The symbol a hoisted literal's forward declaration names, or `None` if
/// the declaration is not the shape this module emits.
fn literal_symbol(prototype: &str) -> Option<String> {
    let rest = prototype.trim().strip_prefix("extern const struct OZString ")?;
    Some(rest.strip_suffix(';')?.trim().to_string())
}

fn walk_top_level<'a>(
    source: &'a str,
    program: &'a Program,
    pools: &'a crate::pools::PoolSizes,
    origins: &[(String, Range<usize>)],
    header_ranges: &[Range<usize>],
    repaired_semicolons: &[usize],
    lines: &'a LineDirectives<'a>,
) -> TopLevel {
    let tree = crate::parse::parse(source);
    let root = tree.root_node();
    let file_vars = file_scope_vars(root, source, program);

    // Did this byte come from a header? See
    // `imports::ResolvedSource::header_ranges` for why it matters.
    let from_header = |byte: usize| -> bool {
        header_ranges.iter().any(|r| r.contains(&byte))
    };
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
    let mut introspection_used = IntrospectionUse::default();
    let mut hoisted_structs: Vec<(String, String)> = Vec::new();
    let mut hoisted_enums: Vec<String> = Vec::new();
    let mut hoisted_forward_decls: Vec<String> = Vec::new();
    let mut hoisted_c_structs: Vec<String> = Vec::new();

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
                let mut ctx = EmitCtx::new(source, program, name.clone(), scope, pools, lines);
                let (header_part, alloc_free_part) = render_interface(node, &mut ctx, program);
                diags.extend(ctx.diags);
                introspection_used.merge(ctx.introspection_used);
                hoisted_structs.extend(ctx.hoisted_structs);
                headers.entry(stem.clone()).or_default().push(header_part);
                if !alloc_free_part.is_empty() {
                    bodies.entry(stem.clone()).or_default().push(alloc_free_part);
                }
            }
            "class_implementation" => {
                let (name, _, category) = crate::collect::class_header(node, source);
                let is_category_impl = category.is_some();
                let mut ivars_scope = base_scope(&name, program);
        // File-scope statics are visible inside every method too, and an
        // ivar of the same name shadows one, so these go in first.
        for (var, ty) in &file_vars {
            ivars_scope.entry(var.clone()).or_insert_with(|| ty.clone());
        }
                let mut ctx =
                    EmitCtx::new(source, program, name.clone(), ivars_scope.clone(), pools, lines);
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
                introspection_used.merge(ctx.introspection_used);
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
            "struct_specifier" | "union_specifier" => {
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
                } else {
                    // A full `struct Tag { ... };` definition written in
                    // plain C in one of the spliced sources. Output is built
                    // only from what each arm pushes, so until this arm
                    // existed such a definition was dropped outright --
                    // `samples/hello_category`'s `struct color` came out as
                    // nothing but its trailing `;`, and every use of it
                    // failed with "variable has incomplete type 'struct
                    // color'". `emit()` had concealed that by patching the
                    // original text, where anything no arm claimed survived
                    // untouched; since #254 it shares this walk and so
                    // shares this arm.
                    //
                    // It goes to the companion header rather than this
                    // origin's own `.h` because that is the header every
                    // generated file includes, and the type is needed in
                    // more than one of them: the companion's own prototypes
                    // name it (`struct color* Car_color(struct Car *)`),
                    // and another origin's code can build a value of it
                    // (that sample's `main` writes
                    // `&(struct color){255, 255, 0}`).
                    //
                    // Unions share this arm and this one list, so that
                    // source order survives: a struct may have a union
                    // field by value, or the reverse, and the source had
                    // to declare them in a working order already.
                    /* Patched, not copied. This pushed `node_text` --
                     * the author's bytes -- so a class-typed field arrived
                     * in the companion header with no `struct` tag and the
                     * build failed with `unknown type name 'Thing'`, from a
                     * declaration needing no store and no read to break
                     * (#367). Every other position that carries a type
                     * through to the output tags it; this one did not, which
                     * is the same methods-vs-free-functions asymmetry as
                     * #326 and #336 in a third place.
                     *
                     * `id` is lowered here too, for the same reason: a field
                     * typed `id` or `id<Proto>` is as unrepresentable in C
                     * as an untagged class name. */
                    let mut field_edits = class_tag_edits(node, source, program);
                    if let Some(root) = program.root_class() {
                        rewrite_id_types(
                            node,
                            source,
                            0,
                            &format!("struct {} *", root),
                            &mut field_edits,
                        );
                    }
                    hoisted_c_structs.push(apply_edits(
                        source,
                        node.start_byte(),
                        node.end_byte(),
                        &field_edits,
                    ));
                    headers.entry(stem.clone()).or_default().push(format!(
                        "/* {} definition hoisted to the companion header -- named by generated prototypes there, and by other origins' code */",
                        if node.kind() == "union_specifier" { "union" } else { "struct" }
                    ));
                }
            }
            "function_definition" => {
                // No self or ivars here, but a file-scope object variable is
                // in scope for a top-level function just as much as for a
                // method -- `samples/gpio_demo`'s `[led toggle]` sits in
                // `main()`.
                let mut ctx =
                    EmitCtx::new(source, program, String::new(), file_vars.clone(), pools, lines);
                // The free-function twin of the line in
                // `render_method_definition` that records a method's return
                // type: `render_return_statement` needs it to type the
                // temporary it synthesizes when a `return` has cleanups to
                // run after the value is evaluated. Without it the field
                // kept `EmitCtx::new`'s placeholder and every such
                // temporary was an `int` (#336). Left at the placeholder
                // only if the declarator names no type at all, which no
                // parsed `function_definition` does.
                let known: std::collections::HashSet<String> =
                    program.classes.keys().cloned().collect();
                if let Some(ret_ty) =
                    crate::collect::function_return_type(node, source, &known)
                {
                    ctx.method_return_type = ret_ty;
                }
                let mut sig_edits = class_tag_edits(node, source, program);
                // A block-typed parameter is lowered to a function pointer
                // here for the same reason its class names are tagged here:
                // this signature is patched text, not rebuilt through
                // `collect::render_type`, so nothing else lowers it and the
                // `^` reached GCC (#272). A method's equivalent parameter
                // has always been lowered.
                sig_edits.extend(block_pointer_edits(node, source, program.root_class()));
                // And `id` -- bare or protocol-qualified -- lowered the way
                // a method's parameter and a block literal's already are.
                // Missing here, `void f(id<Marker> m)` was copied through
                // verbatim and GCC answered `expected ')'`, while the same
                // parameter on a method lowered to `void *` (#367). Third
                // instance of this asymmetry in the same signature: the
                // class tags above were #326's, the block pointers #272's,
                // and each was a lowering methods had and free functions
                // did not.
                if let Some(root) = program.root_class() {
                    rewrite_id_types(node, source, 0, &format!("struct {} *", root), &mut sig_edits);
                }
                /* `block_pointer_edits` already lowers an `id` *inside* a
                 * block-typed parameter, so `void (^cb)(id)` now has two
                 * lowerings reaching the same bytes with the same
                 * replacement. `apply_edits` refuses overlaps -- rightly,
                 * that is how a real conflict is caught -- but two edits
                 * that agree byte for byte are idempotent, not conflicting.
                 * Dropped here rather than by relaxing `apply_edits`, so
                 * the assertion keeps its teeth everywhere else. */
                sig_edits.sort_by_key(|(range, _)| (range.start, range.end));
                sig_edits.dedup();
                // A plain C function gets the same leading directive a
                // method's definition does, on the same reasoning and
                // needing it just as much: `main()` is `main.m`'s own
                // `int main(void)` and is exactly where someone types
                // `break main.m:73` (#305). The Proposal named only
                // `render_method_definition`, which was an omission --
                // nothing here is method-specific.
                //
                // A signature is *patched* text, not rebuilt, so it has the
                // author's own line count and one directive at its start
                // lines up every line of it -- and every line of a
                // verbatim body after it, which is why the untranslated
                // paths below need nothing more.
                let signature = lines.before(node.start_byte());
                let mut text = format!(
                    "{}{}",
                    signature,
                    apply_edits(source, node.start_byte(), node.end_byte(), &sig_edits)
                );
                let mut c2 = node.walk();
                if let Some(body) = node.children(&mut c2).find(|c| c.kind() == "compound_statement") {
                    // The signature is tagged either way; the body is
                    // rendered by the ordinary machinery, which already
                    // resolves types properly.
                    let prefix = format!(
                        "{}{}",
                        signature,
                        apply_edits(source, node.start_byte(), body.start_byte(), &sig_edits)
                    );
                    if needs_translation(body) {
                        // Same scan as the single-file arm above; both
                        // `function_definition` paths need it, and an earlier
                        // shape of this change had it in only one.
                        let reject_diags =
                            crate::staticbar::check_function_body(body, source, program);
                        if !reject_diags.is_empty() {
                            ctx.diags.extend(reject_diags);
                            text = format!("{}{}", prefix, node_text(body, source));
                        } else {
                            // Parameters first, so a body declaration of the
                            // same name shadows the parameter rather than the
                            // other way round.
                            collect_function_params(node, &mut ctx);
                            collect_local_decls(body, &mut ctx);
                            let rendered_body = render_body_with_comments(body, &mut ctx);
                            text = format!("{}{}", prefix, rendered_body);
                        }
                    } else {
                        text = format!("{}{}", prefix, node_text(body, source));
                    }
                }
                diags.extend(ctx.diags);
                introspection_used.merge(ctx.introspection_used);
                hoisted_structs.extend(ctx.hoisted_structs);
                hoisted_blocks_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_blocks);
                hoisted_strings_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_string_literals);
                hoisted_statics_by_stem.entry(stem.clone()).or_default().extend(ctx.hoisted_statics);
                // A `static inline` helper goes to this origin's own
                // header, for the same reason the passthrough arm below
                // already puts macros there: in the single-file design any
                // top-level definition was visible to everything after it
                // merely by sitting in the same text, and once split into
                // real files only that origin's `.h` gives it the same
                // reach. `tests/behavior/cases/regression/
                // issue_090_header_preservation.m` is the case -- its
                // header's `static inline int sensor_scale(int, int)` has
                // to be callable from outside the file it was written in,
                // which is the whole point of the test.
                //
                // `static inline` and nothing else: it is the one form
                // meant to be duplicated per translation unit. A plain
                // `static` function copied into a header would draw
                // "defined but not used" in every file that includes it
                // and break outright if it touched a file-scope static
                // that stayed behind in the body, and a non-static one
                // would be a duplicate symbol at link time.
                if from_header(node.start_byte()) || is_static_inline(node, source) {
                    headers.entry(stem.clone()).or_default().push(text);
                } else {
                    bodies.entry(stem.clone()).or_default().push(text);
                }
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
                // A plain top-level declaration still needs its class
                // names tagged -- `static OZHeap *sHeap;` is not valid C
                // (see `class_tag_edits`).
                //
                // Two more edits apply to anything that lands here, both
                // #272 and both about blocks reaching C with their `^`
                // intact: a block-pointer declarator is lowered to a
                // function pointer (`block_pointer_edits` -- a file-scope
                // `static void (^g)(int);` is the shape), and a block
                // literal is hoisted to a named function and replaced by
                // that name (`top_level_block_edits` -- which is what makes
                // `ZBUS_LISTENER_DEFINE(n, ^(...){ ... })` compile).
                //
                // All three feed one `apply_edits` call, so their ranges
                // have to be disjoint: the literal belongs to
                // `top_level_block_edits` alone, and the other two skip it
                // (#331). `apply_edits` asserts that rather than trusting
                // it.
                //
                // Everything else here is trivia and passes through
                // untouched: with no edits, `apply_edits` returns the
                // original text byte for byte.
                // A `#define` body is one opaque `preproc_arg` token, so no
                // arm above descends into it and no edit below can reach it:
                // Objective-C written there is emitted verbatim and fails in
                // the C compiler, naming generated code the user never wrote.
                // Rejected here rather than transpiled -- see
                // `staticbar::check_macro_body` (#238).
                if node.kind() == "preproc_function_def" || node.kind() == "preproc_def" {
                    diags.extend(crate::staticbar::check_macro_body(node, source));
                }

                let mut edits = if node.kind() == "declaration" {
                    class_tag_edits(node, source, program)
                } else {
                    Vec::new()
                };
                // The `;` `parse::repair_bare_macro_statements` wrote over a
                // whitespace byte was for tree-sitter's benefit only. Put
                // the space back, or a macro that terminates its own
                // expansion -- `ZBUS_OBS_DECLARE` -- gets a second `;` and
                // a stray empty declaration at file scope (#288).
                edits.extend(
                    repaired_semicolons
                        .iter()
                        .filter(|o| (node.start_byte()..node.end_byte()).contains(o))
                        .map(|o| (*o..*o + 1, " ".to_string())),
                );
                edits.extend(block_pointer_edits(node, source, program.root_class()));
                if contains_block_literal(node) {
                    let mut ctx =
                        EmitCtx::new(source, program, String::new(), file_vars.clone(), pools, lines);
                    edits.extend(top_level_block_edits(node, &mut ctx, program));
                    diags.extend(ctx.diags);
                    introspection_used.merge(ctx.introspection_used);
                    hoisted_structs.extend(ctx.hoisted_structs);
                    hoisted_blocks_by_stem
                        .entry(stem.clone())
                        .or_default()
                        .extend(ctx.hoisted_blocks);
                    hoisted_strings_by_stem
                        .entry(stem.clone())
                        .or_default()
                        .extend(ctx.hoisted_string_literals);
                    hoisted_statics_by_stem
                        .entry(stem.clone())
                        .or_default()
                        .extend(ctx.hoisted_statics);
                }
                let owned_text =
                    apply_edits(source, node.start_byte(), node.end_byte(), &edits);
                let text = owned_text.trim();
                if text.is_empty() {
                    continue;
                }
                // A lone `;` is dropped rather than copied. Several arms
                // above consume a specifier node whose grammar span stops
                // short of the trailing semicolon -- `@compatibility_alias
                // NSObject OZObject;` in `include/oz_sdk/Foundation/
                // OZObject.h` is the one that reaches every generated
                // program -- so the semicolon arrives here as a top-level
                // node of its own, and passing it through left a bare `;`
                // at file scope in 51 of the samples' generated files and
                // 146 of the corpus's.
                //
                // An empty declaration at file scope is not valid ISO C. For
                // the life of this backend it failed no build, because
                // diagnosing it needs `-Wpedantic`, which Zephyr does not
                // pass and neither did the `-Wall -Wextra` sweep behind
                // gap S -- it was found by diffing generated bytes. Since
                // #266 it *does* fail a build: `corpus_parity.rs` compiles
                // every case with `-std=c17 -pedantic-errors`.
                //
                // Handled here rather than in each arm that leaves one: this
                // is the one place every unclaimed node passes through, so a
                // new arm gets the same treatment without knowing to ask for
                // it, and nothing meaningful is lost -- a top-level `;`
                // carries no information in any C dialect.
                if text == ";" {
                    continue;
                }
                // Provenance first: anything a *header* contributed belongs in
                // the generated header, because that is what a header is for
                // -- every file including it should see it. A bare top-level
                // macro invocation is the shape that forced this
                // (`ZBUS_CHAN_DECLARE` in `samples/zbus_service`'s header,
                // which is neither a `preproc` node nor a declaration, so it
                // fell to the body and no other origin could see it).
                //
                // The `preproc` test stays as a fallback for a macro defined
                // in an implementation file, which the single-file design
                // made implicitly visible to everything after it.
                if from_header(node.start_byte()) || node.kind().starts_with("preproc") {
                    headers.entry(stem.clone()).or_default().push(text.to_string());
                } else {
                    bodies.entry(stem.clone()).or_default().push(text.to_string());
                }
            }
        }
    }

    // One instance per string per origin, not per occurrence (#372).
    // Placed here, at the end of the shared walk, so both assemblers --
    // `emit()`'s single-file path and the origin-aware one -- get already
    // collapsed literals and already-renamed text, with no dedup logic of
    // their own to keep in step.
    dedup_string_literals(
        &mut hoisted_strings_by_stem,
        &mut bodies,
        &mut headers,
        &mut hoisted_blocks_by_stem,
        &mut hoisted_statics_by_stem,
    );

    TopLevel {
        stem_order,
        headers,
        bodies,
        extra_includes,
        hoisted_blocks_by_stem,
        hoisted_strings_by_stem,
        hoisted_statics_by_stem,
        hoisted_structs,
        hoisted_enums,
        hoisted_forward_decls,
        hoisted_c_structs,
        class_to_stem,
        introspection_used,
        diags,
    }
}

/// Origin-aware assembler (OZ-096): one `.h`/`.c` pair per origin file,
/// which is what the CLI -- and therefore every real build -- emits.
///
/// The walk is shared with `emit()`; everything here is placement. What
/// makes the two differ at all is that a split program has real
/// translation-unit boundaries, so anything one origin declares and
/// another uses needs an explicit `#include` where the single-file design
/// got the same reach from textual order alone.
pub fn emit_split(
    source: &str,
    program: &Program,
    origins: &[(String, Range<usize>)],
    pools: &crate::pools::PoolSizes,
    header_ranges: &[Range<usize>],
    repaired_semicolons: &[usize],
    lines: &LineDirectives,
) -> EmitSplitOutput {
    let TopLevel {
        stem_order,
        headers,
        bodies,
        extra_includes,
        hoisted_blocks_by_stem,
        hoisted_strings_by_stem,
        hoisted_statics_by_stem,
        hoisted_structs,
        hoisted_enums,
        hoisted_forward_decls,
        hoisted_c_structs,
        class_to_stem,
        introspection_used,
        diags,
    } = walk_top_level(source, program, pools, origins, header_ranges, repaired_semicolons, lines);

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
    //
    // OZString is in the list for the same reason, one step further: a
    // `@"..."` literal emits a *definition* of a `struct OZString` into
    // whichever file used it (see `render_boxed_string_literal`), and
    // defining a variable needs the complete type, not just a
    // declaration. Without this the file gets `error: variable has
    // incomplete type 'struct OZString'` -- which is exactly what five of
    // the cases under tests/behavior/cases/ hit.
    // Carried as (class, stem) rather than just the stem, because whether
    // an edge is safe depends on where the *class* sits in the hierarchy
    // -- see the ancestry check below.
    let mut always_visible: Vec<(String, String)> = Vec::new();
    if let Some(root) = program.root_class() {
        if let Some(stem) = class_to_stem.get(root) {
            always_visible.push((root.to_string(), stem.clone()));
        }
    }
    for helper_class in ["OZArray", "OZDictionary", "OZString"] {
        if let Some(stem) = class_to_stem.get(helper_class) {
            always_visible.push((helper_class.to_string(), stem.clone()));
        }
    }
    // These go into each `.c`, never into a `.h`. They exist so *code* can
    // reach the root class's macros and the boxed-literal helpers, and code
    // lives in the body file. Putting them in headers caused two distinct
    // failures: `main.h` declares nothing, so an earlier attempt to skip
    // declaration-free headers left `main.c` unable to see
    // `OZArray_oz_initWithItems`; and the generated `assert.h` (a shim whose
    // only purpose is keeping `oz_assert` calls in Clang's AST) sits on the
    // include path where the PAL's own `#include <assert.h>` finds it, so
    // pulling the class graph in there re-entered the class headers from
    // inside the companion header, before the root struct existed. A body
    // file is reached by neither path.
    let mut body_includes: HashMap<String, std::collections::BTreeSet<String>> = HashMap::new();
    for (class, target_stem) in &always_visible {
        for stem in &stem_order {
            if stem == target_stem {
                continue;
            }
            // Never point a stem at a *descendant* of a class it owns. A
            // subclass's struct embeds its superclass's by value, so the
            // subclass header must include the superclass header -- and
            // the reverse edge closes a cycle that `#pragma once` then
            // breaks by leaving one of the two structs incomplete
            // (`field has incomplete type 'struct OZObject'`), depending
            // only on which header the compiler happened to enter first.
            // Every class here is a descendant of the root, so without
            // this the root's own header would include all of them.
            let owns_ancestor = program.class_order.iter().any(|owned| {
                class_to_stem.get(owned).is_some_and(|s| s == stem)
                    && program.is_descendant_of(class, owned)
            });
            if owns_ancestor {
                continue;
            }
            body_includes.entry(stem.clone()).or_default().insert(target_stem.clone());
        }
    }

    // A stem that names a class living in another stem needs that stem's
    // header, or the class's struct is incomplete wherever it is used.
    // `samples/hello_category` splits `Car` (its own header, its own
    // origin) from the `main` that does `myCar->_plate = 0xAABBCC`, and
    // without this edge that line is "incomplete definition of type
    // 'struct Car'" -- the companion header carries every class's method
    // prototypes, but only a forward declaration of any non-root struct.
    //
    // Textual mention of the class name is the test. It over-approximates
    // (a comment or an unrelated identifier of the same name counts), but
    // an unnecessary `#include` of a `#pragma once` header costs nothing,
    // while a missing one is a compile error -- so erring towards including
    // is the safe direction. These are body includes, so no header cycle
    // can come of it.
    let mut stem_text: HashMap<&str, String> = HashMap::new();
    for (stem, range) in origins {
        stem_text.entry(stem.as_str()).or_default().push_str(&source[range.clone()]);
    }
    for stem in &stem_order {
        let Some(text) = stem_text.get(stem.as_str()) else {
            continue;
        };
        for (class, owner_stem) in &class_to_stem {
            if owner_stem == stem {
                continue;
            }
            if mentions_identifier(text, class) {
                body_includes.entry(stem.clone()).or_default().insert(owner_stem.clone());
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
        /* A `#line` handing attribution back to this generated file, ahead
         * of every section that is generated code rather than the author's
         * -- and ahead of *each* section, not just the run of them: a
         * section can end inside a method body, on a `.m` line, and
         * without this the next one would inherit it. With directives off
         * these write nothing and the assembly is the `join` it replaced,
         * byte for byte. Both files need it: a `static inline` helper
         * written in a header is rendered with directives too, into the
         * generated `.h`. */
        let reset_h = |out: &mut String| lines.reset(out, stem, "h");
        let reset_c = |out: &mut String| lines.reset(out, stem, "c");
        if let Some(sections) = headers.get(stem) {
            for (i, section) in sections.iter().enumerate() {
                if i > 0 {
                    h.push('\n');
                }
                reset_h(&mut h);
                h.push_str(section);
            }
            h.push('\n');
        }

        let mut c = format!(
            "/* Auto-generated by oz_static -- do not edit */\n#include \"oz_static_dispatch.h\"\n#include \"{}.h\"\n",
            stem
        );
        if let Some(deps) = body_includes.get(stem) {
            for dep in deps {
                c.push_str(&format!("#include \"{}.h\"\n", dep));
            }
        }
        c.push('\n');
        if let Some(statics) = hoisted_statics_by_stem.get(stem) {
            if !statics.is_empty() {
                reset_c(&mut c);
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
                reset_c(&mut c);
                c.push_str("/* non-capturing blocks, hoisted from block literals -- prototypes (defined below) */\n");
                for (prototype, _) in blocks {
                    c.push_str(prototype);
                }
                c.push('\n');
            }
        }
        if let Some(strs) = hoisted_strings_by_stem.get(stem) {
            if !strs.is_empty() {
                reset_c(&mut c);
                c.push_str("/* boxed string literals, hoisted -- extern forward declarations (defined below) */\n");
                for (prototype, _) in strs {
                    c.push_str(prototype);
                }
                c.push('\n');
            }
        }
        if let Some(sections) = bodies.get(stem) {
            for (i, section) in sections.iter().enumerate() {
                if i > 0 {
                    c.push_str("\n\n");
                }
                reset_c(&mut c);
                c.push_str(section);
            }
            c.push('\n');
        }
        if let Some(blocks) = hoisted_blocks_by_stem.get(stem) {
            if !blocks.is_empty() {
                c.push('\n');
                reset_c(&mut c);
                c.push_str("/* non-capturing blocks, hoisted from block literals */\n");
                for (_, definition) in blocks {
                    c.push_str(definition);
                    c.push('\n');
                }
            }
        }
        if let Some(strs) = hoisted_strings_by_stem.get(stem) {
            if !strs.is_empty() {
                c.push('\n');
                reset_c(&mut c);
                c.push_str("/* boxed string literals, hoisted -- static struct OZString instances */\n");
                for (_, definition) in strs {
                    c.push_str(definition);
                }
            }
        }

        files.push((stem.clone(), h, c));
    }

    let (companion_h, companion_c) =
        crate::companion::render(
            program,
            &hoisted_structs,
            &hoisted_enums,
            &hoisted_forward_decls,
            &hoisted_c_structs,
            pools,
            &crate::imports::collect_system_includes(source),
            &introspection_used,
        );

    EmitSplitOutput { files, companion_h, companion_c, diagnostics: diags }
}

/// Byte-range edits that give every bare class name in `node` its `struct`
/// tag, as absolute offsets into the source.
///
/// A class generates `struct Name`, never a typedef, so any type position
/// that keeps the ObjC spelling is invalid C: `error: must use 'struct' tag
/// to refer to type 'Sensor'`. Method signatures, ivars, locals and casts
/// all route through `collect::render_type` already; the positions that do
/// not are the ones copied through verbatim -- a plain top-level declaration
/// (`samples/heap_alloc`'s `static OZHeap *sHeap;`), a free function's own
/// signature (`samples/arc_demo`'s `static Sensor *createSensor(int v)`),
/// both #246, and the parameter list `render_block` patches into a hoisted
/// block literal's signature (#326).
///
/// A name already under a `struct_specifier` is skipped, so an
/// already-tagged `struct OZHeap *` is left alone rather than becoming
/// `struct struct OZHeap *`.
///
/// A `block_literal` is skipped too, by the disjointness rule `apply_edits`
/// states: `top_level_block_edits` replaces the literal's whole byte range
/// with the hoisted function's name, so an edit inside it can only collide
/// (#331). Nothing is lost by not descending -- `render_block` renders that
/// subtree, and patches the hoisted signature's own parameter list through
/// this same helper (#326).
fn class_tag_edits(node: Node, src: &str, program: &Program) -> Vec<(Range<usize>, String)> {
    fn walk(
        node: Node,
        src: &str,
        program: &Program,
        out: &mut Vec<(Range<usize>, String)>,
    ) {
        // A `generic_specifier`'s arguments are erased rather than tagged.
        if node.kind() == "generic_specifier" {
            return;
        }
        // A `struct_specifier` used as a *type* already carries its tag --
        // `struct Widget *w` needs nothing -- but one that *defines* a
        // struct has a body, and the field types inside it are ordinary
        // type positions that need tagging like any other. This used to
        // return for both, on the claim that "inside a struct_specifier the
        // tag is already present": true of the reference, false of the
        // definition, so
        //
        //     struct box { Thing *held; };
        //
        // reached the companion header verbatim and the build failed with
        // `unknown type name 'Thing'` -- accepted by the transpiler,
        // rejected by the compiler, from a declaration that needs no store
        // and no read to break (#367).
        //
        // The tag itself is still skipped: only the body is descended into,
        // so `struct box` does not become `struct struct box`.
        if node.kind() == "struct_specifier" {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            for child in children {
                if child.kind() == "field_declaration_list" {
                    walk(child, src, program, out);
                }
            }
            return;
        }
        // A `block_literal` is skipped for the same reason
        // `block_pointer_edits` skips one: another pass replaces the whole
        // literal, so an edit inside it can only collide (#331).
        if node.kind() == "block_literal" {
            return;
        }
        if node.kind() == "type_identifier" {
            let name = &src[node.byte_range()];
            if program.is_class(name) {
                out.push((node.byte_range(), format!("struct {}", name)));
                return;
            }
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, src, program, out);
        }
    }
    let mut out = Vec::new();
    walk(node, src, program, &mut out);
    out
}

/// Byte-range edits that lower every block-pointer declarator in `node` to a
/// plain function pointer -- `void (^cb)(int)` becomes `void (*cb)(int)` --
/// as absolute offsets into the source.
///
/// A block *is* a function pointer in generated C, and every position that
/// routes through `collect::render_type` already says so: an ivar becomes
/// `void (*_ivarBlk)(int)`, a method parameter becomes
/// `void (*b)(struct k_timer *)`, a local becomes `void (*local)(int)`. The
/// three that did not are the ones assembled by patching the original text,
/// where no edit lowered a block type (#272):
///
///   - a free function's signature, both its prototype and its definition
///     (`static void take_cb(void (^cb)(int));`)
///   - a file-scope block variable (`static void (^g_blk)(int);`)
///
/// Both reached the C compiler with the `^` intact. Blocks are a Clang
/// extension rather than ISO C, so this is not a weaker type but text no GCC
/// target can parse at all: `error: expected ')' before '^' token`.
///
/// Nothing in the repository wrote either shape until #272, which is why
/// they went unnoticed -- the same reason gaps Q, V and R went unnoticed, and
/// the same family: the top-level path getting a reduced version of what a
/// method body gets. `samples/transpiled_blocks` writes both now, and
/// deliberately, so an ARM build is the gate: the Rust suite compiles with
/// the host clang, where a surviving `^` is a valid Clang block.
///
/// A `block_literal` subtree is skipped, because `render_block` synthesizes
/// that function's signature outright rather than patching it, and
/// `top_level_block_edits` replaces the whole literal anyway -- an edit
/// inside it would be discarded or would collide.
///
/// That is the disjointness rule `apply_edits` states, not a quirk of this
/// pass: a pass that replaces a subtree owns its whole byte range, and no
/// other pass may edit inside it. `class_tag_edits` had no such skip until
/// #331, and the file-scope shape this pass exists for -- `static void
/// (^sHook)(Widget *) = ^(Widget *wp) { ... };` -- was corrupted by the
/// collision for the whole life of `top_level_block_edits`.
///
/// The `^` is not the only thing the type has to lose: a type-position `id`
/// in the declarator's own parameter list is lowered to `root` here too, by
/// the same rule as everywhere else an `id` reaches a function-pointer
/// parameter -- see `render_param`. Lowering only the `^` left
/// `static void (^g)(id)` as `void (*g)(id)`, i.e. `void (*)(void *)`,
/// initialized from a hoisted function taking `struct OZObject *`, which
/// Clang rejects as incompatible function pointer types (#319). `root` is
/// `None` when the program has no root class, in which case there is nothing
/// to lower to and `id` stays.
fn block_pointer_edits(
    node: Node,
    src: &str,
    root: Option<&str>,
) -> Vec<(Range<usize>, String)> {
    fn caret(node: Node, src: &str) -> Option<Range<usize>> {
        let mut cursor = node.walk();
        if let Some(tok) = node.children(&mut cursor).find(|c| c.kind() == "^") {
            return Some(tok.byte_range());
        }
        // The grammar names the token, but fall back to the text rather than
        // silently emitting nothing: leaving a `^` behind does not degrade
        // the output, it makes it uncompilable.
        let start = node.start_byte();
        src[node.byte_range()].find('^').map(|off| (start + off)..(start + off + 1))
    }
    fn walk(
        node: Node,
        src: &str,
        root: Option<&str>,
        out: &mut Vec<(Range<usize>, String)>,
    ) {
        if node.kind() == "block_literal" {
            return;
        }
        if matches!(node.kind(), "block_pointer_declarator" | "abstract_block_pointer_declarator")
        {
            if let Some(range) = caret(node, src) {
                out.push((range, "*".to_string()));
            }
        }
        // The parameter list is a *sibling* of the parenthesized declarator
        // holding the `^`, not a child of it, so it is reached from the
        // enclosing function declarator rather than from the arm above.
        if matches!(node.kind(), "function_declarator" | "abstract_function_declarator")
            && wraps_block_pointer_declarator(node)
        {
            if let Some(root) = root {
                let replacement = format!("struct {} *", root);
                let mut cursor = node.walk();
                let lists: Vec<Node> = node
                    .children(&mut cursor)
                    .filter(|c| c.kind() == "parameter_list")
                    .collect();
                for list in lists {
                    rewrite_id_types(list, src, 0, &replacement, out);
                }
            }
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, src, root, out);
        }
    }
    let mut out = Vec::new();
    walk(node, src, root, &mut out);
    out
}

/// Hoist every `block_literal` under a top-level node that no other arm
/// claimed, returning the edits that replace each literal with the name of
/// the function `render_block` synthesized for it.
///
/// Together with `OZM` -- whose two halves are pure preprocessor, in
/// `oz_sdk/Foundation/OZMacro.h` and `platform/oz_platform.h` -- this is
/// what makes a target definition macro writable with an inline block
/// (#272). The hoisting is the transpiler's whole contribution: it turns
/// the literal into a function name, and the preprocessor does the rest.
///
///
/// ```objc
/// OZM(ZBUS_LISTENER_DEFINE, lis_print_temp, ^(const struct zbus_channel *chan) {
///         ...
/// });
/// ```
///
/// The same literal inside a method or free-function *body* has always
/// hoisted -- `walk_top_level`'s passthrough arm copies text, so the literal
/// was simply never reached by `render_block` and arrived at GCC with its
/// `^`. Handled here, at the one place every unclaimed node passes through,
/// rather than per node kind: that is what gap X's bare-`;` fix chose and
/// for the same reason, since it means a future arm gets the same treatment
/// without knowing to ask, and oz_static needs to know no macro's name --
/// `ZBUS_LISTENER_DEFINE`, `K_TIMER_DEFINE` and any other shape are all just
/// unclaimed text with a literal in it.
///
/// The static bar is run over each body, which this position had no scan of
/// at all -- the top-level twin of the free-function scan gap Q added. A
/// rejected block is left exactly as written, so the diagnostic is what the
/// user sees rather than generated code they never wrote.
///
/// A literal is not descended into: `render_block` renders that whole
/// subtree, nested literals included.
fn top_level_block_edits(
    node: Node,
    ctx: &mut EmitCtx,
    program: &Program,
) -> Vec<(Range<usize>, String)> {
    fn walk(
        node: Node,
        ctx: &mut EmitCtx,
        program: &Program,
        out: &mut Vec<(Range<usize>, String)>,
    ) {
        if node.kind() == "block_literal" {
            let mut cursor = node.walk();
            let body = node.children(&mut cursor).find(|c| c.kind() == "compound_statement");
            if let Some(body) = body {
                let reject = crate::staticbar::check_function_body(body, ctx.src, program);
                if !reject.is_empty() {
                    ctx.diags.extend(reject);
                    return;
                }
            }
            let (name, _) = render_block(node, ctx);
            out.push((node.byte_range(), name));
            return;
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, ctx, program, out);
        }
    }
    let mut out = Vec::new();
    walk(node, ctx, program, &mut out);
    out
}

/// Does `node` contain a `block_literal` anywhere?
///
/// Cheap guard so the passthrough arm builds an `EmitCtx` only for the nodes
/// that need one, rather than for every comment and `#include`.
fn contains_block_literal(node: Node) -> bool {
    if node.kind() == "block_literal" {
        return true;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().any(contains_block_literal)
}

/// Apply `edits` (absolute source offsets) to the text of `start..end`.
///
/// The edits must be **disjoint**. Two that overlap truncate each other:
/// applying back to front keeps offsets valid only while each replacement
/// leaves the bytes of every remaining edit alone, and an edit inside
/// another's range does not. What comes out is neither edit but the tail of
/// one spliced into the middle of the other -- text no C compiler accepts,
/// with no diagnostic, since nothing here can tell a truncated splice from
/// an intended one.
///
/// That is a standing rule for every pass that contributes edits, not a
/// local quirk of one: a pass that replaces a whole subtree owns it, and no
/// other pass may edit inside it. Both `block_pointer_edits` and
/// `class_tag_edits` skip a `block_literal` on exactly that ground, because
/// `top_level_block_edits` replaces the literal wholesale (#272, #331).
///
/// The `debug_assert!` is what makes a violation loud. It is not merely a
/// test-only check: `Cargo.toml` keeps oz2c on the dev profile precisely so
/// `debug-assertions` stay on in the binary the build actually runs, so a
/// new pass that overlaps an old one fails at the point of the mistake
/// instead of emitting garbage a C compiler complains about somewhere else.
/// #331 was live for the whole life of `top_level_block_edits` and cost
/// nothing to detect here.
pub(crate) fn apply_edits(src: &str, start: usize, end: usize, edits: &[(Range<usize>, String)]) -> String {
    let mut text = src[start..end].to_string();
    let mut relevant: Vec<&(Range<usize>, String)> =
        edits.iter().filter(|(r, _)| r.start >= start && r.end <= end).collect();
    // Back to front, so earlier offsets stay valid.
    relevant.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for pair in relevant.windows(2) {
        let (later, earlier) = (&pair[0].0, &pair[1].0);
        debug_assert!(
            earlier.end <= later.start,
            "overlapping edits at {}..{} and {}..{}: {:?} vs {:?} in {:?}",
            earlier.start,
            earlier.end,
            later.start,
            later.end,
            pair[1].1,
            pair[0].1,
            &src[start..end]
        );
    }
    for (range, replacement) in relevant {
        text.replace_range(range.start - start..range.end - start, replacement);
    }
    text
}

/// Is `node` a `static inline` function definition?
///
/// Read off the text ahead of the declarator rather than the child nodes,
/// because the two keywords can appear in either order and with any
/// qualifiers or attributes between them.
fn is_static_inline(node: Node, source: &str) -> bool {
    let text = node_text(node, source);
    let prefix = match text.find('(') {
        Some(paren) => &text[..paren],
        None => text,
    };
    let has = |word: &str| {
        prefix.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').any(|t| t == word)
    };
    has("static") && (has("inline") || has("__inline") || has("__inline__"))
}

/// Does `name` appear in `text` as a whole identifier?
///
/// A substring test would match `Car` inside `Carriage`, and a real parse
/// is more than this needs: the caller only wants to know whether a file
/// might refer to a class, and answering "yes" too often merely adds an
/// `#include` that a `#pragma once` header makes free.
fn mentions_identifier(text: &str, name: &str) -> bool {
    let is_ident_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let (haystack, needle) = (text.as_bytes(), name.as_bytes());
    if needle.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(offset) = text[from..].find(name) {
        let start = from + offset;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_ident_byte(haystack[start - 1]);
        let after_ok = end == haystack.len() || !is_ident_byte(haystack[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// First child of `node` with the given kind.
fn child_by_kind_local<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    children.into_iter().find(|c| c.kind() == kind)
}

fn base_scope(class_name: &str, program: &Program) -> HashMap<String, String> {
    program
        .all_ivars(class_name)
        .into_iter()
        .map(|(name, ty)| {
            /* Marked here rather than in `all_ivars` because the scope is
             * the only consumer that has to distinguish them: a struct
             * field declaration wants the plain type plus the extent, and
             * gets both from `array_extents`. */
            if program.array_extent_of(class_name, &name).is_some() {
                let ty = format!("{}{}", ty, ARRAY_MARK);
                (name, ty)
            } else {
                (name, ty)
            }
        })
        .collect()
}

/// File-scope object variables, as `name -> C type`.
///
/// A `static Widget *g_widget;` at translation-unit scope is visible to every
/// method body and to plain top-level functions, but nothing collected it, so
/// a send to it reported the receiver type as `id` and was rejected:
/// "cannot statically resolve the receiver type for selector 'toggle'".
/// `samples/gpio_demo` (`static GPIOOutput *led;`) and `samples/heap_alloc`
/// (`static OZHeap *sHeap;`) are both that shape, and the oracle collects
/// file-scope statics for the same reason (`collect.py`).
///
/// Only declarations at the top level are considered; anything nested is a
/// local and is already handled by `collect_local_decls`.
fn file_scope_vars(root: Node, ctx_src: &str, program: &Program) -> HashMap<String, String> {
    let known: HashSet<String> = program.classes.keys().cloned().collect();
    let mut out = HashMap::new();
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    for child in children {
        if child.kind() != "declaration" {
            continue;
        }
        let (type_text, stars) = crate::collect::extract_type_and_stars(child, ctx_src);
        // A class can be written either way at file scope, and both spellings
        // mean the same thing: `static Widget *g;` gives a `type_identifier`,
        // so `type_text` is `Widget`, while `static struct Widget *g;` goes
        // through `extract_type_and_stars`'s `struct_specifier` arm and gives
        // `struct Widget` -- which is not a key in `known`, so the tagged form
        // was silently skipped and a send to it reported an `id` receiver
        // (#251). `collect_local_decls` has no such gate, which is why the
        // identical *local* resolved and only file scope was affected: the two
        // disagreeing about what counts as an object declaration is the same
        // asymmetry gap R and #246 both came down to.
        //
        // The bare name is what `render_type` wants, since it re-adds the tag.
        let class_name = type_text.strip_prefix("struct ").unwrap_or(&type_text);
        if stars == 0 || !known.contains(class_name) {
            continue;
        }
        let c_type = crate::collect::render_type(class_name, stars, &known);
        let mut c2 = child.walk();
        let declarators: Vec<Node> = child.children(&mut c2).collect();
        for declarator in declarators {
            if !matches!(declarator.kind(), "init_declarator" | "identifier" | "pointer_declarator")
            {
                continue;
            }
            let name = crate::collect::find_declared_name(declarator, ctx_src);
            if !name.is_empty() {
                out.insert(name, c_type.clone());
            }
        }
    }
    out
}

