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
// Since #410 a site is counted once *per call site of the body it escapes
// from*. An allocation an owning factory hands back outlives the call, so
// `+make` called three times needs three slots where the old rule gave
// one and the second and third calls got nil. A site whose reference dies
// inside its body still counts once, however often that body runs --
// `arc::allocation_escapes_via_return` is what tells the two apart, so a
// helper a factory allocates and drops does not multiply.
//
// Two limits of that, both stated here rather than left to be found:
//
//   - **An instance send names no callee.** This pass tracks no locals, so
//     `[obj make]` cannot be resolved to a body and its callee keeps the
//     pre-#410 floor of one slot. Class-method sends (the receiver is
//     always statically known) and plain C calls do resolve. The failure
//     direction is under-counting, which is the old behaviour, not a new
//     hazard.
//   - **The floor for an uncalled body is 0 for a class method and 1 for
//     everything else.** A class method not called here is genuinely not
//     called; an instance method may arrive through dynamic dispatch and a
//     C function may be an entry point, so silence there means "unknown"
//     and must over-count. With a floor of 1 everywhere, `src/OZQ31.m`'s
//     seventeen uncalled `+fixedWith...` forwarders each claimed a slot
//     and every program sized OZQ31 at 16 -- `samples/hello_category`
//     included, which uses no OZQ31 at all.
//
// Two differences from the oracle, both because oz_static already decided
// the question elsewhere:
//
//   - the oracle also tracks "uncertain" sites -- an allocation inside a
//     loop whose result can outlive the iteration -- and reports them as a
//     soft `OZ004` diagnostic asking for an explicit override.
//     `staticbar::walk_for_reject` makes the unbounded form of that a hard
//     error ("allocation of '{}' inside a loop escapes the iteration"), so
//     sizing never has to guess: what reaches it is bounded.
//
//     Note "bounded" is not the same as "does not occur", which is what this
//     comment used to claim. An allocation in a loop assigned to a strong
//     local reaches sizing routinely and is counted once, and that is
//     correct: `emit::render_strong_local_assign` releases the previous
//     object *before* allocating the next, so the slot is returned and one
//     serves the whole loop. It is only accumulation -- a destination the
//     emitter cannot bound -- that the bar rejects. Measured in
//     `tests/arc_strong_locals.rs`: 100 iterations on a 1-slot pool.
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
    /// Total `id`-slots the shared item pool has to hold, for the element
    /// buffers behind `@[...]` and `@{...}`. One number rather than a
    /// per-class map: both collections draw from the same pool, exactly as
    /// the oracle's single `oz_item_pool` does.
    item_slots_counted: usize,
    item_slots_directive: Option<usize>,
    item_slots_cli: Option<usize>,
}

