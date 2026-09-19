// SPDX-License-Identifier: Apache-2.0
//
// preproc.rs - which arm of a preprocessor conditional is part of the
// program.

use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::model::Diagnostic;

/// The top-level Objective-C constructs whose presence inside a
/// conditional makes that conditional oz2c's problem rather than the C
/// compiler's.
///
/// Shared with `main::absorbed_construct`, which reports the same nesting
/// for `--dump-cst`: the marker and the decision must name the same set,
/// or the diagnostic aid stops predicting the behaviour.
pub const TOP_LEVEL_OBJC: &[&str] = &[
    "class_interface",
    "class_implementation",
    "category_interface",
    "category_implementation",
    "protocol_declaration",
];

/// Macros oz2c answers itself because every path that compiles this source
/// defines them, and no `#define` in the buffer will say so.
///
/// Deliberately tiny. `__OBJC__` is defined by every Clang invocation in
/// this repo (`-x objective-c`), and `__clang__` is here only as a
/// backstop -- `imports::unwrap_clang_guard` strips `#ifdef __clang__`
/// line-wise before this module ever sees the text, so in practice it does
/// not arrive.
const BUILTIN_DEFINED: &[&str] = &["__OBJC__", "__clang__"];

/// Namespaces whose members oz2c must not assume are undefined merely
/// because the merged buffer holds no `#define` for them.
///
/// `absent from the buffer` means `undefined` for an ordinary name, and
/// that is what makes `#ifdef MT96_NEVER_DEFINED` decidable (#573). It is
/// **unsound** for two namespaces, and each has a concrete way to reach
/// the compiler without passing through a `#define` oz2c can read:
///
/// - `__x` / `_X` is reserved to the implementation, so the C compiler
///   defines it (`__GNUC__`, `__SIZEOF_INT__`). Handled by
///   `is_reserved_name` rather than a prefix here, since the rule is a
///   shape and not a list.
/// - `CONFIG_` is Zephyr's Kconfig namespace, and those arrive at the C
///   compiler through `-include autoconf.h`. oz2c reads no autoconf.h, so
///   it can neither confirm nor deny one. Guessing `undefined` here would
///   silently select the `#else` arm of every `#ifdef CONFIG_FOO` in a
///   real application -- the exact "convincing false result" shape, and
///   worse than the refusal it replaces.
///
/// A member of either namespace evaluates to `Truth::Unknown`, which for
/// an Objective-C-bearing conditional is a located error naming it.
const EXTERNAL_NAMESPACES: &[&str] = &["CONFIG_"];

/// What oz2c proved about each preprocessor conditional in one merged
/// buffer.
///
/// Every field is a byte range rather than a `Node`, because a `Node`
/// borrows the `Tree` it came from and the front end parses the same text
/// several times over (`collect`, `arc`, `generics`, `emit`). Ranges
/// survive that; they also survive `parse::repair_bare_macro_statements`,
/// which overwrites a whitespace byte in place and so preserves every
/// offset.
#[derive(Debug, Default, Clone)]
pub struct Liveness {
    /// Text oz2c proved is not part of the program: the body of every
    /// conditional arm whose condition it decided against.
    ///
    /// Not restricted to Objective-C-bearing conditionals. A check firing
    /// on text inside `#if 0` is wrong whatever that text is (#570), and
    /// scoping the suppression to arms that happen to hold an
    /// `@implementation` would leave the same defect waiting in a
    /// C-only one.
    dead: Vec<Range<usize>>,
    /// Whole-conditional spans that carry a top-level Objective-C
    /// construct and whose live arm oz2c identified.
    ///
    /// A top-level walk must flatten these to the live arm's children
    /// instead of copying the node's text through, which is what
    /// `effective_top_level` does.
    resolved_objc: Vec<Range<usize>>,
    /// One located error per Objective-C-bearing conditional whose
    /// condition oz2c could not evaluate.
    ///
    /// Public because `lib::front_end` reports these with the rest of the
    /// front end's refusals; there is no soft-diagnostic mode to demote
    /// them to.
    pub undecidable: Vec<Diagnostic>,
}

