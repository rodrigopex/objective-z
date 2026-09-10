// SPDX-License-Identifier: Apache-2.0
//
// arc.rs - which expressions hand back a reference the caller owns.
//
// Scope-based release (see `emit::render_body_with_comments`) has to know
// what it may release. Releasing a borrowed reference is a double free;
// failing to release an owned one is a leak. So every local this decides to
// release must be provably +1, and everything else is left alone.
//
// That rule is necessary and was for a long time treated as sufficient,
// which it is not: it establishes *provenance* -- was this name initialised
// by something recognisable as +1 -- and says nothing about **escape**, or
// whether the reference is still reachable after the scope through some
// other name. A `+1` local that is `provably +1` and also aliased is
// exactly the case where releasing it is wrong, and it was a
// use-after-free rather than a leak: `Thing *b = a; return b;` released
// `a`, the only reference there was (#351). Both halves have to be asked,
// and `alias_chain` / `return_needs_retain` below are the second one.
//
// Worth stating plainly, because it is the reason this is not simply
// Clang's ARC: ARC never asks about escape, because retain-on-binding
// gives every name its own reference and makes the question moot. That
// costs an atomic pair per binding, which Clang gets back from the LLVM
// `ObjCARCOpt` pass and this backend cannot -- `oz_static_retain` and
// `oz_static_release` are ordinary C functions to GCC, and their pairs
// survive -O2, inlining and whole-program LTO alike. So what lives here is
// ARC's *optimizer*, written at the source level: emit the traffic that is
// necessary and elide the rest. The elision is the whole value, and it has
// to be sound in the leak direction rather than the corrupting one.
//
// Two questions, not one, and they have different answers.
// `is_owning_expr` asks whether an expression is +1 *by shape* -- may a
// local holding it be released at scope exit; `discards_ownership` asks
// whether throwing that value away abandons a reference nothing else
// accounts for (`discarded_owning_value`, #322). `-retain` and
// `-init...` are +1 to the first and not to the second, because the
// reference they hand back is one something else is already tracking.
//
// A cast is where the two have to be combined rather than chosen between,
// because `is_owning_expr` reads one as borrowed -- deliberately, and
// still does, since that answer also decides which methods are owning
// factories and so what every caller of one must release. `binds_ownership`
// is the combination, and it is what every *binding* site consults: +1 by
// shape, or a non-bridging cast over a reference `created_by` says is
// genuinely new. That is what lets
//
//     Thing *t = (Thing *)[Thing alloc];   /* +1, released at scope end */
//     Thing *t = (Thing *)[u init];        /* u's own +1, left alone */
//
// differ (#332), and what makes `(void)[t copy];` and `[t copy];` agree on
// whether they leak (#327). Both peels go through `value_behind_casts`, so
// the two cannot drift apart, and a *bridging* cast is looked through by
// neither.
//
// An *argument* is a third site, and it asks the discard question rather
// than a third one of its own: `owning_argument_value` is
// `discarded_owning_value` under a name that says where it is asked from
// (#328). Whether the callee retains the argument or only borrows it does
// not change the caller's obligation -- the `+1` the caller created is the
// caller's to drop -- so all that is left to decide is whether the
// reference is new, which is `created_by` again.
//
// A **receiver** is the fourth, and the first one that needs more than the
// discard question: `receiver_owning_value` asks it *and* consults the
// selector (#340). `[[Foo alloc] poke];` abandons the `+1` and must
// release it, but `[[Foo alloc] init];` hands that same reference back out
// and #322's arm already releases it -- so releasing the receiver as well
// would free one pointer twice. `accounts_for_its_receiver` is that second
// half, and note what it is *not*: being an owning selector is not being a
// pass-through one. `-copy` and an analysis-derived factory return a fresh
// object, so both references are live and both are released.
//
// Ported from the oracle's `_is_owning_expr` / `_find_owning_return_methods`
// (tools/oz_transpile/emit.py), with one improvement: the oracle's scan is a
// single pass, so a factory whose returns call *another* factory is not
// recognised. This iterates to a fixed point, which costs one more pass over
// a symbol table and catches that chain.

use std::collections::HashSet;

use tree_sitter::Node;

use crate::model::Program;

/// This module indexes `src` directly elsewhere; a named helper keeps the
/// new code readable.
fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

/// `(class, selector)` for every method whose every return path hands back a
/// +1 reference, so a caller storing the result must not retain it again and
/// *must* release it.
#[derive(Debug, Default, Clone)]
pub struct OwningMethods {
    methods: HashSet<(String, String)>,
    /// Plain top-level C functions whose every return path hands back +1.
    ///
    /// A helper like `samples/arc_demo`'s
    /// `static Sensor *createSensor(int v)` is exactly as owning as a
    /// factory method, and its callers own what it returns. Left out, the
    /// local holding its result was treated as borrowed and never released
    /// -- and the sample's own comment says otherwise ("s is released here
    /// by ARC"). On target that showed up as an MPU fault: the one-slot
    /// Sensor slab stayed occupied, the next allocation returned NULL, and
    /// `-initWithValue:` wrote through it.
    functions: HashSet<String>,
}

impl OwningMethods {
    pub fn contains(&self, class: &str, selector: &str) -> bool {
        self.methods.contains(&(class.to_string(), selector.to_string()))
    }

    /// Does the plain C function `name` return +1?
    pub fn contains_function(&self, name: &str) -> bool {
        self.functions.contains(name)
    }

    pub fn len(&self) -> usize {
        self.methods.len() + self.functions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.methods.is_empty() && self.functions.is_empty()
    }
}

/// Selectors that are +1 by convention rather than by analysis, matching
/// Objective-C's own naming rule (the "create rule"): these transfer
/// ownership whatever their body does.
fn is_owning_selector(selector: &str) -> bool {
    selector == "alloc"
        // `+allocWithHeap:` is `+alloc` with the storage coming from an
        // OZHeap, so it hands back +1 just the same. Missing from this list,
        // `samples/heap_alloc` leaked every object it allocated: nothing
        // released them, no `-dealloc` ran, and the heap's used-bytes never
        // came back down -- which the sample's own expected output
        // ("app heap after free: 0 bytes used", "Sensor dealloc") states.
        // Compiling and linking cannot catch that; only running it can.
        || selector == "allocWithHeap:"
        || selector == "new"
        || selector == "copy"
        || selector == "mutableCopy"
        || selector == "retain"
        || selector.starts_with("init")
}