impl PoolSizes {
    /// Count allocation sites across the whole translation unit, then
    /// apply any `/* oz-pool: ... */` directive the source carries.
    ///
    /// The oracle walks each method/function body AST in turn; walking the
    /// tree once from the root reaches the same sites (every body is under
    /// it) without needing to enumerate the bodies first.
    pub fn analyze(source: &str, program: &Program) -> (Self, Vec<crate::model::Diagnostic>) {
        let tree = crate::parse::parse(source);
        let mut scan = Scan::default();
        walk_sites(tree.root_node(), source, program, None, None, None, &mut scan);
        let (counted, item_slots_counted, diagnostics) = scan.resolve(program);
        let directive = parse_pool_directive(source).unwrap_or_default();
        (
            PoolSizes {
                counted,
                directive,
                cli: HashMap::new(),
                item_slots_counted,
                item_slots_directive: parse_item_pool_directive(source),
                item_slots_cli: None,
            },
            diagnostics,
        )
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

    /// Apply `--item-pool-size N`. Same precedence rule as
    /// `set_overrides`: the command line beats the source directive, which
    /// beats the static count.
    pub fn set_item_pool_override(&mut self, slots: usize) {
        self.item_slots_cli = Some(slots);
    }

    /// Slots the shared item pool needs.
    ///
    /// Unlike `for_class`, zero is meaningful and is *not* floored to one:
    /// a program with no array or dictionary literal needs no pool, and
    /// `OZ_MEM_BLOCKS_DEFINE(..., 0, ...)` reaches
    /// `SYS_MEM_BLOCKS_DEFINE` with a zero block count on Zephyr. The
    /// emitters therefore treat zero as "emit neither the pool nor the
    /// builders that draw from it", which is what the oracle's
    /// `{% if item_pool_count > 0 %}` guards do
    /// (`templates/oz_dispatch.c.j2`, `templates/class_header.h.j2`).
    pub fn item_slots(&self) -> usize {
        self.item_slots_cli
            .or(self.item_slots_directive)
            .unwrap_or(self.item_slots_counted)
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

/// The `/* oz-item-pool: N */` directive: how many `id`-slots to reserve
/// for collection element buffers.
///
/// Spelled to match `/* oz-pool: ... */` above rather than the oracle's
/// `--item-pool-size` flag, because a directive is what travels with a
/// source file. The two keys cannot be confused for each other in either
/// direction: `"oz-pool:"` does not occur inside `"oz-item-pool:"` (after
/// `oz-` comes `item-`), so neither `find` can match the other's
/// directive. Locked down by
/// `item_pool_directive_does_not_disturb_the_class_pool_directive`.
///
/// Malformed content is ignored rather than rejected, for the same reason
/// `parse_pool_directive` ignores it: a comment is not a command line.
fn parse_item_pool_directive(source: &str) -> Option<usize> {
    let start = source.find("oz-item-pool:")?;
    let after = &source[start + "oz-item-pool:".len()..];
    let end = after.find("*/")?;
    after[..end].trim().parse().ok()
}

/// The method or plain C function an allocation site -- or a call -- sits
/// in. Sizing needs the enclosing body's *identity*, not just its text,
/// because an allocation that escapes costs one slot per call site of
/// whatever it escapes from (#410).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Owner {
    Method(String, String),
    Function(String),
}

impl Owner {
    fn describe(&self) -> String {
        match self {
            Owner::Method(class, selector) => format!("[{} {}]", class, selector),
            Owner::Function(name) => format!("{}()", name),
        }
    }
}

struct AllocSite {
    class: String,
    /// `None` for a site outside any body this pass recognises; it is
    /// counted once, exactly as every site was before #410.
    owner: Option<Owner>,
    /// Does the reference leave its enclosing body through a `return`?
    /// Only then does the site cost one slot per call site.
    escapes: bool,
}

#[derive(Default)]
struct Scan {
    sites: Vec<AllocSite>,
    /// Callee -> the owner each of its resolvable call sites sits in.
    /// `None` for a call outside any recognised body.
    callers: HashMap<Owner, Vec<Option<Owner>>>,
    item_slots: usize,
}

/// One walk, collecting allocation sites with their enclosing body and the
/// call edges between bodies.
///
/// `body` is the enclosing `compound_statement`, carried because
/// `arc::allocation_escapes_via_return` needs the whole body to follow a
/// returned name back to its declaration.
fn walk_sites(
    node: Node,
    src: &str,
    program: &Program,
    class: Option<&str>,
    owner: Option<&Owner>,
    body: Option<Node>,
    scan: &mut Scan,
) {
    /* Descend into a new owner where one starts, so every site below it is
     * attributed to it rather than to whatever enclosed the class. */
    match node.kind() {
        "class_implementation" => {
            let (name, _, _) = crate::collect::class_header(node, src);
            if !name.is_empty() {
                let mut cursor = node.walk();
                let children: Vec<Node> = node.children(&mut cursor).collect();
                for child in children {
                    walk_sites(child, src, program, Some(&name), owner, body, scan);
                }
                return;
            }
        }
        "method_definition" => {
            if let Some(class_name) = class {
                let known = program.classes.keys().cloned().collect();
                let sig = crate::collect::extract_method_sig(node, src, class_name, &known);
                let this = Owner::Method(class_name.to_string(), sig.selector);
                let mut cursor = node.walk();
                let children: Vec<Node> = node.children(&mut cursor).collect();
                let this_body = children.iter().find(|c| c.kind() == "compound_statement").copied();
                for child in children {
                    walk_sites(child, src, program, class, Some(&this), this_body, scan);
                }
                return;
            }
        }
        "function_definition" => {
            if let Some(name) = crate::arc::function_name(node, src) {
                let this = Owner::Function(name);
                let mut cursor = node.walk();
                let children: Vec<Node> = node.children(&mut cursor).collect();
                let this_body = children.iter().find(|c| c.kind() == "compound_statement").copied();
                for child in children {
                    walk_sites(child, src, program, class, Some(&this), this_body, scan);
                }
                return;
            }
        }
        _ => {}
    }

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
        let escapes = body
            .is_some_and(|b| crate::arc::allocation_escapes_via_return(node, b, src));
        scan.sites.push(AllocSite { class: name, owner: owner.cloned(), escapes });
    } else if node.kind() == "message_expression" {
        /* A call edge, for the multiplicity of whatever it calls. Only a
         * class-method send with a literal class receiver is resolvable
         * here: pools tracks no locals, so `[obj make]` names no callee.
         * That under-counts rather than over-counts -- such a site keeps
         * the pre-#410 floor of one slot -- and is stated in the module
         * header rather than left to be discovered. */
        if let Some(callee) = class_method_callee(node, src, program) {
            scan.callers.entry(callee).or_default().push(owner.cloned());
        }
    } else if node.kind() == "call_expression" {
        if let Some(callee) = c_function_callee(node, src) {
            scan.callers.entry(callee).or_default().push(owner.cloned());
        }
    }
    // Element slots, on top of the one object slot counted above. The
    // element counts must agree with what `emit::render_boxed_array_literal`
    // and `render_boxed_dictionary_literal` actually pass as `count`, so
    // both use the same child filters those do.
    scan.item_slots += match node.kind() {
        "array_literal" => array_element_count(node),
        // Keys and values share one contiguous run of `2 * pairs` slots
        // (`companion::render_dict_support` points `_keys` at the first
        // half and `_values` at the second).
        "dictionary_literal" => 2 * dictionary_pair_count(node),
        _ => 0,
    };
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children {
        walk_sites(child, src, program, class, owner, body, scan);
    }
}

/// `[ClassName selector]` where the class declares that class method --
/// the only send whose callee pools can name without tracking types.
fn class_method_callee(node: Node, src: &str, program: &Program) -> Option<Owner> {
    let mut cursor = node.walk();
    let children: Vec<Node> =
        node.children(&mut cursor).filter(|c| c.kind() != "[" && c.kind() != "]").collect();
    if children.len() < 2 {
        return None;
    }
    let receiver = &src[children[0].byte_range()];
    if !program.is_class(receiver) {
        return None;
    }
    let selector = crate::staticbar::message_selector(node, src);
    if selector.is_empty() {
        return None;
    }
    Some(Owner::Method(receiver.to_string(), selector))
}

/// `f(...)` where `f` is a plain identifier.
fn c_function_callee(node: Node, src: &str) -> Option<Owner> {
    let mut cursor = node.walk();
    let callee = node.children(&mut cursor).next()?;
    if callee.kind() != "identifier" {
        return None;
    }
    Some(Owner::Function(src[callee.byte_range()].to_string()))
}

/// Is this owner a `+` method? Only then is "nothing calls it in this
/// unit" a fact rather than an absence of evidence.
fn is_class_method(owner: &Owner, program: &Program) -> bool {
    let Owner::Method(class, selector) = owner else {
        return false;
    };
    program.classes.get(class).is_some_and(|info| {
        info.methods.iter().any(|m| m.selector == *selector && m.is_class_method)
    })
}

/// How many times this body runs, counted as **static call sites** rather
/// than executions -- transitively, so a factory called twice from a
/// factory called once costs two.
///
/// A body nothing in the unit calls counts 1: `main`, an entry point, a
/// method reached only by dynamic dispatch. That is the floor the whole
/// pass has always been, not a claim that it runs once.
fn multiplicity(
    owner: &Owner,
    callers: &HashMap<Owner, Vec<Option<Owner>>>,
    program: &Program,
    memo: &mut HashMap<Owner, usize>,
    stack: &mut Vec<Owner>,
) -> Result<usize, Vec<Owner>> {
    if let Some(seen) = memo.get(owner) {
        return Ok(*seen);
    }
    if stack.contains(owner) {
        /* A cycle has no finite answer, and guessing one would size a pool
         * that cannot be right. Hard error, consistent with the rule that
         * this backend never silently degrades. */
        let mut cycle = stack.clone();
        cycle.push(owner.clone());
        return Err(cycle);
    }
    stack.push(owner.clone());
    /* A body with no resolvable call site in this unit. For a **class
     * method** that means it is not called here at all, and 0 is the
     * truthful answer: a class-method receiver is always statically known
     * (see `alloc_receiver_class`), so there is no dispatch this pass
     * cannot see. Counting 1 instead is what made every program pay for
     * `src/OZQ31.m`'s seventeen `+fixedWith...` forwarders -- each
     * uncalled, each contributing a slot, so `hello_category` sized
     * OZQ31 at 16 while using no OZQ31 at all.
     *
     * Everything else keeps the floor of 1, and the asymmetry is the
     * point: an instance method can arrive through dynamic dispatch and a
     * C function can be an entry point, so "no caller here" does not mean
     * "never runs". Sizing has to over-count there, because the failure
     * mode of under-counting is a nil from an exhausted slab. */
    let floor = if is_class_method(owner, program) { 0 } else { 1 };
    let total = match callers.get(owner) {
        None => floor,
        Some(sites) if sites.is_empty() => floor,
        Some(sites) => {
            let mut sum = 0usize;
            for site in sites {
                sum += match site {
                    None => 1,
                    Some(caller) => multiplicity(caller, callers, program, memo, stack)?,
                };
            }
            sum
        }
    };
    stack.pop();
    memo.insert(owner.clone(), total);
    Ok(total)
}

impl Scan {
    /// Turn sites and call edges into per-class slot counts.
    fn resolve(
        self,
        program: &Program,
    ) -> (HashMap<String, usize>, usize, Vec<crate::model::Diagnostic>) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut diagnostics = Vec::new();
        let mut memo = HashMap::new();
        for site in &self.sites {
            let slots = match (&site.owner, site.escapes) {
                (Some(owner), true) => {
                    let mut stack = Vec::new();
                    match multiplicity(owner, &self.callers, program, &mut memo, &mut stack) {
                        Ok(n) => n,
                        Err(cycle) => {
                            let path = cycle
                                .iter()
                                .map(Owner::describe)
                                .collect::<Vec<_>>()
                                .join(" -> ");
                            diagnostics.push(crate::model::Diagnostic::new(
                                format!(
                                    "cannot size the '{}' slab: the allocation escapes through a \
                                     call cycle ({}), so the number of live instances has no \
                                     static answer. Size it explicitly with a \
                                     `/* oz-pool: {}=N */` directive or --pool-sizes {}=N",
                                    site.class, path, site.class, site.class
                                ),
                                1,
                                1,
                            ));
                            1
                        }
                    }
                }
                _ => 1,
            };
            *counts.entry(site.class.clone()).or_insert(0) += slots;
        }
        (counts, self.item_slots, diagnostics)
    }
}

/// Elements in an `@[...]`, matching
/// `emit::render_boxed_array_literal`'s own filter exactly.
pub(crate) fn array_element_count(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor).filter(|c| !matches!(c.kind(), "@" | "[" | "]" | ",")).count()
}

/// Key/value pairs in an `@{...}`, matching
/// `emit::render_boxed_dictionary_literal`'s own filter exactly.
pub(crate) fn dictionary_pair_count(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor).filter(|c| c.kind() == "dictionary_pair").count()
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
    /* Whole-string, and deliberately so. `+dynamicAlloc` (#413) is a
     * zero-argument class-method send on a literal class name, which is
     * *structurally identical* to `+alloc` here -- two children, a class
     * receiver -- so a prefix or `starts_with` test would count it and
     * reserve a slab slot for a class that never takes one. Heap-allocated
     * objects carry `_meta.heap_allocated` and go back to their heap in
     * `{Class}_oz_free`, never touching the slab (`companion.rs`). The
     * omission of `dynamicAlloc` from this comparison is the point, not an
     * oversight. */
    if selector != "alloc" {
        return None;
    }
    if program.is_class(receiver) {
        Some(receiver.to_string())
    } else {
        None
    }
}
