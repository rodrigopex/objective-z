// SPDX-License-Identifier: Apache-2.0
//
// pools.rs - how many instances of each class the generated slab has to
// hold.
//
// Ported from the Python pipeline's `_count_alloc_calls`
// (tools/oz_transpile/emit.py): one slot per *allocation site*, not per
// execution, counting explicit `[ClassName alloc]` sends plus the
// implicit allocations the literal desugars perform (`@[...]` -> OZArray,
// `@{...}` -> OZDictionary, `@42` -> OZQ31). A site is counted once
// however many times it runs, which is why the count is a floor rather
// than a bound and why `--pool-sizes` exists to override it.
//
// Two differences from the oracle, both because oz_static already decided
// the question elsewhere:
//
//   - the oracle also tracks "uncertain" sites -- an allocation inside a
//     loop that doesn't initialize a fresh per-iteration local, which can
//     accumulate live instances across iterations -- and reports them as
//     a soft `OZ004` diagnostic asking for an explicit override.
//     `staticbar::walk_for_reject` already makes that exact shape a hard
//     error ("allocation of '{}' inside a loop escapes the iteration"),
//     so by the time sizing runs it cannot occur.
//   - the oracle reserves a slot per `@synchronized` block for the
//     OZSpinLock object it allocates. oz_static's `@synchronized` lowers
//     to a stack-local lock with no object at all (see
//     `emit::render_synchronized_statement`), so there is nothing to size.

use std::collections::HashMap;

use tree_sitter::Node;

use crate::model::Program;

/// Alignment passed to `OZ_SLAB_DEFINE`, matching the oracle's own
/// emission (`emit.py`: `OZ_SLAB_DEFINE(oz_slab_{name}, sizeof(struct
/// {name}), {count}, 4)`). On Zephyr this reaches `K_MEM_SLAB_DEFINE`,
/// which requires the block size to be a multiple of it; every generated
/// struct leads with pointer- or word-sized tracking fields, so 4 always
/// divides `sizeof`.
pub const SLAB_ALIGNMENT: u32 = 4;

pub struct PoolSizes {
    counted: HashMap<String, usize>,
    /// From the source's own `/* oz-pool: ... */`. Kept apart from `cli`
    /// because the two are held to different standards -- see
    /// `unknown_overrides`.
    directive: HashMap<String, usize>,
    cli: HashMap<String, usize>,
}

impl PoolSizes {
    /// Count allocation sites across the whole translation unit, then
    /// apply any `/* oz-pool: ... */` directive the source carries.
    ///
    /// The oracle walks each method/function body AST in turn; walking the
    /// tree once from the root reaches the same sites (every body is under
    /// it) without needing to enumerate the bodies first.
    pub fn analyze(source: &str, program: &Program) -> Self {
        let tree = crate::parse::parse(source);
        let mut counted = HashMap::new();
        count_sites(tree.root_node(), source, program, &mut counted);
        let directive = parse_pool_directive(source).unwrap_or_default();
        PoolSizes { counted, directive, cli: HashMap::new() }
    }

    /// Apply `--pool-sizes Class=N,...` overrides on top of the counted
    /// sizes and any source directive. An override always wins, including
    /// when it is smaller: the author may know a bound the static count
    /// cannot see. A CLI override beats a source directive for the classes
    /// it names, being specific to this invocation; classes it doesn't
    /// name keep whatever the directive said.
    pub fn set_overrides(&mut self, overrides: HashMap<String, usize>) {
        self.cli.extend(overrides);
    }

    /// Names given an override on the *command line* that aren't classes
    /// in this program -- almost always a typo, and silently ignoring it
    /// would leave the pool at its counted size with no hint why.
    ///
    /// A source `/* oz-pool: ... */` directive is deliberately not
    /// checked. The same directive is read by both backends, and the
    /// oracle has classes oz_static does not: every
    /// `tests/behavior/cases/synchronized/*.m` names `OZSpinLock`, which
    /// the oracle allocates per `@synchronized` block and oz_static never
    /// creates at all (its lock is a stack local -- see
    /// `emit::render_synchronized_statement`). Rejecting those would fail
    /// five corpus cases over a class whose absence is the point.
    pub fn unknown_overrides(&self, program: &Program) -> Vec<String> {
        let mut unknown: Vec<String> =
            self.cli.keys().filter(|name| !program.is_class(name)).cloned().collect();
        unknown.sort();
        unknown
    }