/// Find every method that returns +1, iterating until the set stops growing.
pub fn analyze(source: &str, program: &Program) -> OwningMethods {
    let tree = crate::parse::parse(source);
    let mut owning = OwningMethods::default();
    loop {
        let before = owning.methods.len();
        scan_once(tree.root_node(), source, program, &mut owning);
        if owning.methods.len() == before {
            return owning;
        }
    }
}

fn scan_once(root: Node, src: &str, program: &Program, owning: &mut OwningMethods) {
    fn walk(
        node: Node,
        src: &str,
        program: &Program,
        owning: &mut OwningMethods,
        class: Option<&str>,
    ) {
        if node.kind() == "function_definition" {
            consider_function(node, src, program, owning);
            return;
        }
        let class = if node.kind() == "class_implementation" {
            // Only an @implementation has bodies to analyse.
            let (name, _, _) = crate::collect::class_header(node, src);
            if name.is_empty() {
                class
            } else {
                // Leaked into the recursion below via the owned String's
                // lifetime, so it is resolved eagerly here instead.
                return walk_impl(node, src, program, owning, &name);
            }
        } else {
            class
        };
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, src, program, owning, class);
        }
    }

    fn walk_impl(
        node: Node,
        src: &str,
        program: &Program,
        owning: &mut OwningMethods,
        class: &str,
    ) {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            if child.kind() == "implementation_definition" {
                let mut c2 = child.walk();
                let defs: Vec<Node> = child.children(&mut c2).collect();
                for def in defs {
                    if def.kind() == "method_definition" {
                        consider_method(def, src, program, owning, class);
                    }
                }
            }
        }
    }

    /// Same rule as `consider_method`, for a plain C function: an
    /// object-returning function whose every return path is owning is
    /// itself owning.
    fn consider_function(
        function: Node,
        src: &str,
        program: &Program,
        owning: &mut OwningMethods,
    ) {
        let Some(name) = function_name(function, src) else {
            return;
        };
        if owning.functions.contains(&name) {
            return;
        }
        let mut cursor = function.walk();
        let children: Vec<Node> = function.children(&mut cursor).collect();
        // Only a pointer return can carry ownership. Checked on the
        // declared type's own text, ahead of the body, so a function
        // returning a struct by value is skipped.
        let returns_pointer = children.iter().any(|c| {
            matches!(c.kind(), "pointer_declarator" | "function_declarator")
                && node_text(*c, src).contains('*')
        });
        if !returns_pointer {
            return;
        }
        let Some(body) = children.iter().find(|c| c.kind() == "compound_statement") else {
            return;
        };
        let returns = collect_returns(*body);
        if returns.is_empty() {
            return;
        }
        let all_owning = returns
            .iter()
            .all(|ret| return_hands_back_ownership(*ret, *body, src, program, owning));
        if all_owning {
            owning.functions.insert(name);
        }
    }

    fn consider_method(
        method: Node,
        src: &str,
        program: &Program,
        owning: &mut OwningMethods,
        class: &str,
    ) {
        let known: HashSet<String> = program.classes.keys().cloned().collect();
        let sig = crate::collect::extract_method_sig(method, src, class, &known);
        // Only an object-returning method can hand back ownership, and the
        // convention-named ones are already owning without analysis.
        if is_owning_selector(&sig.selector) {
            return;
        }
        if !sig.return_type.contains('*') {
            return;
        }
        let mut cursor = method.walk();
        let body = method.children(&mut cursor).find(|c| c.kind() == "compound_statement");
        let Some(body) = body else {
            return;
        };
        let returns = collect_returns(body);
        if returns.is_empty() {
            return;
        }
        // Every path must be owning. One borrowed return makes the whole
        // method +0, because the caller cannot tell the paths apart.
        // Same shape as the function case above: a factory method that
        // returns a local is just as owning as one that returns the
        // allocation directly.
        let all_owning = returns
            .iter()
            .all(|ret| return_hands_back_ownership(*ret, body, src, program, owning));
        if all_owning {
            owning.methods.insert((class.to_string(), sig.selector));
        }
    }

    walk(root, src, program, owning, None);
}