/// One arm of a conditional chain: the condition that selects it, and the
/// body nodes it contributes to the enclosing scope.
struct Arm<'t> {
    /// The condition node, and whether a `#ifndef` inverts it. `None` for
    /// an `#else`, which is selected by elimination.
    condition: Option<(Node<'t>, bool)>,
    /// The directive node the condition belongs to, for anchoring a
    /// diagnostic.
    directive: Node<'t>,
    /// The arm's own children -- everything but its directive tokens, its
    /// condition, and the nested `#elif`/`#else` that continues the chain.
    body: Vec<Node<'t>>,
}

/// Whether a condition holds, when oz2c may not be able to tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    fn not(self) -> Truth {
        match self {
            Truth::True => Truth::False,
            Truth::False => Truth::True,
            Truth::Unknown => Truth::Unknown,
        }
    }
}

impl Liveness {
    /// Decide every conditional in `source`.
    ///
    /// Parses once. Descends into a *live* arm to reach a conditional
    /// nested inside it, and does not descend into a dead one -- the whole
    /// dead arm is already recorded, and a conditional inside text that is
    /// not part of the program has no live arm to speak of.
    pub fn scan(source: &str) -> Liveness {
        let tree = crate::parse::parse(source);
        let defined = defined_names(tree.root_node(), source);
        let mut out = Liveness::default();
        out.scan_children(tree.root_node(), source, &defined);
        out
    }

    fn scan_children(&mut self, node: Node, source: &str, defined: &HashSet<String>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if is_conditional(child) {
                self.scan_conditional(child, source, defined);
            } else {
                /* A conditional can sit inside any construct -- a
                 * `#ifdef` around one method of an `@implementation` is
                 * ordinary code -- so the search is the whole tree and not
                 * just its top level. */
                self.scan_children(child, source, defined);
            }
        }
    }

    fn scan_conditional(&mut self, node: Node, source: &str, defined: &HashSet<String>) {
        let arms = chain(node);
        let carries_objc = contains_top_level_objc(node);

        /* First arm whose condition is true wins, and every other arm is
         * dead. An `Unknown` before that point makes the whole chain
         * undecidable: oz2c cannot know whether a later arm is reached
         * without knowing whether this one was. */
        let mut live: Option<usize> = None;
        let mut blocked_by: Option<(Node, Option<Node>)> = None;
        for (i, arm) in arms.iter().enumerate() {
            let truth = match arm.condition {
                None => Truth::True, /* `#else` -- reached, so selected */
                Some((condition, inverted)) => {
                    let t = eval(condition, source, defined);
                    if inverted {
                        t.not()
                    } else {
                        t
                    }
                }
            };
            match truth {
                Truth::True => {
                    live = Some(i);
                    break;
                }
                Truth::False => continue,
                Truth::Unknown => {
                    blocked_by = Some((arm.directive, arm.condition.map(|(c, _)| c)));
                    break;
                }
            }
        }

        if let Some((directive, condition)) = blocked_by {
            if carries_objc {
                self.undecidable.push(undecidable_diagnostic(
                    node, directive, condition, source,
                ));
            }
            /* Nothing is known about any arm, so nothing is dead and
             * nothing is flattened. A conditional carrying no
             * Objective-C keeps today's behaviour exactly: its text is
             * copied through and the C compiler decides it. */
            return;
        }

        for (i, arm) in arms.iter().enumerate() {
            if Some(i) == live {
                continue;
            }
            if let Some(span) = body_span(&arm.body) {
                self.dead.push(span);
            }
        }

        match live {
            Some(i) => {
                if carries_objc {
                    self.resolved_objc.push(node.byte_range());
                }
                let mut cursor = arms[i].body.clone();
                for body in cursor.drain(..) {
                    if is_conditional(body) {
                        self.scan_conditional(body, source, defined);
                    } else {
                        self.scan_children(body, source, defined);
                    }
                }
            }
            None => {
                /* Every arm decided false and there was no `#else`: the
                 * conditional contributes nothing. Each arm is already in
                 * `dead`, and an Objective-C-bearing one still needs
                 * flattening so its text is not copied through -- to
                 * nothing, correctly. */
                if carries_objc {
                    self.resolved_objc.push(node.byte_range());
                }
            }
        }
    }

    /// Whether `byte` sits in text oz2c proved is not part of the program.
    pub fn is_dead(&self, byte: usize) -> bool {
        self.dead.iter().any(|r| r.contains(&byte))
    }

    /// Whether `diagnostic` is about text that is part of the program.
    ///
    /// A diagnostic with no span is kept: the three whole-program checks
    /// that lack one are not about a position at all, so no dead arm can
    /// contain them.
    pub fn keep(&self, diagnostic: &Diagnostic) -> bool {
        match &diagnostic.span {
            None => true,
            Some(span) => !self.is_dead(span.start),
        }
    }

    /// Drop every diagnostic about text oz2c proved is not in the program.
    ///
    /// Applied where diagnostics leave a pass rather than inside each
    /// check, because there are 306 tree walks in this crate and one of
    /// them firing on `#if 0` text was the whole of #570. Filtering here
    /// fixes the checks that exist and the ones nobody has written yet.
    pub fn retain_live(&self, diagnostics: &mut Vec<Diagnostic>) {
        if self.dead.is_empty() {
            return;
        }
        diagnostics.retain(|d| self.keep(d));
    }

    /// `node`'s children as the program actually contains them: a
    /// conditional oz2c resolved is replaced by its live arm's children,
    /// flattened in place.
    ///
    /// Every top-level iteration in `collect` and `emit` goes through
    /// this. Without it the nested `@interface` reaches no arm that knows
    /// what to do with it, and `emit`'s catch-all copies the whole
    /// conditional into the generated header as raw Objective-C (#573).
    ///
    /// A conditional oz2c could *not* resolve is returned as itself, so
    /// the catch-all still copies it through and the C compiler still
    /// decides it. That is what keeps the SDK's C-level conditionals
    /// (`#ifndef OZ_Q31_HELPERS` and its neighbours) working unchanged.
    pub fn effective_top_level<'t>(&self, node: Node<'t>) -> Vec<Node<'t>> {
        let mut out = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if is_conditional(child) && self.resolved_objc.contains(&child.byte_range()) {
                self.push_live_arm(child, &mut out);
            } else {
                out.push(child);
            }
        }
        out
    }

    /// The live arm's children of a resolved conditional, appended to
    /// `out`. Recurses, so a conditional nested in the live arm is
    /// flattened too.
    fn push_live_arm<'t>(&self, node: Node<'t>, out: &mut Vec<Node<'t>>) {
        for arm in chain(node) {
            let Some(span) = body_span(&arm.body) else {
                continue;
            };
            if self.is_dead(span.start) {
                continue;
            }
            for body in arm.body {
                if is_conditional(body) && self.resolved_objc.contains(&body.byte_range()) {
                    self.push_live_arm(body, out);
                } else {
                    out.push(body);
                }
            }
        }
    }
}