    /// Slots to reserve for `name`. Never zero: a class with no
    /// allocation site still gets one slot, because
    /// `K_MEM_SLAB_DEFINE(..., 0, ...)` is not a usable slab and the
    /// class's alloc function is emitted regardless of whether this
    /// translation unit happens to call it.
    pub fn for_class(&self, name: &str) -> usize {
        self.cli
            .get(name)
            .copied()
            .or_else(|| self.directive.get(name).copied())
            .or_else(|| self.counted.get(name).copied())
            .unwrap_or(0)
            .max(1)
    }
}

/// Parse a `Class=N,Class2=M` list, as accepted by `--pool-sizes` and by
/// the oracle's identically-spelled flag. Returns the offending text on
/// malformed input rather than skipping it, so a typo is a hard error
/// instead of a silently-unapplied override.
pub fn parse_pool_sizes(spec: &str) -> Result<HashMap<String, usize>, String> {
    let mut out = HashMap::new();
    for entry in spec.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((name, count)) = entry.split_once('=') else {
            return Err(format!("'{}' is not 'Class=N'", entry));
        };
        let name = name.trim();
        let count: usize = count
            .trim()
            .parse()
            .map_err(|_| format!("'{}' has a non-numeric count", entry))?;
        if name.is_empty() {
            return Err(format!("'{}' has an empty class name", entry));
        }
        out.insert(name.to_string(), count);
    }
    Ok(out)
}

/// The `/* oz-pool: Class=N,... */` directive, read straight from the
/// source text. This is the oracle's own convention, not something new:
/// `tests/tools/compile_and_run.py` matches the same comment
/// (`POOL_RE = /\*\s*oz-pool:\s*(.+?)\s*\*/`) and forwards it as
/// `--pool-sizes`, and 42 of the cases under `tests/behavior/cases/`
/// declare one. Reading it here means a case's sizes travel with the case
/// rather than having to be replayed by whatever harness compiles it.
///
/// Scanned textually rather than off the CST because a comment is not a
/// node: tree-sitter attaches it nowhere useful, and the oracle's own
/// contract is defined on the text.
///
/// Malformed content is ignored rather than rejected, unlike the
/// identically-shaped `--pool-sizes` argument. The difference is who is
/// speaking: a stray `oz-pool:`-looking comment in prose should not fail a
/// build, whereas a command-line flag was unambiguously meant as one.
fn parse_pool_directive(source: &str) -> Option<HashMap<String, usize>> {
    let start = source.find("oz-pool:")?;
    let after = &source[start + "oz-pool:".len()..];
    let end = after.find("*/")?;
    parse_pool_sizes(after[..end].trim()).ok()
}

fn count_sites(node: Node, src: &str, program: &Program, counts: &mut HashMap<String, usize>) {
    let allocated = match node.kind() {
        "message_expression" => alloc_receiver_class(node, src, program),
        // The desugars these drive allocate through the same per-class
        // alloc path, so they consume slots exactly like an explicit
        // `[X alloc]` (see `emit::render_boxed_*`).
        "array_literal" => Some("OZArray".to_string()),
        "dictionary_literal" => Some("OZDictionary".to_string()),
        "at_expression" if crate::emit::is_numeric_boxed_shape(node, src) => {
            Some("OZQ31".to_string())
        }
        _ => None,
    };
    if let Some(name) = allocated {
        *counts.entry(name).or_insert(0) += 1;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children {
        count_sites(child, src, program, counts);
    }
}

/// `[ClassName alloc]` -- the receiver has to be a literal class name for
/// this to size anything, which is the only form that can allocate: `alloc`
/// is a class method, and a class-method receiver is always statically
/// known (see `Program::is_dynamically_dispatched`).
fn alloc_receiver_class(node: Node, src: &str, program: &Program) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<Node> =
        node.children(&mut cursor).filter(|c| c.kind() != "[" && c.kind() != "]").collect();
    if children.len() != 2 {
        return None;
    }
    let receiver = &src[children[0].byte_range()];
    let selector = &src[children[1].byte_range()];
    if selector != "alloc" {
        return None;
    }
    if program.is_class(receiver) {
        Some(receiver.to_string())
    } else {
        None
    }
}