/// Does this `return` hand back a reference the caller owns?
///
/// `is_owning_expr` alone is not enough, because the idiomatic factory
/// returns a *variable*:
///
/// ```objc
/// static Sensor *createSensor(int v)
/// {
///         Sensor *s = [[Sensor alloc] init];
///         [s setValue:v];
///         return s;
/// }
/// ```
///
/// `samples/arc_demo` is built on that shape, and with the returned
/// identifier read as borrowed the function looked +0, its callers released
/// nothing, and the one-slot Sensor slab stayed occupied -- an MPU fault on
/// target at the next allocation.
///
/// So a returned identifier is followed back to its declaration, and counts
/// as owning when that declaration's initialiser is. Requiring the name to
/// be assigned nowhere else keeps the usual bias: a variable that is
/// reassigned might hold something borrowed by the time it is returned, and
/// guessing wrong in that direction is a double free, where guessing wrong
/// the other way only leaks.
///
/// A `return` is a binding site like any other, so it asks
/// `binds_ownership` rather than `is_owning_expr`: `return (Thing *)[Thing
/// alloc];` hands the caller the same +1 the uncast spelling does (#332).
/// The identifier is read from behind a cast too --
/// `emit::render_return_statement` reads the returned *name* through the
/// same peel, and the two have to agree: whichever local it decides not to
/// release is the one whose ownership this says passes to the caller.
///
/// That agreement was stated here long before it held. A returned name
/// that *aliases* the owned local is in neither side's set, so emit
/// released the owner while this reported `+0` -- nothing owned an object
/// that had already been freed (#351). The two are kept together now by
/// construction rather than by assertion: both walk `alias_chain` for the
/// resolvable case, and both call `return_needs_retain` for the case that
/// is not resolvable at all.
fn return_hands_back_ownership(
    ret: Node,
    body: Node,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> bool {
    let Some(value) = value_of_return(ret) else {
        return false;
    };
    if binds_ownership(value, src, program, owning) {
        return true;
    }
    let value = value_behind_casts(value, src);
    if value.kind() != "identifier" {
        return false;
    }
    let name = node_text(value, src);
    if is_reassigned(body, src, name) {
        return false;
    }
    if declared_initializer(body, src, name)
        .is_some_and(|init| binds_ownership(init, src, program, owning))
    {
        return true;
    }
    /* The returned name may be an *alias* of the local that owns the
     * reference rather than that local itself, and the caller is handed
     * the reference either way (#351). `emit::render_return_statement`
     * keeps whichever local this finds, so the two answers come from one
     * walk. */
    if alias_chain(body, src, name).into_iter().any(|aliased| {
        declared_initializer(body, src, &aliased)
            .is_some_and(|init| binds_ownership(init, src, program, owning))
    }) {
        return true;
    }
    /* And where the emitter retains the returned value because its
     * provenance cannot be established, the caller is handed that `+1`
     * and must release it. One predicate, both sides -- see
     * `return_needs_retain`. */
    return_needs_retain(ret, body, src, program, owning)
}

/// Must a `return` of this value retain it before the scope's releases run?
///
/// The second half of #351, and the half no alias analysis can reach.
/// `Thing *b = passthrough(a); return b;` hands back whatever the call
/// returned, and nothing here can know whether that is `a`, a different
/// object, or nothing -- so the reference cannot be identified, and the
/// owned local cannot be safely released against it. Retaining the
/// returned value is the only sound answer, and it is what ARC does: the
/// Clang AST for exactly this shape marks the call
/// `ImplicitCastExpr cast=ARCReclaimReturnedObject`, which is the retain
/// this emits.
///
/// **Both sides must call this, not merely agree with it.**
/// `emit::render_return_statement` retains when it says yes;
/// `return_hands_back_ownership` reports the function as `+1` when it says
/// yes, so callers release what was retained. Two implementations of the
/// same rule would be one drift away from a double free -- the direction
/// this whole issue was filed for.
///
/// Deliberately narrow, because a retain here is not free (see the
/// measurements in `emit::render_return_statement`). It fires only for a
/// **local with an initialiser this cannot classify**:
///
///   - a *parameter* is excluded: it carries the caller's own reference, so
///     releasing our local cannot strand it.
///   - an *ivar* read is excluded: a strong ivar store already retained,
///     so the ivar's reference outlives the scope on its own.
///   - a local that *is* the owner, or aliases one, is excluded: that is
///     mechanism one, which keeps the owner instead and costs nothing.
///
/// What is left is the opaque-call case, and returning `+0` there is what
/// hands back a freed object.
pub fn return_needs_retain(
    ret: Node,
    body: Node,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> bool {
    let Some(value) = value_of_return(ret) else {
        return false;
    };
    if binds_ownership(value, src, program, owning) {
        return false;
    }
    let value = value_behind_casts(value, src);
    if value.kind() != "identifier" {
        return false;
    }
    let name = node_text(value, src);
    if is_reassigned(body, src, name) {
        return false;
    }
    /* A local, and one whose initialiser says nothing about ownership.
     * No declaration at all means a parameter or an ivar, both excluded
     * above. */
    let Some(init) = declared_initializer(body, src, name) else {
        return false;
    };
    if binds_ownership(init, src, program, owning) {
        return false;
    }
    if alias_chain(body, src, name).into_iter().any(|aliased| {
        declared_initializer(body, src, &aliased)
            .is_some_and(|init| binds_ownership(init, src, program, owning))
    }) {
        return false;
    }
    owned_local_live_at(ret, body, src, program, owning, name)
}

/// Is some *other* local holding a `+1` at the point `ret` runs?
///
/// With none, there is nothing for the return to release and so nothing to
/// protect the returned value from -- and no retain is emitted, which is
/// what keeps every ordinary `return _ivar;` and `return borrowed;` byte
/// identical.
///
/// Read in byte order rather than by walking scope structure. That
/// over-approximates: a local declared in a sibling block that has already
/// closed is counted as live when its release has in fact already run. The
/// consequence is one unnecessary retain, balanced by the caller's release
/// -- a cost, never a corruption -- and it is the reason both sides call
/// this one function instead of each deciding for itself.
fn owned_local_live_at(
    ret: Node,
    body: Node,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
    returned: &str,
) -> bool {
    fn walk(
        node: Node,
        before: usize,
        src: &str,
        program: &Program,
        owning: &OwningMethods,
        returned: &str,
        found: &mut bool,
    ) {
        if *found {
            return;
        }
        /* A block literal's body is a separate function; its locals are
         * not live here and its releases are its own (#342). */
        if node.kind() == "block_literal" {
            return;
        }
        if node.kind() == "init_declarator" && node.end_byte() <= before {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.children(&mut cursor).collect();
            let declared = children.iter().find(|c| {
                matches!(c.kind(), "identifier" | "pointer_declarator")
            });
            let is_returned = declared
                .is_some_and(|c| node_text(*c, src).trim_start_matches('*').trim() == returned);
            if !is_returned {
                if let Some(equals) = children.iter().position(|c| c.kind() == "=") {
                    if let Some(init) = children.get(equals + 1) {
                        if binds_ownership(*init, src, program, owning) {
                            *found = true;
                            return;
                        }
                    }
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, before, src, program, owning, returned, found);
        }
    }

    let mut found = false;
    walk(body, ret.start_byte(), src, program, owning, returned, &mut found);
    found
}

/// The names `name` aliases, nearest first, following plain-identifier
/// initialisers out to the local that actually holds the reference.
///
/// `Thing *b = a;` makes `b` a second name for whatever `a` holds, so a
/// `return b` hands back `a`'s reference -- and releasing `a` on the way
/// out drops the only one there is (#351). Neither side of the release
/// decision could see that before: both matched the returned *name*
/// against the set of locals known to be `+1`, and an alias is in neither
/// set.
///
/// Both sides call this, which is what keeps them agreeing.
/// `emit::render_return_statement` walks the chain to find the local it
/// must *not* release; `return_hands_back_ownership` walks the same chain
/// to decide whether the caller is being handed a reference. The doc
/// comment on that function states the requirement, and it is a real one:
/// whichever local one of them keeps is the one whose ownership the other
/// says passes to the caller.
///
/// A reassigned name ends the chain. `b = a;` only says what `b` holds if
/// nothing else was stored into it later, and this walk has no idea which
/// assignment ran -- the usual bias applies, since guessing that a name
/// still aliases an owned local when it does not means releasing nothing
/// (a leak) while guessing the other way means releasing it twice.
///
/// Only *plain identifier* initialisers extend the chain, read from behind
/// casts by the same `value_behind_casts` every other ownership question
/// uses. A call does not: nothing here can know whether
/// `Thing *b = passthrough(a);` hands back `a`, a different object, or
/// nothing at all, so that shape is not an alias question and cannot be
/// answered by looking at names (see #351's second mechanism).
pub fn alias_chain(body: Node, src: &str, name: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = name.to_string();
    /* A bound rather than a visited-set because the chain is short by
     * construction and a cycle needs a reassignment, which already ends
     * it: `a = b; b = a;` leaves both reassigned. The bound is what makes
     * that argument unnecessary to trust. */
    for _ in 0..8 {
        if is_reassigned(body, src, &current) {
            return chain;
        }
        let Some(init) = declared_initializer(body, src, &current) else {
            return chain;
        };
        let init = value_behind_casts(init, src);
        if init.kind() != "identifier" {
            return chain;
        }
        let next = node_text(init, src).to_string();
        if next == current || chain.contains(&next) {
            return chain;
        }
        chain.push(next.clone());
        current = next;
    }
    chain
}

/// The initialiser of `name`'s declaration inside `node`, if it has one.
///
/// Read *positionally* -- the child after the `=` -- and not as "the last
/// child that is not a declarator", which is what this did until #351. The
/// difference is only visible for an initialiser that is itself a plain
/// identifier: `Thing *b = a;` has `identifier` on both sides of the `=`,
/// the old filter excluded that kind to avoid returning the declarator, and
/// so it excluded the initialiser too and answered `None`. An alias was
/// therefore invisible to every caller here -- which is why
/// `return_hands_back_ownership` looked like it followed one level of
/// indirection and structurally could not (#351).
fn declared_initializer<'a>(node: Node<'a>, src: &str, name: &str) -> Option<Node<'a>> {
    if node.kind() == "init_declarator" {
        let mut cursor = node.walk();
        let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
        let declares = children.iter().any(|c| {
            matches!(c.kind(), "identifier" | "pointer_declarator")
                && node_text(*c, src).trim_start_matches('*').trim() == name
        });
        if declares {
            let equals = children.iter().position(|c| c.kind() == "=")?;
            return children.into_iter().nth(equals + 1);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = declared_initializer(child, src, name) {
            return Some(found);
        }
    }
    None
}

/// Is `name` the target of an assignment anywhere in `node`?
fn is_reassigned(node: Node, src: &str, name: &str) -> bool {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    if node.kind() == "assignment_expression" {
        if let Some(lhs) = children.first() {
            if lhs.kind() == "identifier" && node_text(*lhs, src) == name {
                return true;
            }
        }
    }
    children.into_iter().any(|child| is_reassigned(child, src, name))
}

/// A `function_definition`'s own name, reached through however many
/// declarator layers its return type needs (`static Sensor *f(int)` nests a
/// `pointer_declarator` around the `function_declarator`).
fn function_name(function: Node, src: &str) -> Option<String> {
    fn find_declarator_identifier<'a>(node: Node<'a>, src: &str) -> Option<String> {
        if node.kind() == "function_declarator" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    return Some(node_text(child, src).to_string());
                }
                if let Some(found) = find_declarator_identifier(child, src) {
                    return Some(found);
                }
            }
            return None;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_declarator_identifier(child, src) {
                return Some(found);
            }
        }
        None
    }
    find_declarator_identifier(function, src)
}