/// Whether `node` is a conditional group -- the node that owns a whole
/// `#if`/`#endif`, not one of its arms.
///
/// `preproc_elif` and `preproc_else` are *children* of the group in this
/// grammar rather than siblings, so they are arms and never groups.
fn is_conditional(node: Node) -> bool {
    matches!(node.kind(), "preproc_if" | "preproc_ifdef")
}

/// Flatten a conditional group into its arms, in source order.
///
/// The grammar nests the continuation: a `preproc_ifdef`'s last child is
/// its `preproc_elif`, whose last child is the next `preproc_elif` or the
/// `preproc_else`. Walking that chain is what turns it back into the list
/// of alternatives the author wrote.
fn chain<'t>(node: Node<'t>) -> Vec<Arm<'t>> {
    let mut arms = Vec::new();
    let mut current = Some(node);
    while let Some(group) = current {
        let mut cursor = group.walk();
        let children: Vec<Node> = group.children(&mut cursor).collect();

        /* `#ifndef X` and `#ifdef X` are one node kind distinguished only
         * by the directive token's text. */
        let directive = children.first().copied().unwrap_or(group);
        let directive_text = directive.kind();
        let inverted = directive_text == "#ifndef";

        let condition = match directive_text {
            "#ifdef" | "#ifndef" => children
                .iter()
                .find(|c| c.kind() == "identifier")
                .map(|c| (*c, inverted)),
            "#if" | "#elif" => children.get(1).filter(|c| !is_trivia(**c)).map(|c| (*c, false)),
            /* `#else` selects by elimination and carries no condition. */
            _ => None,
        };

        let mut next = None;
        let mut body = Vec::new();
        for child in children {
            if child.kind() == "preproc_elif" || child.kind() == "preproc_else" {
                next = Some(child);
                continue;
            }
            if is_directive_token(child) || is_trivia(child) {
                continue;
            }
            if let Some((condition, _)) = condition {
                if child.id() == condition.id() {
                    continue;
                }
            }
            body.push(child);
        }

        arms.push(Arm { condition, directive, body });
        current = next;
    }
    arms
}