/// Every `return_statement` in `body`, not descending into a nested block
/// literal -- that is a separate function with its own returns.
fn collect_returns<'a>(body: Node<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    fn walk<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
        if node.kind() == "block_literal" {
            return;
        }
        if node.kind() == "return_statement" {
            out.push(node);
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        for child in children {
            walk(child, out);
        }
    }
    walk(body, &mut out);
    out
}

fn value_of_return<'a>(ret: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = ret.walk();
    let children: Vec<Node<'a>> = ret.children(&mut cursor).collect();
    children.into_iter().find(|c| c.kind() != "return" && c.kind() != ";")
}

/// Does this expression hand back a reference its receiver owns?
///
/// Deliberately narrow: anything not recognised is treated as borrowed, so
/// an unrecognised shape leaks rather than double-frees. That asymmetry is
/// the whole point -- a leak is a bug, a double free is memory corruption.
///
/// The question is specifically whether this expression is +1 *by shape*.
/// It is the wrong one for a value bound to nothing: `[c retain]` and
/// `[f init]` are +1 here and must be, since a local taking one over needs
/// no retain of its own, but discarding either abandons no reference -- see
/// `discards_ownership`, which #322 added for exactly that difference.
///
/// It is also not the whole answer at a binding site, because it reads a
/// cast as borrowed: `binds_ownership` is what those sites consult (#332).
pub fn is_owning_expr(
    node: Node,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> bool {
    match node.kind() {
        // `@42` / `@3.5f` allocate an OZQ31; `@[...]`/`@{...}` allocate the
        // collection. A `@"..."` literal is a static, so releasing it is a
        // guarded no-op (see `emit::render_boxed_string_literal`) -- counting
        // it as owning keeps the rule uniform and costs nothing.
        "at_expression" => crate::emit::is_numeric_boxed_shape(node, src),
        "array_literal" | "dictionary_literal" => true,
        "string_literal" => crate::emit::is_boxed_string_literal(node),
        "parenthesized_expression" => {
            let mut cursor = node.walk();
            let inner = node.children(&mut cursor).find(|c| c.kind() != "(" && c.kind() != ")");
            inner.is_some_and(|inner| is_owning_expr(inner, src, program, owning))
        }
        "message_expression" => {
            let (receiver_class, selector) = message_target(node, src, program);
            if is_owning_selector(&selector) {
                return true;
            }
            /* A send the emitter routes through the class_id switch cannot
             * be judged from the receiver's static class: the
             * implementation that runs may be an override with a different
             * contract (#365), and assuming borrowed instead leaks
             * whenever it returns +1 (#361). Both are answered by polling
             * every implementation the send can reach -- see
             * `dispatch_ownership`, which also explains why a disagreement
             * has no caller-side remedy.
             *
             * `is_dynamically_dispatched` is the same predicate
             * `emit::render_message` consults before choosing that route,
             * so the two cannot disagree about which sends this covers.
             * An `Ambiguous` answer reads as borrowed here -- the leaking
             * direction, per the standing rule -- and
             * `emit::dynamic_dispatch_call` refuses the send outright, so
             * the leak is never actually emitted. */
            if receiver_class.is_none() || program.is_dynamically_dispatched(&selector, false) {
                return matches!(
                    dispatch_ownership(
                        program,
                        owning,
                        receiver_class.as_deref(),
                        &selector,
                        false
                    ),
                    DispatchOwnership::Owning
                );
            }
            receiver_class.is_some_and(|class| owning.contains(&class, &selector))
        }
        // A call to a plain C function that returns +1 -- see
        // `OwningMethods::functions`.
        "call_expression" => {
            let mut cursor = node.walk();
            let callee = node.children(&mut cursor).next();
            callee.is_some_and(|callee| {
                callee.kind() == "identifier" && owning.contains_function(node_text(callee, src))
            })
        }
        // A cast says nothing about ownership, and `__bridge` explicitly
        // means "not mine" -- borrowed either way, *here*.
        //
        // `binds_ownership` (#332) and `discards_ownership` (#327) both
        // look through a non-bridging cast; this function still does not,
        // and the difference is not stylistic. Those two read the send
        // behind the cast through `created_by`, which excludes `-retain`
        // and follows an `-init...` back to its receiver. Answering yes
        // here instead would skip that check and hand every +1-named
        // selector through, and it is not just about one local:
        // `consider_method` classifies a method as an owning factory only
        // when every return path is `is_owning_expr`, so widening it here
        // changes what every caller of such a method is told to release.
        // `Thing *t = (Thing *)[u init];` then releases `t` and `u`, which
        // are one pointer -- measured, and a use-after-free under ASan.
        _ => false,
    }
}

/// Does binding this expression's value to a strong slot -- a local, an
/// ivar, an array element, a `return` -- take over a +1 reference nothing
/// else accounts for?
///
/// This is the question every *binding* site asks, and it is
/// `is_owning_expr` plus exactly one shape: a non-bridging cast over a
/// reference `created_by` says is genuinely new (#332).
///
/// The cast has to be looked through, because it changes the static type
/// and nothing else -- `Thing *t = (Thing *)[Thing alloc];` leaked where
/// the uncast spelling did not. It has to be looked through *narrowly*,
/// through `discards_ownership` rather than by widening `is_owning_expr`,
/// because the reference behind a cast is not always new:
///
/// ```objc
/// Thing *u = [Thing alloc];
/// Thing *t = (Thing *)[u init];   /* -init hands back u's own +1 */
/// ```
///
/// `-init...` consumes its receiver's +1 and hands it back, so releasing
/// `t` as well as `u` frees one pointer twice. `created_by` is what
/// separates the two: it excludes `-retain` outright and follows an
/// `-init...` send back to its receiver instead of trusting the selector's
/// name. A bridging cast is looked through by neither -- see
/// `value_behind_casts`.
///
/// Nothing is added where no cast was peeled, so every uncast shape keeps
/// exactly the answer `is_owning_expr` gave it.
pub fn binds_ownership(
    node: Node,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> bool {
    if is_owning_expr(node, src, program, owning) {
        return true;
    }
    let behind = value_behind_casts(node, src);
    if behind.id() == node.id() {
        return false;
    }
    discards_ownership(behind, src, program, owning)
}

/// Of the convention-named +1 selectors, the ones whose reference is a
/// *newly created* object rather than one the send was handed.
///
/// The distinction only matters when the result is thrown away, and there
/// it is the whole question. Two of `is_owning_selector`'s entries hand
/// back a reference something else is already accounting for:
///
///   - `-retain` returns its own receiver, and a bare `[c retain];` is the
///     manual-retain/release idiom whose balancing `[c release];` is
///     written by hand -- `samples/smp_shared` does exactly that, twice per
///     iteration. Releasing the discarded result would undo the retain and
///     free the object out from under the sample. Real ARC has nothing to
///     match here either: it makes an explicit `retain` a compile error.
///   - `-init...` *consumes* the receiver's +1 and hands it back, so the
///     reference is the receiver's. In `[[Foo alloc] init]` that receiver
///     is a temporary nothing tracks, and the result is genuinely
///     abandoned; in
///
///     ```objc
///     Foo *f = [Foo alloc];
///     [f init];
///     ```
///
///     it is `f`, which scope-based ARC already releases at the end of the
///     block. Releasing the discarded result too would be a double free.
///     So an `init` send is followed back to its receiver rather than
///     trusted on its name.
fn creates_reference(selector: &str) -> bool {
    matches!(selector, "alloc" | "allocWithHeap:" | "new" | "copy" | "mutableCopy")
}

/// The +1 reference throwing `node`'s value away would abandon, or None
/// when discarding it abandons nothing.
///
/// The node handed back is the one to release, which is not always `node`
/// itself: `(void)[t copy]` abandons the `[t copy]`, and
/// `oz_static_release((struct OZObject *)((void)(...)))` is not C. Emit
/// wraps *this* node, so the two answers -- whether to release, and what
/// -- come from one place and cannot drift apart (#327).
pub fn discarded_owning_value<'a>(
    node: Node<'a>,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> Option<Node<'a>> {
    if discards_ownership(node, src, program, owning) {
        Some(value_behind_casts(node, src))
    } else {
        None
    }
}

/// The +1 reference an *argument* hands a message send that nothing else
/// will release, or None when the argument is borrowed (#328).
///
/// `[self setFoo:[Foo new]];` binds nothing and discards nothing, so
/// neither scope-based release nor `discarded_owning_value` reached it and
/// the `+1` from `+new` was never released. A synthesized strong setter
/// *retains* its argument on top of that, so the object ended at +2 with
/// one release ever owed.
///
/// This is a *third site* asking the question `discards_ownership` already
/// answers, and it is deliberately the same predicate rather than a
/// parallel one -- unlike the fourth, a receiver, where the same question
/// is necessary but not sufficient (`receiver_owning_value`, #340). The caller's obligation does not depend on what the
/// callee does with the argument: a callee that stores it strongly retains
/// it (`render_strong_ivar_assign`, and a synthesized setter), and one that
/// merely borrows it retains nothing -- either way the `+1` the *caller*
/// created is the caller's to drop. So the only question left is the one
/// this module keeps asking: is this reference genuinely new, or one
/// something else already accounts for? `created_by` answers it, which is
/// what keeps `[self setFoo:[c retain]];` and `[self setFoo:[u init]];`
/// alone -- releasing either would be a double free, not a leak.
///
/// The node handed back is the one to release, read out from behind any
/// casts, exactly as for a discard: the temporary the call site holds the
/// reference in takes *that* node's value, so what is released and what is
/// released *once* come from one place.
pub fn owning_argument_value<'a>(
    arg: Node<'a>,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> Option<Node<'a>> {
    discarded_owning_value(arg, src, program, owning)
}

/// The +1 reference a send's **receiver** abandons, or None when the
/// receiver is borrowed or the send itself accounts for that reference
/// (#340).
///
/// `[[Foo alloc] poke];` is the shape: the allocation has no name, so no
/// scope-exit release reaches it, and the send's *value* is `void`, so
/// `discarded_owning_value` sees nothing to release. It was the last
/// position in the sweep #322, #327, #332 and #328 worked through, and it
/// is the one that cannot be answered by the discard question alone.
///
/// Releasing a receiver unconditionally is a double free, and
/// `[[Foo alloc] init];` is the counterexample: `-init` consumes the
/// receiver's `+1` and hands it back, and #322's discarded-statement arm
/// **already releases that** by following the `-init...` send back to its
/// receiver through `created_by`. So the selector has to be consulted as
/// well as the receiver, which is what `accounts_for_its_receiver` is for
/// and why this is not `owning_argument_value` with a different argument.
///
/// Being an *owning* selector is not the same as accounting for the
/// receiver, and this is the distinction worth stating: `-copy` and an
/// analysis-derived factory build a **fresh** object, so the receiver's
/// `+1` is abandoned exactly as `-poke`'s is and both references are
/// released -- one here, one by #322's arm. Only the four selectors that
/// consume or hand back the *receiver's own* reference are excluded.
///
/// A `-performSelector:` whose selector resolves statically is the same
/// send by another spelling and gets the same answer, exactly as in
/// `discards_ownership`. When it does not resolve, the receiver is still
/// released: whatever the run-time selector turns out to be, the reference
/// the *call site* created is one reference, and releasing it once is
/// right for any callee that retains what it keeps -- which is the same
/// obligation `owning_argument_value` reasons from.
pub fn receiver_owning_value<'a>(
    send: Node<'a>,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> Option<Node<'a>> {
    if send.kind() != "message_expression" {
        return None;
    }
    let parts = crate::emit::parse_message(send, src);
    let selector = statically_performed_selector(send, src).unwrap_or(parts.selector);
    if accounts_for_its_receiver(&selector) {
        return None;
    }
    discarded_owning_value(parts.receiver, src, program, owning)
}

/// Does a send of `selector` consume its receiver's reference, or hand that
/// same reference back out as its value?
///
/// The four that do, and what each would cost if it were missing here:
///
/// - `init...` hands the `+1` back out, and #322's arm releases it at the
///   receiver -- releasing here too frees one pointer twice;
/// - `retain` hands the receiver back at +2, and `created_by` excludes it
///   from being a new reference at all, so the balancing release is the
///   author's;
/// - `release` *is* the release, so a second one is the second free;
/// - `dealloc` has already torn the object down when the send returns.
///
/// Nothing else belongs here. A selector left out leaks; a selector wrongly
/// added would corrupt, so the list is a reading of the four selectors
/// `emit::render_message` and `staticbar` already treat specially rather
/// than a guess at which methods might keep their receiver.
fn accounts_for_its_receiver(selector: &str) -> bool {
    matches!(selector, "retain" | "release" | "dealloc") || selector.starts_with("init")
}

/// Does throwing this expression's value away abandon a +1 reference that
/// nothing else releases?
///
/// `is_owning_expr` answers a different question -- may a *local* holding
/// this value be released at scope exit -- and it is the wrong one for a
/// result bound to nothing: it says yes to `[f init]` and `[c retain]`,
/// where the reference belongs to the receiver. Getting that wrong here is
/// a double free rather than a leak, which is the direction this module
/// exists to avoid, so the two selectors that hand back a reference
/// something else accounts for are separated out (`creates_reference`) and
/// an `init` send is resolved through its receiver.
///
/// Everything else is `is_owning_expr` unchanged: a boxed literal, a
/// collection literal, a call to an owning C function and an
/// analysis-derived owning method all produce a fresh object. That an
/// owning *method* cannot be returning `self` is not an assumption --
/// `consider_method` classifies one only when every return path is
/// `is_owning_expr`, and a bare `self` is not.
///
/// The value is read out from behind whatever the discard is written
/// behind first -- see `value_behind_casts`, which is why `(void)[t copy];`
/// answers the same as `[t copy];` (#327).
fn discards_ownership(
    node: Node,
    src: &str,
    program: &Program,
    owning: &OwningMethods,
) -> bool {
    let node = value_behind_casts(node, src);
    if node.kind() != "message_expression" {
        return is_owning_expr(node, src, program, owning);
    }
    let (receiver_class, selector) = message_target(node, src, program);
    // A `-performSelector:` whose selector resolves statically is the same
    // send by another spelling, so it gets the same answer. When it does
    // not resolve, the selector is a run-time value and no +1 can be seen:
    // borrowed, as ever, is the safe reading.
    if let Some(performed) = statically_performed_selector(node, src) {
        return created_by(program, &performed, receiver_class.as_deref(), owning);
    }
    if selector.starts_with("init") {
        let parts = crate::emit::parse_message(node, src);
        return discards_ownership(parts.receiver, src, program, owning);
    }
    created_by(program, &selector, receiver_class.as_deref(), owning)
}

/// The value behind the parentheses and casts an expression may be written
/// behind (#327).
///
/// `(void)[t copy];` has to answer the same as `[t copy];`, and
/// `Thing *t = (Thing *)[Thing alloc];` the same as `Thing *t = [Thing
/// alloc];`. Neither is a shape someone stumbles into: `(void)expr` is the
/// idiom for "I am throwing this away on purpose", so it is the spelling
/// *most* likely to have been written by someone who thought about the
/// result, and least likely to be a mistake -- which is exactly why the
/// two spellings must not differ in whether they leak. The alternative,
/// rejecting the cast, would make a deliberate discard a hard error while
/// leaving the careless one to compile.
///
/// Two callers peel with this and no others do: `discards_ownership`
/// (#322/#327) and `binds_ownership` (#332). Both then read the send
/// behind the peel through `created_by`, which is what keeps the peel
/// safe. It is deliberately *not* done in `is_owning_expr`: that answer
/// bypasses `created_by` and also decides which methods `consider_method`
/// classifies as owning factories, and so what every caller of one is told
/// to release.
///
/// A *bridging* cast is the one cast that speaks about ownership rather
/// than about type, and is not looked through.
/// `(__bridge_retained void *)[t copy];` hands the reference to a
/// non-Objective-C holder, so releasing it would pull the object out from
/// under that holder -- a double free, the direction this module exists to
/// avoid. `__bridge` and `__bridge_transfer` are held back with it: not
/// because either is known to be unsafe, but because there is no
/// CoreFoundation here for any of the three to bridge to, so leaving all
/// of them borrowed costs nothing observable and keeps the bias exact
/// rather than resting on a reading of a bridge this project does not
/// have.
pub(crate) fn value_behind_casts<'a>(node: Node<'a>, src: &str) -> Node<'a> {
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
    let inner = match node.kind() {
        "parenthesized_expression" => {
            children.into_iter().find(|c| c.kind() != "(" && c.kind() != ")")
        }
        "cast_expression" if !is_bridging_cast(node, src) => cast_value(children),
        _ => None,
    };
    match inner {
        Some(inner) => value_behind_casts(inner, src),
        None => node,
    }
}

/// The expression a `cast_expression` casts, or None when the node is one
/// of the other shapes the grammar files under that kind -- a compound
/// literal, whose value is an `initializer_list` and not an expression at
/// all. `emit::render_cast_expression` splits the same two cases the same
/// way, and by the same rule: the last child that is neither a paren nor
/// the type.
fn cast_value<'a>(children: Vec<Node<'a>>) -> Option<Node<'a>> {
    if !children.iter().any(|c| c.kind() == "type_descriptor") {
        return None;
    }
    children
        .into_iter()
        .rev()
        .find(|c| c.kind() != ")" && c.kind() != "(" && c.kind() != "type_descriptor")
}

/// Does this cast carry an ARC bridging qualifier?
///
/// Named exactly rather than matched on a `__` prefix: the grammar files
/// every Objective-C ownership word under `type_qualifier` alongside C's
/// own `const`/`volatile`, and only these three say anything about a
/// reference crossing out of Objective-C's hands. `__strong`, `__weak`,
/// `__unsafe_unretained` and `__autoreleasing` describe *storage*, which a
/// discarded value has none of.
fn is_bridging_cast(node: Node, src: &str) -> bool {
    let mut cursor = node.walk();
    let Some(descriptor) = node.children(&mut cursor).find(|c| c.kind() == "type_descriptor")
    else {
        return false;
    };
    let mut cursor = descriptor.walk();
    let qualifiers: Vec<Node> = descriptor.children(&mut cursor).collect();
    qualifiers.into_iter().any(|child| {
        child.kind() == "type_qualifier"
            && matches!(
                node_text(child, src).trim(),
                "__bridge" | "__bridge_transfer" | "__bridge_retained"
            )
    })
}