fn is_directive_token(node: Node) -> bool {
    matches!(node.kind(), "#if" | "#ifdef" | "#ifndef" | "#elif" | "#else" | "#endif")
}

/// A node carrying no program text of its own -- the newline the grammar
/// keeps after a condition, and comments.
fn is_trivia(node: Node) -> bool {
    node.kind().trim().is_empty() || node.kind() == "comment"
}

/// The span an arm's body occupies, or `None` for an empty arm.
fn body_span(body: &[Node]) -> Option<Range<usize>> {
    let first = body.first()?;
    let last = body.last()?;
    Some(first.start_byte()..last.end_byte())
}

/// Whether a top-level Objective-C construct sits anywhere inside `node`.
fn contains_top_level_objc(node: Node) -> bool {
    if TOP_LEVEL_OBJC.contains(&node.kind()) {
        return true;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().any(contains_top_level_objc)
}

/// Every name the merged buffer `#define`s.
///
/// Position is not tracked: a name defined anywhere in the buffer counts
/// as defined everywhere in it. That is a deliberate over-approximation in
/// the safe direction -- it can only turn a decision into
/// `Truth::Unknown`-adjacent looseness on an already-defined name, never
/// turn an absent name into a present one, and the absent case is the one
/// a wrong answer would silently mis-lower.
fn defined_names(node: Node, source: &str) -> HashSet<String> {
    let mut out: HashSet<String> = BUILTIN_DEFINED.iter().map(|s| s.to_string()).collect();
    collect_defines(node, source, &mut out);
    out
}

fn collect_defines(node: Node, source: &str, out: &mut HashSet<String>) {
    if node.kind() == "preproc_def" || node.kind() == "preproc_function_def" {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        if let Some(name) = children.into_iter().find(|c| c.kind() == "identifier") {
            out.insert(source[name.byte_range()].to_string());
        }
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children {
        collect_defines(child, source, out);
    }
}

/// Whether `name` is reserved to the implementation, and so may be defined
/// by the C compiler without any `#define` in the buffer.
///
/// C17 7.1.3: identifiers beginning with an underscore followed by an
/// uppercase letter or a second underscore. A shape, not a list, which is
/// why `__GNUC__` needs no entry in `BUILTIN_DEFINED` to be treated as
/// unknowable.
fn is_reserved_name(name: &str) -> bool {
    let mut chars = name.chars();
    if chars.next() != Some('_') {
        return false;
    }
    match chars.next() {
        Some('_') => true,
        Some(c) => c.is_ascii_uppercase(),
        None => false,
    }
}

/// Whether oz2c can answer "is `name` defined?" at all.
fn definedness_is_knowable(name: &str) -> bool {
    !is_reserved_name(name) && !EXTERNAL_NAMESPACES.iter().any(|p| name.starts_with(p))
}

/// Evaluate a `#if` / `#elif` condition, or a `#ifdef`'s identifier.
fn eval(node: Node, source: &str, defined: &HashSet<String>) -> Truth {
    match node.kind() {
        "number_literal" => match integer_value(&source[node.byte_range()]) {
            Some(0) => Truth::False,
            Some(_) => Truth::True,
            None => Truth::Unknown,
        },
        "preproc_defined" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            match children.into_iter().find(|c| c.kind() == "identifier") {
                Some(name) => is_defined(&source[name.byte_range()], defined),
                None => Truth::Unknown,
            }
        }
        /* A `#ifdef`'s operand, and a bare name in a `#if`. An identifier
         * the buffer never defines expands to nothing and the condition
         * reads as `0`, which is C's rule and what makes
         * `#ifdef MT96_NEVER_DEFINED` decidable. A *defined* one expands
         * to its replacement list, which oz2c does not evaluate -- so it
         * is knowably-defined but its value is unknown, and only
         * `#ifdef`'s caller (which asks about definedness, not value) can
         * use that. */
        "identifier" => match is_defined(&source[node.byte_range()], defined) {
            Truth::False => Truth::False,
            _ => Truth::Unknown,
        },
        "parenthesized_expression" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let inner = children
                .into_iter()
                .find(|c| !is_trivia(*c) && c.kind() != "(" && c.kind() != ")");
            match inner {
                Some(inner) => eval(inner, source, defined),
                None => Truth::Unknown,
            }
        }
        "unary_expression" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let Some(operator) = children.first() else {
                return Truth::Unknown;
            };
            let Some(operand) = children.get(1) else {
                return Truth::Unknown;
            };
            match operator.kind() {
                "!" => eval(*operand, source, defined).not(),
                _ => Truth::Unknown,
            }
        }
        "binary_expression" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let Some(left) = children.first() else {
                return Truth::Unknown;
            };
            let Some(operator) = children.get(1) else {
                return Truth::Unknown;
            };
            let Some(right) = children.get(2) else {
                return Truth::Unknown;
            };
            let left = eval(*left, source, defined);
            let right = eval(*right, source, defined);
            match operator.kind() {
                /* Short-circuits on a decided operand, so one unknown
                 * half does not cost the whole condition:
                 * `defined(NOPE) && <anything>` is false. */
                "&&" => {
                    if left == Truth::False || right == Truth::False {
                        Truth::False
                    } else if left == Truth::True && right == Truth::True {
                        Truth::True
                    } else {
                        Truth::Unknown
                    }
                }
                "||" => {
                    if left == Truth::True || right == Truth::True {
                        Truth::True
                    } else if left == Truth::False && right == Truth::False {
                        Truth::False
                    } else {
                        Truth::Unknown
                    }
                }
                _ => Truth::Unknown,
            }
        }
        _ => Truth::Unknown,
    }
}

fn is_defined(name: &str, defined: &HashSet<String>) -> Truth {
    if defined.contains(name) {
        return Truth::True;
    }
    if definedness_is_knowable(name) {
        Truth::False
    } else {
        Truth::Unknown
    }
}

/// The value of a C integer literal as a `#if` sees it.
///
/// Only what a condition plausibly holds: an optional radix prefix and the
/// `u`/`l` suffixes. Anything else -- a float, a character constant, a
/// digit separator -- is `None`, which reads as `Truth::Unknown` and so
/// refuses rather than guesses.
fn integer_value(text: &str) -> Option<i64> {
    let text = text.trim();
    let text = text.trim_end_matches(|c| matches!(c, 'u' | 'U' | 'l' | 'L'));
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok();
    }
    if let Some(bin) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
        return i64::from_str_radix(bin, 2).ok();
    }
    if text.len() > 1 && text.starts_with('0') {
        return i64::from_str_radix(&text[1..], 8).ok();
    }
    text.parse::<i64>().ok()
}

/// The refusal for an Objective-C-bearing conditional oz2c cannot decide.
///
/// Hard and located, per the standing rule: oz2c has no soft-diagnostic
/// mode, and the alternative here is not a warning but the broken C of
/// #573 -- a raw `@interface` copied into the generated header and no
/// struct, no method and no dispatch row behind it.
fn undecidable_diagnostic(
    group: Node,
    directive: Node,
    condition: Option<Node>,
    source: &str,
) -> Diagnostic {
    /* The whole directive line -- `#ifdef CONFIG_FOO`, not the bare
     * `#ifdef`. A reader needs the name to know which macro oz2c could
     * not answer for. */
    let end = condition.map_or(directive.end_byte(), |c| c.end_byte());
    let text = crate::emit::one_line(&source[directive.start_byte()..end]);
    Diagnostic::spanning(
        format!(
            "oz2c cannot evaluate '{}', and it guards an Objective-C declaration whose \
             lowering is a whole-program decision",
            text
        ),
        source,
        group.byte_range(),
    )
    .with_note(
        "a class becomes a struct, a row in the shared dispatch table and a slab sized for \
         its allocation sites, so which arm is live has to be settled before any C is \
         emitted -- it cannot be deferred to the C compiler the way a conditional around \
         plain C can. oz2c reads only this translation unit's own '#define's, so a macro \
         supplied on the command line or by Zephyr's 'autoconf.h' is one it cannot see",
    )
    .with_help(
        "move the Objective-C out of the conditional and guard the plain C that uses it \
         instead",
    )
    .with_help(
        "or '#define' the macro in this translation unit, where oz2c can read it",
    )
}