/// Does a send of `selector` to a receiver of `class` create a reference
/// its caller owns and nothing else accounts for?
/// What a **dynamically dispatched** send hands its caller.
///
/// The ownership of such a send cannot come from the receiver's static
/// class, because the implementation that runs may be an override with a
/// different contract (#365), and it cannot be assumed borrowed either,
/// because that leaks whenever the implementation returns `+1` (#361).
/// Both are the same question over different sets, so both are answered
/// here: poll every implementation the send can *reach*
/// (`Program::reachable_implementors`) and see whether they agree.
///
/// Why unanimity and not something cleverer: which implementor runs is a
/// run-time fact, and no caller-side action can paper over the
/// disagreement. A `+1` result must be released exactly once and a `+0`
/// one never, so the two differ by one release -- and adding a retain
/// shifts *both* by one, leaving the difference intact. That is why the
/// retain-when-unprovable trick #351 uses at a `return` does not transfer
/// to a call site: there, retaining creates a new reference the caller can
/// own; here, the question is whether an existing one was handed over.
/// Measured against Clang too -- a protocol send, a `+1` class send and a
/// `+0` class send all carry the identical `ARCReclaimReturnedObject`,
/// because ARC's callee autoreleases and its caller always reclaims, a
/// convention that needs the pool this target does not have. So the AST
/// cannot answer it either.
#[derive(Debug, PartialEq, Eq)]
pub enum DispatchOwnership {
    /// Every reachable implementation hands back `+1`.
    Owning,
    /// Every reachable implementation hands back a borrowed reference, or
    /// the selector reaches nothing at all.
    Borrowed,
    /// They disagree, so no answer is right for the caller. Carries one
    /// owning and one borrowing class, for the diagnostic.
    Ambiguous { owning: String, borrowed: String },
}

pub fn dispatch_ownership(
    program: &Program,
    owning: &OwningMethods,
    receiver: Option<&str>,
    selector: &str,
    is_class_method: bool,
) -> DispatchOwnership {
    /* Convention beats analysis, and applies whatever the class: `alloc`
     * and the create-rule selectors are +1 from every implementor by
     * definition, so there is nothing to disagree about. */
    if creates_reference(selector) {
        return DispatchOwnership::Owning;
    }
    if selector == "retain" || selector.starts_with("init") {
        return DispatchOwnership::Borrowed;
    }
    let reachable = program.reachable_implementors(receiver, selector, is_class_method);
    let mut first_owning: Option<String> = None;
    let mut first_borrowed: Option<String> = None;
    for class in reachable {
        if owning.contains(&class, selector) {
            first_owning.get_or_insert(class);
        } else {
            first_borrowed.get_or_insert(class);
        }
    }
    match (first_owning, first_borrowed) {
        (Some(owning), Some(borrowed)) => DispatchOwnership::Ambiguous { owning, borrowed },
        (Some(_), None) => DispatchOwnership::Owning,
        _ => DispatchOwnership::Borrowed,
    }
}

fn created_by(
    program: &Program,
    selector: &str,
    class: Option<&str>,
    owning: &OwningMethods,
) -> bool {
    if creates_reference(selector) {
        return true;
    }
    if selector == "retain" || selector.starts_with("init") {
        return false;
    }
    /* A send that the emitter will route through the class_id switch has
     * to be judged from every implementation it can reach, not from the
     * receiver's static class (#365) and not as borrowed by default
     * (#361). `is_dynamically_dispatched` is the same question
     * `emit::render_message` asks before choosing that route, so the two
     * cannot disagree about which sends this applies to. */
    if class.is_none() || program.is_dynamically_dispatched(selector, false) {
        return matches!(
            dispatch_ownership(program, owning, class, selector, false),
            DispatchOwnership::Owning
        );
    }
    class.is_some_and(|class| owning.contains(class, selector))
}

/// The selector a `-performSelector:` send performs, when the source says
/// so exactly: a `@selector(...)` literal at the call site, or a local
/// declared once from one and never reassigned.
///
/// `SEL` is a real value type here, so the argument can be anything --
/// a parameter, a field, the result of a call. Only these two spellings
/// are readings of the source rather than guesses, and anything else stays
/// unresolved.
fn statically_performed_selector(node: Node, src: &str) -> Option<String> {
    let parts = crate::emit::parse_message(node, src);
    if !matches!(
        parts.selector.as_str(),
        "performSelector:" | "performSelector:withObject:" | "performSelector:withObject:withObject:"
    ) {
        return None;
    }
    selector_value_of(*parts.args.first()?, src)
}

/// The selector literal `node` evaluates to, or None.
fn selector_value_of(node: Node, src: &str) -> Option<String> {
    if node.kind() == "parenthesized_expression" {
        let mut cursor = node.walk();
        let inner = node.children(&mut cursor).find(|c| c.kind() != "(" && c.kind() != ")")?;
        return selector_value_of(inner, src);
    }
    if node.kind() == "selector_expression" {
        return crate::collect::selector_literal_name(node, src);
    }
    if node.kind() != "identifier" {
        return None;
    }
    // A named local, read out of its own declaration. Declared exactly
    // once, so an inner block shadowing the name cannot be mistaken for
    // the outer one, and assigned nowhere, so what the declaration says is
    // what the send performs. The initializer must be the literal itself:
    // following a chain of identifiers would have to guard against
    // `SEL s = s;`, and nothing writes that.
    let scope = enclosing_scope(node)?;
    let name = node_text(node, src);
    if declaration_count(scope, src, name) != 1 || is_reassigned(scope, src, name) {
        return None;
    }
    let init = declared_initializer(scope, src, name)?;
    if init.kind() != "selector_expression" {
        return None;
    }
    crate::collect::selector_literal_name(init, src)
}

/// The `method_definition` or `function_definition` enclosing `node`.
fn enclosing_scope<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut scope = node.parent();
    while let Some(n) = scope {
        if matches!(n.kind(), "method_definition" | "function_definition") {
            return Some(n);
        }
        scope = n.parent();
    }
    None
}

/// How many declarations or parameters inside `node` introduce `name`.
fn declaration_count(node: Node, src: &str, name: &str) -> usize {
    if matches!(node.kind(), "declaration" | "parameter_declaration")
        && declares_name(node, name, src)
    {
        return 1;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().map(|child| declaration_count(child, src, name)).sum()
}

/// The receiver's class (when statically known) and the selector of a
/// message send, for looking the send up in `OwningMethods`.
fn message_target(node: Node, src: &str, program: &Program) -> (Option<String>, String) {
    let mut cursor = node.walk();
    let children: Vec<Node> =
        node.children(&mut cursor).filter(|c| c.kind() != "[" && c.kind() != "]").collect();
    if children.is_empty() {
        return (None, String::new());
    }
    let selector = selector_of(&children, src);
    let receiver_text = &src[children[0].byte_range()];
    // A class-name receiver is a class-method send; a nested message send is
    // resolved through its own selector's declaring class. Anything else
    // (a variable) is left unresolved: the analysis stays conservative.
    if program.is_class(receiver_text) {
        return (Some(receiver_text.to_string()), selector);
    }
    if children[0].kind() == "message_expression" {
        let (inner_class, _) = message_target(children[0], src, program);
        return (inner_class, selector);
    }
    // A variable receiver, resolved from its *declaration* rather than
    // guessed. Left unresolved, an owning instance method called on a
    // variable hands back +1 that nothing releases: `[a sub:b]` in
    // `foundation/q31_basic` leaked an OZQ31 on every call, because
    // `OZQ31 *a` is not a class name and the send therefore looked
    // borrowed however owning `-sub:` was known to be. Found by running the
    // corpus under LeakSanitizer through this backend for the first time.
    //
    // Both forms below are exact readings of the source, not inferences,
    // which matters more here than usual: the standing bias is that an
    // unrecognised shape must *leak* rather than double-free, so widening
    // what counts as owning is the dangerous direction. `self` is the class
    // whose `@implementation` encloses the send; a named local or parameter
    // is whatever its declaration says. Neither can be wrong about the
    // receiver's static type.
    if receiver_text == "self" {
        if let Some(class) = enclosing_impl_class(node, src) {
            return (Some(class), selector);
        }
    }
    if children[0].kind() == "identifier" {
        if let Some(class) = declared_class_of(receiver_text, node, src, program) {
            return (Some(class), selector);
        }
    }
    (None, selector)
}

/// The class whose `@implementation` encloses `node`, for a `self` receiver.
fn enclosing_impl_class(node: Node, src: &str) -> Option<String> {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if n.kind() == "class_implementation" {
            let (name, _, _) = crate::collect::class_header(n, src);
            if !name.is_empty() {
                return Some(name);
            }
            return None;
        }
        cur = n.parent();
    }
    None
}

/// The class a named receiver was declared as, searching the enclosing
/// method or function: its parameter list first, then the body's
/// declarations. Returns None unless the name is declared exactly once with
/// a type that is a known class, so an ambiguous or unknown spelling stays
/// unresolved rather than being assumed.
fn declared_class_of(
    name: &str,
    node: Node,
    src: &str,
    program: &Program,
) -> Option<String> {
    let mut scope = node.parent();
    while let Some(n) = scope {
        if matches!(n.kind(), "method_definition" | "function_definition") {
            break;
        }
        scope = n.parent();
    }
    let scope = scope?;

    let mut found: Option<String> = None;
    let mut count = 0usize;
    collect_declared_types(scope, name, src, program, &mut found, &mut count);
    if count == 1 {
        found
    } else {
        None
    }
}

/// Walk `node` for declarations and parameters naming `name`, recording the
/// class each says it has and how many such declarations were seen.
fn collect_declared_types(
    node: Node,
    name: &str,
    src: &str,
    program: &Program,
    found: &mut Option<String>,
    count: &mut usize,
) {
    if matches!(node.kind(), "declaration" | "parameter_declaration") && declares_name(node, name, src)
    {
        let (ty, _) = crate::collect::extract_type_and_stars(node, src);
        let bare = ty.trim().trim_start_matches("struct ").trim();
        if program.is_class(bare) {
            *found = Some(bare.to_string());
        }
        *count += 1;
        return;
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children {
        collect_declared_types(child, name, src, program, found, count);
    }
}

/// Does this declaration or parameter introduce `name`?
fn declares_name(node: Node, name: &str, src: &str) -> bool {
    fn any_identifier(node: Node, name: &str, src: &str) -> bool {
        if node.kind() == "identifier" && &src[node.byte_range()] == name {
            return true;
        }
        // A declarator's own name only: do not descend into an initialiser,
        // where the same identifier may merely be *read*.
        if node.kind() == "init_declarator" {
            let mut c = node.walk();
            let kids: Vec<Node> = node.children(&mut c).collect();
            return kids.first().is_some_and(|d| any_identifier(*d, name, src));
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();
        children.into_iter().any(|c| any_identifier(c, name, src))
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    children.into_iter().any(|c| {
        matches!(
            c.kind(),
            "init_declarator" | "pointer_declarator" | "identifier" | "function_declarator"
        ) && any_identifier(c, name, src)
    })
}

fn selector_of(children: &[Node], src: &str) -> String {
    if children.len() == 2 {
        return src[children[1].byte_range()].to_string();
    }
    let mut selector = String::new();
    let mut i = 1;
    while i < children.len() {
        if children[i].kind() == "identifier"
            && children.get(i + 1).map(|n| n.kind()) == Some(":")
        {
            selector.push_str(&src[children[i].byte_range()]);
            selector.push(':');
            i += 2;
        } else {
            i += 1;
        }
    }
    selector
}
