// SPDX-License-Identifier: Apache-2.0
//
// staticbar.rs - accept/reject scan for the static subset.
//
// Philosophy carried over from OZ-091 Track A: never silently degrade or
// best-effort a construct outside the static bar. Anything not explicitly
// supported is a named, located hard error.

use std::collections::HashMap;
use std::collections::HashSet;

use tree_sitter::Node;

use crate::model::{ClassInfo, Diagnostic, Program};

/// Selectors the emitter answers itself, at the call site, from facts the
/// whole-program view already has -- so they never become C functions and
/// can never be overridden.
///
/// `emit::render_message` intercepts each of these before dispatch
/// resolution, which means an `@implementation` defining one would be
/// silently ignored: every call site would keep the compile-time answer
/// and the body would never run. Silently ignoring a method someone wrote
/// is exactly the degradation this module exists to prevent, so defining
/// one is a hard, located error instead (see `check_method_body`).
/// `render_prototype` skips them for the matching reason -- declaring a C
/// function that is never defined invites a call that fails at link time,
/// which is the shape of the `[X class]` defect in #226.
pub const INTRINSIC_SELECTORS: &[&str] = &[
    "class",
    "isMemberOfClass:",
    "isKindOfClass:",
    "conformsToProtocol:",
    "respondsToSelector:",
    "performSelector:",
    "performSelector:withObject:",
    "performSelector:withObject:withObject:",
];

const LOOP_KINDS: &[&str] = &["for_statement", "while_statement", "do_statement"];

/// Is this `@protocol(...)` the direct argument of a
/// `-conformsToProtocol:` message?
///
/// The only position where a protocol name resolves to something --
/// `emit::render_protocol_literal` turns it into that protocol's
/// conformance bitmap, which `oz_conforms` reads.
fn is_conforms_to_protocol_argument(node: Node, src: &str) -> bool {
    let Some(parent) = node.parent() else { return false };
    parent.kind() == "message_expression" && message_selector(parent, src) == "conformsToProtocol:"
}

fn node_text<'a>(node: Node, src: &'a str) -> &'a str {
    &src[node.start_byte()..node.end_byte()]
}

fn err(diags: &mut Vec<Diagnostic>, src: &str, node: Node, message: impl Into<String>) {
    diags.push(Diagnostic::spanning(message, src, node.start_byte()..node.end_byte()));
}

/// A rejection split into its three parts: what is wrong, why, and what
/// to do instead.
///
/// One diagnosis, at most one reason, and any number of remedies -- the
/// ARC rejections carry three.
struct Rejection {
    message: String,
    note: Option<String>,
    help: Vec<String>,
}

/// `err` for a rejection that has a reason and remedies to offer.
///
/// Routed through the same `Diagnostic::spanning` as `err`, so the two
/// cannot drift on how a span is recorded.
fn err_detailed(diags: &mut Vec<Diagnostic>, src: &str, node: Node, rejection: Rejection) {
    let mut d = Diagnostic::spanning(rejection.message, src, node.start_byte()..node.end_byte());
    if let Some(note) = rejection.note {
        d = d.with_note(note);
    }
    for help in rejection.help {
        d = d.with_help(help);
    }
    diags.push(d);
}

/// The selector of a `message_expression`, whatever shape its receiver has.
///
/// Delegates to `emit::parse_message`, which is the point: this function
/// used to answer the question a second time and answered it wrongly. It
/// found the receiver by taking the first `identifier` child and skipping
/// it, which is only correct when the receiver *is* a bare identifier. For
/// `[self->_ivar foo]`, `[arr[0] foo]`, `[[Thing alloc] foo]` and
/// `[(Thing *)t foo]` the receiver is a `field_expression`,
/// `subscript_expression`, `message_expression` or `cast_expression`, so the
/// skip consumed the *selector's* identifier instead and the function
/// returned `""` (#435).
///
/// `emit::parse_message` reads the same node correctly -- filter the
/// brackets and `children[0]` is the receiver however it is spelled -- and
/// `pools::class_method_callee` had already written that extraction inline a
/// third time. One question, one answer now.
///
/// Returns `""` for a node with fewer than two non-bracket children, which
/// `parse_message` would index past. A well-formed `message_expression`
/// always has a receiver and a selector piece; a node inside a syntax error
/// need not, and this runs on user input.
pub(crate) fn message_selector(node: Node, src: &str) -> String {
    let mut cursor = node.walk();
    let n = node.children(&mut cursor).filter(|c| c.kind() != "[" && c.kind() != "]").count();
    if n < 2 {
        return String::new();
    }
    crate::emit::parse_message(node, src).map(|p| p.selector).unwrap_or_default()
}

struct MethodScope<'a> {
    class_ivars: &'a HashSet<String>,
    /// The ivars the emitter manages as **strong slots**, which is a subset
    /// of `class_ivars`: `Program::owned_object_ivar_names`, the same list
    /// `render_strong_ivar_assign` and `render_strong_array_element_assign`
    /// gate on. An `__unsafe_unretained` ivar, or one of a type Clang did
    /// not call an owned object, is not here -- both of those lower to a
    /// plain C store that releases nothing, so a loop accumulates into them
    /// rather than overlapping at two (#423).
    owned_object_ivars: &'a HashSet<String>,
    /// Object locals ARC manages as strong variables, so that an overwrite
    /// releases what was there (`emit::managed_object_locals`). An
    /// allocation stored into one of these is bounded at a single live
    /// instance however many times the loop runs, which is what lets the
    /// loop rule below tell *reassignment* apart from *accumulation*.
    arc_managed_locals: &'a HashSet<String>,
    locals: HashSet<String>,
    /// `__block`-qualified locals (tree-sitter-objc parses `__block` as a
    /// `type_qualifier` child of the `declaration` node -- confirmed
    /// against the vendored grammar, there is no dedicated node kind for
    /// it). Mirrors oz_transpile's BlocksAttr promotion-to-static: these
    /// are exempt from the capture check in `find_capture` below, since
    /// emit.rs hoists them to file-scope statics rather than leaving them
    /// as real stack locals a block would need to close over.
    block_locals: HashSet<String>,
    /// The slab sizes this program will be built with, so the loop rule can
    /// ask whether a class has the slots a shape needs (#433).
    ///
    /// `None` where no sizing is available -- the pure `transpile(source)`
    /// form has no `PoolSizes` to give -- and `None` keeps the old, stricter
    /// answer. That is the safe direction: a missing size refuses a shape
    /// that might have fitted, where assuming capacity would hand the second
    /// allocation a full slab and a nil the program never checks.
    pools: Option<&'a crate::pools::PoolSizes>,
    /// Static type of every name visible in this body, by which a stored
    /// value's class resolves. Emit's own `EmitCtx::scope`, plus this
    /// body's parameters -- **passed rather than rebuilt**, so the bar
    /// resolves a receiver through the same map the emitter resolves it
    /// through. A second resolver here is how the bar and the lowering
    /// come to answer different questions (#423, #433).
    ///
    /// Empty where no sizing was supplied, which resolves nothing and so
    /// keeps the stricter answer.
    types: HashMap<String, String>,
}

/// What the loop rule needs in order to ask a **capacity** question rather
/// than pass a verdict on a shape.
///
/// One struct rather than two parameters because neither half answers it:
/// a pool size with no resolved class, and a class with no pool sizes, both
/// fall back to the rejection. `None` is the pure `transpile(source)` form,
/// which has no `PoolSizes` to give.
pub struct Sizing<'a> {
    pub pools: &'a crate::pools::PoolSizes,
    pub types: &'a HashMap<String, String>,
}

/// `@synchronized` lowers to an explicit `oz_spin_lock` / `oz_spin_unlock`
/// pair around the body (see `emit::render_synchronized_statement`), so a
/// jump out of the body would skip the unlock and leave the lock held
/// forever. Rather than silently emitting that deadlock, reject the jump
/// and say how to restructure.
///
/// `return` is exempt: `emit::render_return_statement` replays the pending
/// unlock ahead of it, so an early return works (as it does in the
/// oracle -- `tests/behavior/cases/synchronized/early_return.m`).
/// `break`/`continue` are only a problem when they escape the body; one
/// belonging to a loop or switch *inside* the body is fine, so the walk
/// stops treating them as escaping once it descends into one.
fn check_synchronized_body(sync_node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    fn walk(node: Node, src: &str, in_nested_breakable: bool, diags: &mut Vec<Diagnostic>) {
        let escaping = match node.kind() {
            "goto_statement" => Some("goto"),
            "break_statement" if !in_nested_breakable => Some("break"),
            "continue_statement" if !in_nested_breakable => Some("continue"),
            _ => None,
        };
        if let Some(keyword) = escaping {
            err(
                diags,
                src,
                node,
                format!(
                    "'{}' inside @synchronized would skip the unlock and leave the lock held \
                     (the static subset emits an explicit oz_spin_lock/oz_spin_unlock pair, not a \
                     scope guard) -- move the value out to a local, end the @synchronized block, \
                     then '{}'",
                    keyword, keyword
                ),
            );
            return;
        }
        // A loop or switch inside the body captures its own
        // break/continue, so those no longer escape.
        let captures_break = matches!(
            node.kind(),
            "for_statement" | "while_statement" | "do_statement" | "switch_statement"
        );
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(child, src, in_nested_breakable || captures_break, diags);
        }
    }

    let mut cursor = sync_node.walk();
    let body =
        sync_node.children(&mut cursor).find(|c| c.kind() == "compound_statement");
    if let Some(body) = body {
        walk(body, src, false, diags);
    }
}

/// Is this owning expression merely the **receiver** of another owning
/// send, so that the outer one carries the same reference?
///
/// `[[Foo alloc] init]` creates one object, not two: `-init` consumes its
/// receiver's `+1` and hands it back. Reporting both would give two
/// diagnostics for one allocation, and reporting only the inner one would
/// start the escape walk from an expression that is not the thing stored.
/// So the inner is skipped and the outer carries it.
///
/// A *non*-owning outer send is a different matter and must not skip:
/// `[[Foo alloc] poke]` abandons the receiver's reference after the send,
/// which is precisely the shape the group machinery releases inside the
/// iteration -- and precisely the shape this rule used to refuse.
fn is_owning_receiver_of_owning_send(node: Node, src: &str, program: &Program) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != "message_expression" {
        return false;
    }
    let mut c = parent.walk();
    let parts: Vec<Node> = parent
        .children(&mut c)
        .filter(|n| n.kind() != "[" && n.kind() != "]")
        .collect();
    if parts.first().map(|n| n.id()) != Some(node.id()) {
        return false;
    }
    crate::arc::is_owning_expr(parent, src, program, &program.owning_methods)
}

/// Why a `+1` reference created inside a loop cannot be served by the
/// slab's **one slot per allocation site**, or `None` when it can.
///
/// This replaces two proxies and a selector-name test, and the whole point
/// is that all three were standing in for one question: *does this
/// reference outlive the iteration, and if it is kept, is the previous one
/// released before the next is allocated?*
///
/// Measured, on a deliberately one-slot pool, four iterations each:
///
/// | destination | reused? | released before next alloc? | slots | run |
/// | --- | --- | --- | --- | --- |
/// | managed local | yes | **yes** | 1 | 4/4 objects |
/// | ivar / global, store cannot read it | yes | **yes** | 1 | 4/4 objects |
/// | ivar / global, store reads it | yes | **no** | **2** | 1/4, then nil |
/// | array element, constant index, store cannot read it | yes | **yes** | 1 | 4/4 objects |
/// | array element, constant index, store reads it | yes | **no** | **2** | refused |
/// | array element, varying index | **no** | n/a | loop bound | unbounded |
///
/// The ivar overlap was called *inherent* here, on the grounds that the new
/// value has to be evaluated before the old one is released or
/// `_ivar = [_ivar retain]` would free a live object. That is true only of a
/// store that **reads** the ivar. `_ivar = [Thing alloc]` does not, and
/// `render_strong_local_assign` had been releasing first on exactly that
/// condition for locals since #234 while the ivar path evaluated the new
/// value first unconditionally -- so an ivar needed two slots for a store a
/// local served with one, and an `-init` that allocates into an ivar could
/// not run twice on its own sizing (#405).
///
/// So the arm narrows rather than disappears, and
/// `overlapping_unless_released_first` asks the emitter which it is. The
/// refused shape is refused because **two objects are briefly live and one
/// slab slot cannot hold both**, and that is now the whole of the reason.
///
/// It used to be more than that, and the change is worth recording because
/// it moves what a bigger pool could do. Until #424 the `Unsupported`
/// lowering pushed its temporary's *initialiser* through `ctx.pre_stmts`,
/// which a loop lifts above itself, so the temporary read the destination
/// once while it was still nil and every iteration released that same
/// stale pointer -- accepting the shape would have miscompiled, not merely
/// exhausted the pool. #424 made the lowering shared
/// (`emit::render_overlapping_strong_store`) and split it: `pre_stmts`
/// carries a bare declaration, and the assignment is the comma
/// expression's first operand, inside the loop. A declaration with no
/// initialiser evaluates nothing, so lifting it reorders nothing.
///
/// The consequence for *this* rule is a pure capacity refusal, measured:
/// with the predicate temporarily relaxed, `_thing = [_thing dup]` over
/// four iterations runs correctly on a pool of two -- five allocations,
/// five frees, no nil -- and is short of slots on one. So a bigger pool
/// would genuinely serve this shape, which is exactly what
/// `LoopEscape::OverlappingStore`'s message used to advise and cannot
/// deliver, because this check never reads `PoolSizes` (#425). Making it
/// pool-aware is now sound where it previously was not; it is a
/// behavioural change and deliberately not made here.
///
/// #405 reached two destination spellings of three and left the subscript
/// one answering `OverlappingStore` unconditionally, so an array-element
/// store that *is* lowered release-first --
/// `render_strong_array_element_assign` has emitted that shape for a `+1`
/// right-hand side that does not read the element since #405, the same as
/// the ivar path -- stayed refused on a one-slot pool it in fact fits
/// (#423). All three spellings now go through `assigned_slot_name` and
/// then through the one predicate, which is also what closed the fourth
/// site: `self->_ivar = [_ivar dup]` was *accepted*, because the old
/// extractor answered `self` and the right-hand side does not mention
/// `self`.
///
/// The two shapes the old rule conflated, kept from the helper this
/// replaced:
///
/// ```objc
/// /* reassignment -- bounded at one live instance */
/// Counter *c;
/// for (...) { c = [Counter alloc]; }
///
/// /* accumulation -- genuinely N live instances */
/// for (...) { [arr addObject:[Counter alloc]]; }
/// ```
///
/// The second is `Accumulates`: the array keeps every one and nothing
/// releases anything, so the per-site count is a floor the program walks
/// straight through.
///
/// `None` covers every position where the reference dies with the
/// statement, which is now every operand position -- a send's receiver or
/// argument (#340, #328), a discarded result (#322), and a controlling
/// expression (#376). Those were refused before this change even though
/// the emitter released them inside the iteration: proved by the same
/// shape spelled through a factory, which the old selector-name test
/// could not see, running 8 allocations on one slot with every object
/// live.
enum LoopEscape {
    /// Kept in a slot that *is* reused, but whose previous value is
    /// released only after the new one exists. Bounded at two, not one.
    OverlappingStore(&'static str, PoolAdvice),
    /// Kept in a destination that differs per iteration, so nothing is
    /// released and live instances accumulate to the loop's own bound.
    Accumulates(&'static str),
    /// Handed to the caller, so the iteration does not end its life at
    /// all.
    Returned,
}

/// Whether raising a pool lifts an `OverlappingStore` rejection, and if
/// not, **why not** -- decided where both facts are known rather than
/// re-derived in the message.
///
/// #425's defect was one sentence of advice that changed nothing when
/// taken. The guard against repeating it is not a rule about wording: it
/// is that the only arm which offers the pool is the one that has already
/// resolved the class and read its size, so the advice is a consequence of
/// the check rather than a claim beside it.
enum PoolAdvice {
    /// The class resolved and has fewer than two slots, so raising it to
    /// two genuinely lifts this rejection -- the same source is then
    /// accepted. Carries the class, so the directive can name it.
    RaiseTo(String),
    /// The bar could not name the slab this allocation draws from -- an
    /// `id`-typed receiver, a C factory, a selector declared nowhere -- so
    /// it cannot promise that any size lifts the rejection, and must not
    /// suggest one.
    ClassUnresolved,
    /// This destination's store is not lowered through a liftable
    /// temporary, so no size lifts it. The array-element arm is the case:
    /// `emit::render_strong_array_element_assign` answers
    /// `LocalStore::Unsupported` with a located error and no temporary at
    /// all.
    LoweringCannotUseIt,
}

impl LoopEscape {
    /// The located message. It names the destination and why one slot
    /// cannot serve it, rather than listing workarounds for a reason that
    /// may not apply -- the old wording claimed the reference "escapes the
    /// iteration" even for the shapes that were being refused wrongly.
    ///
    /// **Every remedy here has to be one the author can actually take**,
    /// which is what #425 was: both of the messages below ended by
    /// suggesting a bigger pool, and taking that suggestion changed
    /// nothing. This check has no pool awareness at all -- it never reads
    /// `PoolSizes` -- so it cannot know the directive was added, and
    /// measured on the issue's own example the rejection stood unchanged at
    /// `Foo=1`, `Foo=2` and `Foo=8`.
    ///
    /// Making it pool-aware instead was the issue's preferred answer, and
    /// what that answer is worth changed twice while this was being
    /// written, so the current state is worth being exact about.
    ///
    /// What still reaches `OverlappingStore` from
    /// `overlapping_unless_released_first` is `LocalStore::Unsupported` --
    /// the store that reads its own destination, which the emitter lowers
    /// through a temporary. #423 is why that is the only thing left: the
    /// shapes that release first are accepted at one slot, so they never
    /// get here. Until #424 that remaining shape could not be accepted at
    /// *any* pool size, because `ctx.pre_stmts` lifted the temporary's
    /// initialiser out of the loop and every iteration released a stale
    /// pointer -- pool-awareness would have turned a refusal into a
    /// miscompile. #424's shared lowering pushes a bare declaration and
    /// assigns inside the comma expression, so that hazard is gone and
    /// **pool-awareness is now sound for this arm** -- measured: with the
    /// predicate relaxed, the shape runs correctly on a pool of two, five
    /// allocations and five frees, and is short of slots on one.
    ///
    /// **#433 implemented it, so the sentence that stood here is now
    /// false and has gone.** The record is kept as a chain rather than
    /// rewritten, because each link was true when written and it is the
    /// sequence that is instructive: the message first claimed the shape
    /// would be wrong at any pool size (#424 falsified that); #425
    /// narrowed it to the true, narrower fact that *this check* never read
    /// `PoolSizes`, and noted the remedy would be right again "the day
    /// this check can read the size"; #433 is that day. What changed is an
    /// argument, not a sentence -- the reason the advice was false expired,
    /// and it expired because #424 changed the lowering, not because
    /// anyone rewrote the wording.
    ///
    /// So the pool is offered again, under one condition #425 could not
    /// have met: it is offered **only** by the arm that has already
    /// resolved the class and found fewer than two slots (`PoolAdvice`).
    /// Advice the checker cannot honour is the defect; advice that *is* the
    /// checker's own finding cannot be.
    ///
    /// `Accumulates` had the same false remedy for a different reason, and
    /// that reason has not moved: the loop's bound is not something this
    /// pass knows, so "size the pool for the loop's own bound" is not a
    /// number the author can be told and not one any directive could
    /// express for a loop whose trip count is dynamic.
    ///
    /// **A remedy also has to be writable in ARC source, and that ruled out
    /// the first replacement tried here.** `Accumulates` briefly advised
    /// "release each instance before the next iteration allocates", which
    /// is unwritable: ARC is always on -- `-fobjc-arc` is passed on every
    /// path that produces the Clang AST oracle -- and under it an explicit
    /// `[x release]` is a Clang error, so such a source never reaches
    /// `oz2c` at all. That `emit::released_by_hand` exists, and that
    /// `oz2c` tolerates manual retain/release as a feature of its own, is
    /// not a licence to *recommend* it. The advice is now the two things an
    /// ARC author can actually write: overwrite one local per iteration and
    /// let ARC release the previous instance, or allocate once outside the
    /// loop. Same lesson as the pool half, one step further in: a remedy
    /// must be one the author can express **and** the checker can honour.
    ///
    /// So both keep only the advice that works. The pool remains the right
    /// tool for a *site* that needs more than one live instance, and since
    /// #433 it is that tool for `OverlappingStore` too -- named, and only
    /// where raising it is what the bar is waiting for.
    fn describe(&self, what: &str) -> String {
        match self {
            LoopEscape::OverlappingStore(dest, advice) => {
                let head = format!(
                    "{what} inside a loop is stored into {dest}, which needs **two** slab \
                     slots rather than one: the store releases the previous object only after \
                     the new one exists, so both are briefly live."
                );
                /* The local always works, so it is always offered; the
                 * pool is offered only where it is the thing this check is
                 * waiting for. */
                let local = "Bind it to a local declared before the loop and store that -- a \
                             local's previous value is released *before* the next allocation, \
                             so one slot serves it";
                match advice {
                    PoolAdvice::RaiseTo(class) => format!(
                        "{head} '{class}' has fewer than two. Give it two \
                         (`/* oz-pool: {class}=2 */`, or `--pool-sizes {class}=2`), or else \
                         {local}",
                        head = head,
                        class = class,
                        local = local[0..1].to_lowercase() + &local[1..]
                    ),
                    PoolAdvice::ClassUnresolved => format!(
                        "{head} {local}. Raising a pool is not offered here because this \
                         check could not tell which class's slab the allocation draws from, \
                         so it cannot promise any size lifts the rejection",
                        head = head,
                        local = local
                    ),
                    PoolAdvice::LoweringCannotUseIt => format!(
                        "{head} {local}. Raising a pool does not lift this rejection: this \
                         destination's store is not lowered through a temporary, so no slab \
                         size makes the shape work",
                        head = head,
                        local = local
                    ),
                }
            }
            LoopEscape::Accumulates(dest) => format!(
                "{what} inside a loop is stored into {dest}, so each iteration keeps its own \
                 instance and nothing is released; the static subset sizes one slab slot per \
                 allocation site and cannot bound how many the loop needs. Store it in a local \
                 that each iteration overwrites, so ARC releases the previous instance, or \
                 allocate outside the loop and reuse the one instance. Raising this class's \
                 pool does not lift this either: the loop's bound is not a number this pass \
                 knows"
            ),
            LoopEscape::Returned => format!(
                "{what} inside a loop is returned, so the iteration does not end its life and \
                 one slab slot cannot serve the next one. Allocate it outside the loop, or \
                 return on the first iteration that produces a value"
            ),
        }
    }
}

/// Walk outward from a `+1` expression to whatever finally keeps it.
///
/// The climb through a `message_expression` **only when the expression is
/// its receiver** is the same rule the helper this replaced used, and
/// for the same reason: `-init` and friends hand the receiver's own
/// reference back, so the send's value is still the object in question.
/// As an *argument* it is borrowed and the statement's group releases it,
/// which is why that direction stops with `None`.
///
/// Reaching the enclosing block without passing a store or a `return` is
/// the confined case: nothing kept the reference, so it dies with the
/// statement.
/// How to name the allocation in a diagnostic: with its class when that
/// resolves, and without when it does not.
///
/// **This was the last hand-extraction left in this file, and it was the
/// one `pools::class_method_callee` warns about.** That function's comment
/// records the history: taking `children[0]` after filtering brackets was
/// "the third place" to do it, and the copy in
/// `staticbar::message_selector` "got it wrong for eight years' worth of
/// receiver shapes (#435)". `message_selector` was routed through
/// `emit::parse_message` then; this site was not, because it only fed a
/// message and a wrong word in a diagnostic is cosmetic.
///
/// It stops being cosmetic the moment anything reads the answer. Measured
/// on the old extraction:
///
/// | expression | yielded |
/// |---|---|
/// | `[Foo alloc]` | `Foo` |
/// | `[[Foo alloc] init]` | `Foo` |
/// | `makeThing()` | `makeThing()` |
/// | `[self make]` | `self` |
/// | `@[1, 2]` | `@[1, 2]` |
///
/// So three of five named something that is not a class, and
/// `PoolSizes::for_class` answers `0` for each -- which fails *safe*, and
/// would have given pool-awareness to one syntactic form while silently
/// withholding it from the others for no principled reason.
///
/// Routed through `parse_message` and `program.is_class`, the same way
/// `class_method_callee` is. An unresolvable receiver now yields "an
/// allocation" rather than a fabricated class name, which is a better
/// diagnostic as well as a resolvable input.
fn allocation_of(node: Node, src: &str, program: &Program, scope: &MethodScope) -> String {
    match stored_class(node, src, program, scope) {
        Some(class) => format!("an allocation of '{}'", class),
        None => "an allocation".to_string(),
    }
}

fn loop_escape(
    node: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
) -> Option<LoopEscape> {
    let mut cur = node;
    loop {
        let Some(parent) = cur.parent() else {
            return None;
        };
        match parent.kind() {
            "parenthesized_expression" | "cast_expression" | "unary_expression"
            | "binary_expression" | "conditional_expression" => {
                cur = parent;
            }
            "message_expression" => {
                let mut c = parent.walk();
                let parts: Vec<Node> = parent
                    .children(&mut c)
                    .filter(|n| n.kind() != "[" && n.kind() != "]")
                    .collect();
                match parts.first() {
                    /* The receiver: the send hands this same reference on,
                     * so keep climbing to find who ends up with it. */
                    Some(receiver) if receiver.id() == cur.id() => cur = parent,
                    /* An argument: borrowed, and released by the group the
                     * statement is wrapped in. */
                    _ => return None,
                }
            }
            /* A plain C call's argument -- same reasoning as a send's. */
            "argument_list" => return None,
            /* A local declaration. Fresh per iteration, or ARC-managed and
             * overwritten with the previous released first; both are one
             * slot (measured). */
            "init_declarator" | "declaration" => return None,
            "assignment_expression" => {
                return assignment_escape(parent, cur, src, program, scope)
            }
            "return_statement" => return Some(LoopEscape::Returned),
            /* Nothing kept it: it dies with the statement. */
            "expression_statement" | "compound_statement" => return None,
            /* A controlling expression: released per evaluation, inside the
             * iteration (#376). */
            "if_statement" | "while_statement" | "do_statement" | "for_statement"
            | "switch_statement" => return None,
            _ => cur = parent,
        }
    }
}

/// Does one slab slot serve this store, because the emitter releases the
/// previous value *before* evaluating the new one?
///
/// The bar asks `emit::classify_store` rather than re-deriving the answer,
/// because the two have to agree exactly: a shape the emitter lowers
/// release-first needs one slot and must be accepted, and a shape it
/// lowers through a temporary keeps the previous object alive while the new
/// one is allocated, so it needs two. Refusing the first over-rejects;
/// accepting the second hands the second allocation a full slab, which is
/// a nil the program never checks. One predicate is the only thing that
/// keeps them from drifting apart (#405).
///
/// The second half of that used to read "*and* has that temporary lifted
/// out of the loop by `ctx.pre_stmts` ... accepting the second
/// miscompiles", and #424 retired that reason rather than this rule: the
/// shared lowering now pushes a bare declaration and assigns inside the
/// comma expression, so nothing is evaluated outside the loop. The
/// rejection stands on capacity alone, and it still has to be the emitter
/// that is asked -- a predicate keyed on the store's shape is what makes
/// the bar and the lowering answer one question, whatever the lowering
/// happens to be this month.
///
/// Before #405 a strong ivar always evaluated the new value first, so every
/// store to one was refused here. Two of the three shapes now need no
/// temporary:
///
///   - `LocalStore::Owning` -- a `+1` that does not read the ivar. One slot.
///   - `LocalStore::BorrowedIdent` -- a plain identifier, named twice safely.
///   - `LocalStore::Unsupported` -- everything else, `_x = [_x copy]`
///     included: `+1`, but it reads the ivar, so the new value has to exist
///     before the old one can go. Still two slots, still refused.
/// Whether a second slab slot can serve the overlap at all -- a question
/// about the **lowering** the destination's store goes through, not about
/// the pool.
///
/// Keyed on the destination kind because that is what selects the
/// lowering, and the two lowerings differ in kind rather than in degree.
enum SecondSlotServes {
    /// `emit::render_overlapping_strong_store` -- the ivar, `self->` ivar,
    /// local and file-scope spellings. Since #424 it pushes a bare
    /// `struct ROOT *prev;` through `ctx.pre_stmts` and assigns inside the
    /// comma expression, so two briefly-live instances are correct code
    /// wherever two slots exist.
    Yes,
    /// `emit::render_strong_array_element_assign` -- for
    /// `LocalStore::Unsupported` it emits a located error and no temporary
    /// at all, so the shape does not work at *any* pool size and a pool
    /// size must not be allowed to lift its rejection.
    ///
    /// Verified against the tree rather than carried over from #433's
    /// text: that arm is `emit.rs`'s `if kind == LocalStore::Unsupported {
    /// ctx.err(...) }`, ahead of the two lowerings that do emit an
    /// expression.
    No,
}

/// The class whose slab this `+1` expression draws its slot from, when it
/// resolves.
///
/// This is **not** `allocated_class`, which the diagnostic uses: that one
/// answers "which class is named as the receiver of the allocation", and
/// for `[_thing dup]` the answer is nothing, because `_thing` is an ivar.
/// The capacity question is about the slab the new object lands in, and for
/// a send that is the declared return type of the selector -- which is how
/// `Foo *_thing = [_thing dup]` draws from `Foo`'s slab although no `Foo`
/// is named in the expression. Keeping the two apart is the point: naming
/// them the same thing is what made the first cut of #433 accept nothing it
/// was filed to accept.
///
/// The three literal desugars mirror `pools::walk_sites` exactly, since
/// that is the pass whose slots are being counted.
fn stored_class(
    value: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
) -> Option<String> {
    match value.kind() {
        "array_literal" => return Some("OZArray".to_string()),
        "dictionary_literal" => return Some("OZDictionary".to_string()),
        "at_expression" if crate::emit::is_numeric_boxed_shape(value, src) => {
            return Some("OZNumber".to_string());
        }
        /* The wrappers `loop_escape` already walks *up* through, walked
         * down here for the same reason: they change no one's class, so a
         * question answered differently on either side of a cast or a pair
         * of parentheses is being asked about the spelling rather than
         * about the reference. */
        "parenthesized_expression" | "cast_expression" | "unary_expression" => {
            let mut cursor = value.walk();
            let inner = value
                .children(&mut cursor)
                .find(|c| c.is_named() && c.kind() != "type_descriptor")?;
            return stored_class(inner, src, program, scope);
        }
        /* Both arms draw from the same slab or the question has no single
         * answer. `cond ? [Foo make] : [_ivar dup]` is the shape the loop
         * rule sees most often in `LocalStore::Unsupported`, and answering
         * `None` for it meant the commonest refusal got the vaguest
         * message. */
        "conditional_expression" => {
            let mut cursor = value.walk();
            let arms: Vec<Node> = value
                .children(&mut cursor)
                .filter(|c| c.is_named())
                .skip(1)
                .collect();
            let [then, otherwise] = arms.as_slice() else {
                return None;
            };
            let a = stored_class(*then, src, program, scope)?;
            let b = stored_class(*otherwise, src, program, scope)?;
            return if a == b { Some(a) } else { None };
        }
        "message_expression" => {}
        _ => return None,
    }
    let mut cursor = value.walk();
    if value.children(&mut cursor).filter(|c| c.kind() != "[" && c.kind() != "]").count() < 2 {
        return None;
    }
    let Some(parts) = crate::emit::parse_message(value, src) else {
        return None;
    };
    let receiver = receiver_class(parts.receiver, src, program, scope)?;
    let to_class = parts.receiver.kind() == "identifier"
        && program.is_class(&src[parts.receiver.byte_range()]);
    /* Through `find_defining_class`, so an inherited declaration resolves
     * -- `+alloc` and `-copy` are declared on the root class, not on the
     * class the send names. */
    let defining = crate::emit::find_defining_class(program, &receiver, &parts.selector, to_class)?;
    let (ret, returns_instancetype) =
        crate::emit::method_return_type(program, &defining, &parts.selector, to_class)?;
    if returns_instancetype {
        /* `instancetype` is the *receiver's* class, which is the whole
         * reason `[[Foo alloc] init]` draws from `Foo`'s slab and not from
         * the root's. */
        return Some(receiver);
    }
    class_named_by(&ret, program)
}

/// The static class of a message-send receiver: a literal class name is
/// itself, and anything else is looked up in the scope's type map.
///
/// **The emitter resolves one shape more than this does, deliberately.**
/// Since #534 `self` in a `+` method resolves there to the enclosing
/// class; here it is in neither `program.is_class` nor `scope.types`, so
/// it answers `None`. That is the strict direction rather than a hole:
/// `stored_class`'s one caller reads `None` as
/// `PoolAdvice::ClassUnresolved` and **keeps** the rejection, so a
/// `[[self alloc] init]` escaping a loop is refused for want of a
/// resolved class instead of being waved through on a capacity it was
/// never checked against. The doc comment on `allocation_of` lists
/// `[self make]` among the receivers that name no class for the same
/// reason. Resolving it here would need the enclosing class *and* which
/// side of it this body is on, neither of which `MethodScope` carries;
/// worth doing when a real program is refused by it, not before.
fn receiver_class(
    receiver: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
) -> Option<String> {
    match receiver.kind() {
        /* `[[Foo alloc] init]`: the receiver is itself a send, and its
         * class is the class that send produces. */
        "message_expression" => stored_class(receiver, src, program, scope),
        "identifier" => {
            let name = &src[receiver.byte_range()];
            if program.is_class(name) {
                return Some(name.to_string());
            }
            class_named_by(scope.types.get(name)?, program)
        }
        _ => None,
    }
}

/// The class a declared type names, or `None` when it names none.
///
/// `id`, `OZObject *` and a C scalar all reach `None` deliberately: `id`
/// resolves to no single slab, and a value the pool does not size cannot be
/// the subject of a capacity question. `None` keeps the rejection.
fn class_named_by(ty: &str, program: &Program) -> Option<String> {
    let bare = ty.trim().trim_end_matches(|c: char| c == '*' || c.is_whitespace()).trim();
    let bare = bare.strip_prefix("struct ").unwrap_or(bare).trim();
    if program.is_class(bare) {
        return Some(bare.to_string());
    }
    None
}

fn overlapping_unless_released_first(
    name: &str,
    value: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
    what: &'static str,
    second_slot: SecondSlotServes,
) -> Option<LoopEscape> {
    match crate::emit::classify_store(name, value, src, program) {
        crate::emit::LocalStore::Owning | crate::emit::LocalStore::BorrowedIdent => None,
        /* Two objects are briefly live, so the shape needs two slots -- and
         * since #433 that is a question about capacity rather than a verdict
         * on the shape. Where the class resolves and has the slots, accept
         * it; where it does not, refuse as before.
         *
         * **What made this answerable is #424, not this change.** The
         * rejection used to rest on two reasons, and the load-bearing one
         * was that the whole capture was lifted out of the loop by
         * `ctx.pre_stmts` -- so accepting the shape at *any* pool size
         * would have miscompiled. #424 split that: the lowering now pushes
         * a bare `struct ROOT *prev;` (`emit.rs:3136`) and assigns inside
         * the comma expression, and a declaration with no initialiser
         * evaluates nothing, so lifting it above a loop reorders nothing.
         * This function's own doc has recorded since then that "#424
         * retired that reason rather than this rule -- the rejection stands
         * on capacity alone". Re-verified against the tree rather than
         * quoted: both the push and that sentence are still there.
         *
         * The class must be *resolved*, never extracted from text. The
         * extraction this file used until #433 answered `_slot` for
         * `[_slot copy]` and `makeThing()` for a C factory, and
         * `for_class` returns 0 for both -- so keying acceptance on it
         * would have granted pool-awareness to one syntactic form and
         * silently withheld it from others. `allocated_class` is the
         * resolved answer; an unresolved one keeps the rejection. */
        crate::emit::LocalStore::Unsupported => {
            if matches!(second_slot, SecondSlotServes::No) {
                return Some(LoopEscape::OverlappingStore(
                    what,
                    PoolAdvice::LoweringCannotUseIt,
                ));
            }
            match stored_class(value, src, program, scope) {
                None => Some(LoopEscape::OverlappingStore(what, PoolAdvice::ClassUnresolved)),
                Some(class) => {
                    let slots = scope.pools.map(|p| p.for_class(&class)).unwrap_or(0);
                    if slots >= 2 {
                        None
                    } else {
                        Some(LoopEscape::OverlappingStore(what, PoolAdvice::RaiseTo(class)))
                    }
                }
            }
        }
    }
}

/// A store into an **ivar**, whichever of the three spellings named it.
///
/// The one place the ivar arms agree, so that the owned-slot gate and the
/// release-first question are each asked once. `scope.class_ivars` is every
/// ivar the class has; only `scope.owned_object_ivars` are the ones the
/// emitter lowers as strong slots, and the bar was asking the first while
/// the emitter gated on the second -- so an `__unsafe_unretained` ivar was
/// accepted with `OverlappingStore`'s reasoning ("raise the pool and both
/// live copies fit") when in truth nothing ever releases the previous
/// value and no pool size bounds the loop at all (#423).
fn ivar_slot_escape(
    name: &str,
    value: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
    what: &'static str,
    second_slot: SecondSlotServes,
) -> Option<LoopEscape> {
    if !scope.owned_object_ivars.contains(name) {
        return Some(LoopEscape::Accumulates("an ivar that is not an owned strong slot"));
    }
    overlapping_unless_released_first(name, value, src, program, scope, what, second_slot)
}

/// The strong slot an assignment's left side names, by the **same rule the
/// emitter keys its store on** (`emit::assigned_ivar_name`).
///
/// `find_last_identifier` stood in for this and answered a different
/// question. In `self->_ivar` the field is a `field_identifier`, not an
/// `identifier`, so the last *identifier* under that node is `self` -- and
/// the bar then asked `classify_store` about `"self"` while the emitter
/// asked about `"_ivar"`. The two agree only by accident, because
/// `classify_store`'s question is whether the right-hand side *mentions the
/// name*, and they disagree on exactly the store that reads the ivar under
/// its bare name -- the one spelling that hides `self` from the right-hand
/// side:
///
/// ```objc
/// for (i = 0; i < 4; i++) { self->_ivar = [_ivar dup]; }
/// ```
///
/// That was accepted, and the emitter lowered it through a temporary --
/// the shape that keeps the previous object alive across the new one's
/// allocation. So this was not an over-rejection being lifted but a
/// rejection failing to fire, reached through the one spelling the arm
/// could not see: on a pool sized for one live instance the second
/// allocation gets a full slab and hands back nil, which nothing checks.
///
/// When this was found the symptom was worse than that. The lowering then
/// pushed the temporary's initialiser through `ctx.pre_stmts`, so a loop
/// lifted it above itself, captured the ivar once while still nil and
/// released that stale pointer every iteration -- a miscompile, and the
/// emitted C is in `docs/STATUS.md`. #424 fixed the lowering generally
/// (`emit::render_overlapping_strong_store` declares the temporary through
/// `pre_stmts` and assigns it inside the comma expression), so the
/// consequence of the gap is now a nil rather than a stale release.
/// **The gap itself is unchanged, and so is this fix**: the bar and the
/// emitter still have to key on the same name, or this spelling is
/// accepted on a pool that cannot serve it.
///
/// Fourth site of the cause #351, #352, #360 and #405 each fixed once: a
/// decision keyed on a spelling rather than on the reference (#423). The
/// answer each time is to route every spelling through one function, which
/// is why the subscript arm below extracts its array's name with this
/// rather than growing a rule of its own.
///
/// It cannot *call* `emit::assigned_ivar_name`, which needs an `EmitCtx`
/// the bar does not have, so it mirrors it; the local-shadowing check that
/// function does through `ctx.locals` is the caller's here, against
/// `scope.locals`.
fn assigned_slot_name(left: Node, src: &str) -> Option<String> {
    if left.kind() == "identifier" {
        return Some(node_text(left, src).to_string());
    }
    if left.kind() != "field_expression" {
        return None;
    }
    let mut cursor = left.walk();
    let children: Vec<Node> = left.children(&mut cursor).collect();
    /* `self->_x` only. Dot syntax is a property store, which the caller
     * handles separately because a setter's ordering is its own question. */
    if !children.iter().any(|c| c.kind() == "->") {
        return None;
    }
    let object = children.first()?;
    if object.kind() != "identifier" || node_text(*object, src) != "self" {
        return None;
    }
    let field = children.last()?;
    if field.kind() != "field_identifier" {
        return None;
    }
    Some(node_text(*field, src).to_string())
}

/// Which slot an assignment keeps the reference in, and whether one slab
/// slot can serve it.
fn assignment_escape(
    assignment: Node,
    value: Node,
    src: &str,
    program: &Program,
    scope: &MethodScope,
) -> Option<LoopEscape> {
    let mut c = assignment.walk();
    let parts: Vec<Node> = assignment.children(&mut c).collect();
    /* Only the value side keeps it; reaching here from the *left* means
     * the expression was part of the destination, not the thing stored. */
    if parts.last().map(|n| n.id()) != Some(value.id()) {
        return None;
    }
    let Some(lhs) = parts.first() else {
        return None;
    };
    match lhs.kind() {
        "identifier" => {
            let name = node_text(*lhs, src);
            if scope.arc_managed_locals.contains(name) {
                /* Overwritten each iteration, previous released *first* --
                 * one slot, measured at 4/4 on a one-slot pool. */
                return None;
            }
            /* A local ARC declined to manage -- one whose store shape it
             * could not support (see `arc_strong_locals`). Nothing
             * releases the previous value, so this accumulates rather than
             * overlapping at two. */
            if scope.locals.contains(name) {
                return Some(LoopEscape::Accumulates("a local ARC does not manage"));
            }
            if scope.class_ivars.contains(name) {
                return ivar_slot_escape(
                    name,
                    value,
                    src,
                    program,
                    scope,
                    "an ivar",
                    SecondSlotServes::Yes,
                );
            }
            /* A file-scope variable, which ARC manages as a strong slot the
             * same way an ivar is (#359) -- including the release-first
             * store, since `render_strong_local_assign` is the lowering for
             * all three kinds of strong slot. */
            overlapping_unless_released_first(
                name,
                value,
                src,
                program,
                scope,
                "a file-scope variable",
                SecondSlotServes::Yes,
            )
        }
        /* `self->_x`. Routed through the same extractor the emitter keys
         * its store on, for the same reason the arms above share one
         * predicate (#423). */
        "field_expression" => match assigned_slot_name(*lhs, src) {
            Some(name) => {
                ivar_slot_escape(&name, value, src, program, scope, "an ivar", SecondSlotServes::Yes)
            }
            /* Dot syntax, or a field of something that is not `self`. A
             * property store sends the setter, and a synthesized setter
             * retains the new value before releasing the old -- which is
             * the two-slot overlap by construction, whatever the right-hand
             * side reads. */
            None => {
                Some(LoopEscape::OverlappingStore("an ivar", PoolAdvice::LoweringCannotUseIt))
            }
        },
        "subscript_expression" => {
            /* A constant index names the same element every iteration, so
             * it behaves like an ivar; anything else varies, and varying is
             * what accumulates. */
            let mut sc = lhs.walk();
            let parts: Vec<Node> =
                lhs.children(&mut sc).filter(|n| !matches!(n.kind(), "[" | "]")).collect();
            let index = parts.get(1);
            if !index.is_some_and(|i| i.kind() == "number_literal") {
                return Some(LoopEscape::Accumulates("an array element chosen per iteration"));
            }
            /* The array's own name, by the same rule as the two arms above,
             * so that both `_arr[0]` and `self->_arr[0]` reach the one
             * predicate -- `render_strong_array_element_assign` extracts it
             * with `assigned_ivar_name` and then asks `classify_store`
             * about the *array*, not the element, so this has to ask about
             * the same name or the two answer different questions. */
            let Some(name) = parts.first().and_then(|recv| assigned_slot_name(*recv, src)) else {
                return Some(LoopEscape::OverlappingStore(
                    "one element of an array ivar",
                    PoolAdvice::LoweringCannotUseIt,
                ));
            };
            /* Only an *ivar* array is a strong slot the emitter manages: a
             * local or parameter array has no scope-exit release to pair
             * with and a file-scope one is not in
             * `owned_object_ivar_names`, so `render_strong_array_element_assign`
             * declines all three and the store lowers to a plain C one that
             * releases nothing. A local of the same name shadows the ivar,
             * exactly as in C and exactly as `assigned_ivar_name` treats
             * it; `ivar_slot_escape` then applies the owned-slot gate the
             * other two arms apply. */
            if scope.locals.contains(&name) || !scope.class_ivars.contains(&name) {
                return Some(LoopEscape::Accumulates("an array element that is not an ivar"));
            }
            ivar_slot_escape(
                &name,
                value,
                src,
                program,
                scope,
                "one element of an array ivar",
                SecondSlotServes::No,
            )
        }
        _ => Some(LoopEscape::Accumulates("a destination this pass cannot bound")),
    }
}


fn walk_for_reject(
    node: Node,
    src: &str,
    program: &Program,
    scope: &mut MethodScope,
    in_loop: bool,
    fresh_decl: bool,
    diags: &mut Vec<Diagnostic>,
) {
    match node.kind() {
        "try_statement" => {
            err(diags, src, node, "@try/@catch is not supported in the static subset (exception handling requires runtime unwinding info this backend does not generate)");
            return;
        }
        "synchronized_statement" => {
            check_synchronized_body(node, src, diags);
        }
        "message_expression" => {
            /* Any expression that *creates* a `+1`, not the literal
             * `alloc` spelling. Keying on the selector name let every
             * other way of producing one through untouched -- a class's
             * own `+new`, `-copy`, and any analysis-derived factory -- so
             * `_arr[i] = [Foo make];` in a loop was accepted and silently
             * yielded `nil` from the second iteration on, which is exactly
             * what this rule exists to prevent. `walk_for_reject` runs
             * from `emit`, after `arc::analyze`, so
             * `program.owning_methods` is populated and the question can
             * be asked properly. */
            let creates_plus_one = crate::arc::is_owning_expr(
                node,
                src,
                program,
                &program.owning_methods,
            ) && !is_owning_receiver_of_owning_send(node, src, program);
            if creates_plus_one && in_loop {
                if let Some(escape) = loop_escape(node, src, program, scope) {
                    err(diags, src, node, escape.describe(&allocation_of(node, src, program, scope)));
                }
            }
        }
        "block_literal" => {
            check_block_capture(node, src, scope, diags);
            return; // don't descend further with loop/decl context; block is opaque
        }
        // `array_literal` (`@[...]`) and `dictionary_literal`
        // (`@{...}`) are accepted in general -- they desugar to
        // OZArray_oz_initWithItems / OZDictionary_oz_initWithKeysValues
        // calls in emit.rs -- but they allocate, so they are held to the
        // same loop rule as an explicit `alloc` above. Sizing counts one
        // site once however many times it runs (see `pools`), which is
        // sound only when each iteration's instance dies before the next
        // begins. Two things guarantee that: a fresh per-iteration local,
        // released when the iteration's scope ends, and a strong local ARC
        // manages, whose overwrite releases the previous object before
        // allocating the next. A literal in a loop stored anywhere else can
        // accumulate live instances the static count cannot bound, exhausting
        // both the OZArray/OZDictionary slab and the shared element pool.
        // This arm does not return:
        // child nodes (elements; key/value pairs) still get walked by the
        // default descent below, so an unsupported construct nested
        // inside one of them is still caught.
        "array_literal" | "dictionary_literal" if in_loop => {
            let what = if node.kind() == "array_literal" {
                "a boxed array literal"
            } else {
                "a boxed dictionary literal"
            };
            if let Some(escape) = loop_escape(node, src, program, scope) {
                err(diags, src, node, escape.describe(what));
            }
        }
        //
        // `selector_expression` (`@selector(...)`) is a real node kind
        // in tree-sitter-objc 3.0.2 (confirmed against its
        // node-types.json) and is rejected directly here.
        // `protocol_expression` -- unlike `selector_expression` --
        // isn't: `@protocol(Foo)` parses as a generic `at_expression`
        // (see the `at_expression` arm below), the same class of bug
        // already found and fixed for `boxed_expression` in #191. This
        // match arm never fired for it; a dedicated `at_expression`
        // sub-case now gives `@protocol(...)` its own clear message
        // instead of relying on the generic boxed-literal one.
        // `@selector(...)` resolves to that selector's generated record
        // (`companion::render_reflection`), so unlike `@protocol(...)` it
        // is a value with a type -- `SEL` -- and needs no position
        // restriction: it can be stored in a local, held in an ivar or
        // passed as an argument, and C's own type checking covers misuse.
        // What it still cannot be is a selector nothing implements, or one
        // whose signature has no uniform-shape wrapper in a program that
        // performs; both are refused with a located message in
        // `emit::render_selector_literal`, which is where the whole-
        // program facts needed to tell are available.
        /* A `+1` inside a brace initialiser (#477 M3).
         *
         * `emit::reject_owning_store_into_c_struct` already refuses
         * `p.a = [[Thing alloc] init];` -- "the field's ownership cannot be
         * tracked, so the value would be released when its scope ends and
         * the field left dangling". An *initialiser* reaches the same
         * destination through different syntax and was not refused:
         *
         *     struct Pair p = { [[Thing alloc] init], [[Thing alloc] init] };
         *     struct Pair p = { .a = [[Thing alloc] init] };
         *     id arr[2] = { [[Thing alloc] init], nil };
         *
         * Measured: 2, 1 and 1 allocations respectively, **zero** releases
         * in all three, no diagnostic, and the C compiles. The same
         * asymmetry as #326, #336 and #367 -- one question, two walks, and
         * only one of them written.
         *
         * Refused rather than managed for the reason the field refusal
         * gives: a struct field and a C array element are not strong slots,
         * so there is no scope-exit release to pair the store with.
         * `__unsafe_unretained` on the field remains the way to say the
         * aggregate does not own it, exactly as the assignment form
         * already tells the author. */
        "initializer_list" => {
            let mut cursor = node.walk();
            let elements: Vec<Node> = node
                .children(&mut cursor)
                .filter(|c| c.is_named())
                .collect();
            for element in elements {
                /* A designated initialiser wraps the value; look through it
                 * so `.a = [[Thing alloc] init]` answers as the bare form
                 * does. */
                let value = if element.kind() == "initializer_pair" {
                    let mut c2 = element.walk();
                    element
                        .children(&mut c2)
                        .filter(|c| c.is_named())
                        .last()
                        .unwrap_or(element)
                } else {
                    element
                };
                if crate::arc::binds_ownership(value, src, program, &program.owning_methods) {
                    diags.push(
                        Diagnostic::spanning(
                            "storing a '+1' in a brace initialiser is not supported"
                                .to_string(),
                            src,
                            value.start_byte()..value.end_byte(),
                        )
                        .with_note(
                            "a struct field and a C array element are not strong slots, so                              nothing releases what the initialiser stores -- the same reason                              an assignment into a plain C struct field is refused"
                                .to_string(),
                        )
                        .with_help(
                            "initialise the aggregate with nil and store through an ivar,                              or hold each object in its own local"
                                .to_string(),
                        )
                        .with_help(
                            "or declare the field '__unsafe_unretained' to say the aggregate                              does not own it"
                                .to_string(),
                        ),
                    );
                }
            }
        }
        "selector_expression" => {
            return;
        }
        // tree-sitter-objc 3.0.2 parses every `@`-prefixed boxed literal --
        // `@42`, `@3.14f`, `@(expr)`, `@YES`, `@(call())`, even
        // `@protocol(Foo)` -- as a single generic `at_expression` node
        // (there is no dedicated `boxed_expression` or `protocol_expression`
        // node kind in this grammar version). A numeric/boolean-shaped one
        // (see `emit::is_numeric_boxed_shape`) desugars to an OZNumber class-
        // method call, handled in `emit.rs`; a `@protocol(Name)`-shaped one
        // (see `emit::is_protocol_literal_shape`) gets its own message
        // below; anything else (a boxed call expression, etc.) has no
        // desugaring and must still be rejected here, or the emitter's
        // catch-all would pass the raw `@(...)` text straight through as
        // bogus C.
        // A protocol has no value representation here: `@protocol(Name)`
        // resolves to that protocol's generated conformance bitmap, which
        // is only meaningful as the thing `-conformsToProtocol:` tests
        // against. Anywhere else -- assigned to a variable, passed to
        // something else, returned -- there is nothing sensible to hand
        // over, so it stays a hard error rather than leaking a
        // `const uint32_t *` into source that thinks it holds a protocol.
        "at_expression" if crate::emit::is_protocol_literal_shape(node, src) => {
            if !is_conforms_to_protocol_argument(node, src) {
                err(
                    diags,
                    src,
                    node,
                    "'@protocol(...)' is accepted only as the argument of '-conformsToProtocol:' -- a protocol has no runtime value in the static subset",
                );
            }
            return;
        }
        /* `@defs(...)` reaches this arm's condition too -- it is neither
         * numeric nor `@protocol`-shaped -- and `check_at_keywords` now
         * names it specifically (#563). Returning here rather than
         * widening that arm's negation keeps one construct to one
         * diagnostic: before this, a `@defs` inside a method body was
         * refused by the generic message below while the same construct at
         * file scope was not refused at all, which is how the two halves
         * of #563 came to disagree. */
        "at_expression" if is_defs_shape(node, src) => {
            return;
        }
        "at_expression" if !crate::emit::is_numeric_boxed_shape(node, src) => {
            err(
                diags,
                src,
                node,
                "this '@'-boxed expression is not in the static subset's accepted construct set (only a numeric/boolean literal like '@42', '@3.5f', or '@YES' desugars to an OZNumber class-method call)",
            );
            return;
        }
        _ => {}
    }

    let child_in_loop = in_loop || LOOP_KINDS.contains(&node.kind());

    if node.kind() == "declaration" {
        // A declaration's own init_declarator initializer is "fresh" only
        // when the declaration itself sits directly in a loop body.
        let is_block_qualified = {
            let mut cursor = node.walk();
            let found = node
                .children(&mut cursor)
                .any(|c| c.kind() == "type_qualifier" && node_text(c, src) == "__block");
            found
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // A declaration with no initializer has no `init_declarator`
            // anywhere: a pointer gives `pointer_declarator` (`Counter *c;`)
            // and a non-pointer gives a bare `identifier` (`int n;`). Matching
            // only `init_declarator` left both out of `scope.locals`, so
            // `find_capture` did not recognise them and a block closing over
            // one was silently accepted -- while the identical code written
            // with an initializer was rejected. The generated C then failed
            // with `use of undeclared identifier`, naming code the user never
            // wrote.
            //
            // This is deliberately the same set `emit::collect_local_decls`
            // uses, so the bar's idea of what is a local matches the
            // emitter's; they disagreeing is what produced the asymmetry in
            // the first place. It carries that function's known wart too: for
            // a declaration whose *type* is itself a bare `identifier` the
            // type name is also recorded. That needs a typedef'd class name
            // used without a pointer (`OZObject x;`), which is not valid ObjC
            // for an object, and a class type otherwise parses as
            // `type_identifier` -- so the case does not arise in practice.
            if matches!(
                child.kind(),
                "init_declarator" | "pointer_declarator" | "identifier"
            ) {
                // record the declared name as a local
                if let Some(name) = find_first_identifier_before_eq(child, src) {
                    scope.locals.insert(name.clone());
                    if is_block_qualified {
                        scope.block_locals.insert(name);
                    }
                }
                let mut c2 = child.walk();
                for gc in child.children(&mut c2) {
                    walk_for_reject(gc, src, program, scope, child_in_loop, true, diags);
                }
            } else {
                walk_for_reject(child, src, program, scope, child_in_loop, fresh_decl, diags);
            }
        }
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_reject(child, src, program, scope, child_in_loop, fresh_decl, diags);
    }
}

fn check_block_capture(node: Node, src: &str, scope: &MethodScope, diags: &mut Vec<Diagnostic>) {
    // Anything the block itself declares/binds is not a capture.
    let mut own_names: HashSet<String> = HashSet::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_bound_names(child, src, &mut own_names);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_capture(child, src, scope, &own_names, diags);
    }
}

fn collect_bound_names(node: Node, src: &str, out: &mut HashSet<String>) {
    match node.kind() {
        "parameter_declaration" => {
            if let Some(id) = find_last_identifier(node, src) {
                out.insert(id);
            }
        }
        "init_declarator" | "declaration" => {
            if let Some(id) = find_first_identifier_before_eq(node, src) {
                out.insert(id);
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_bound_names(child, src, out);
    }
}

fn find_last_identifier(node: Node, src: &str) -> Option<String> {
    let mut result = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            result = Some(node_text(child, src).to_string());
        } else {
            result = find_last_identifier(child, src).or(result);
        }
    }
    result
}

fn find_first_identifier_before_eq(node: Node, src: &str) -> Option<String> {
    // A declarator can *be* the identifier, with no children to search:
    // `int n;` parses as `declaration(primitive_type, identifier, ;)`, so the
    // declarator handed here is the bare `identifier` itself. Searching only
    // children returned None for it, which is why recording bare declarations
    // as locals did not work on the first attempt -- the caller matched the
    // node kind correctly and then got no name back.
    if node.kind() == "identifier" {
        return Some(node_text(node, src).to_string());
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "=" {
            break;
        }
        if child.kind() == "identifier" {
            return Some(node_text(child, src).to_string());
        }
        if let Some(found) = find_first_identifier_before_eq(child, src) {
            return Some(found);
        }
    }
    None
}

/// Type specifiers and qualifiers, which a declaration's name never is.
///
/// Used to find the declarator among a declaration's direct children: what is
/// left once these are skipped is what carries the declared name.
const TYPE_SPECIFIER_KINDS: &[&str] = &[
    "primitive_type",
    "sized_type_specifier",
    "struct_specifier",
    "union_specifier",
    "enum_specifier",
    "type_identifier",
    "typedefed_specifier",
];

const DECL_TYPE_KINDS: &[&str] = &[
    "type_qualifier",
    "primitive_type",
    "sized_type_specifier",
    "struct_specifier",
    "union_specifier",
    "enum_specifier",
    "type_identifier",
    "typedefed_specifier",
    "storage_class_specifier",
    "macro_type_specifier",
    "attribute_specifier",
];

/// `id` is a reserved word, so nothing may be *declared* with that name.
///
/// It is a type in Objective-C -- the untyped object pointer -- and oz2c
/// rewrites it as one wherever it appears in a declaration. Nothing in the
/// emitter can tell `uint8_t id` (a parameter that happens to be called `id`)
/// from `id obj` (a parameter typed `id`) once a declaration has been
/// flattened to text, and #317 is what that costs: a block parameter named
/// `id` came out as `uint8_t struct OZObject *` -- two type specifiers, no
/// parameter name, and a body still referring to one. Clang accepts the name,
/// because shadowing a typedef with a declarator is legal C, so nothing
/// upstream refuses it either.
///
/// Reserving the name is the fix rather than lowering it correctly. It keeps
/// one spelling of `id` in the language oz2c accepts, and it turns a
/// GCC error about generated C -- in a file the author did not write -- into
/// a located error on the line that caused it. Emitting broken C is the
/// silent degradation this bar exists to prevent.
///
/// Member access is deliberately untouched. `sAdvParam.id` reads a field of
/// a struct that came from a plain `#include`, which `imports::resolve_imports`
/// never expands, so no foreign declaration is even visible here -- only
/// names the author wrote. Zephyr is full of `.id` fields and reaching them
/// has to keep working.
pub fn check_reserved_names(root: Node, src: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_reserved_names(root, src, &mut diags);
    diags
}

fn walk_reserved_names(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    match node.kind() {
        "parameter_declaration" | "declaration" | "field_declaration" | "struct_declaration" => {
            let mut names = Vec::new();
            declared_name_nodes(node, &mut names);
            for name in names {
                if node_text(name, src) == "id" {
                    reserved_name_err(diags, src, name);
                }
            }
        }
        /*
         * An Objective-C method parameter is `:(type)name`, where the name is
         * a plain identifier sibling of the `method_type` -- not a
         * `parameter_declaration`, so the arm above never sees it.
         */
        "method_parameter" => {
            let mut cursor = node.walk();
            let name = node.children(&mut cursor).find(|c| c.kind() == "identifier");
            if let Some(name) = name {
                if node_text(name, src) == "id" {
                    reserved_name_err(diags, src, name);
                }
            }
        }
        _ => {}
    }

    /*
     * An ivar block is the one place the grammar reads the *name* as a type.
     * `uint8_t id;` there parses as `struct_declaration(primitive_type,
     * typedefed_specifier(id), struct_declarator(identifier ""))` -- two
     * stacked type specifiers and an empty declarator -- so there is no
     * identifier node spelling `id` to find, and the arm above sees only
     * what looks like a type.
     *
     * Two type specifiers cannot stack in C, so a `typedefed_specifier`
     * spelling `id` that *follows* another specifier is a name. A lone one
     * is a genuine `id`-typed ivar (`id _delegate;`) and is left alone.
     * Qualifiers deliberately do not count as the preceding specifier, or
     * `const id _delegate;` would trip this.
     */
    if node.kind() == "struct_declaration" {
        let mut cursor = node.walk();
        let mut seen_specifier = false;
        for child in node.children(&mut cursor) {
            let bare_id =
                child.kind() == "typedefed_specifier" && node_text(child, src).trim() == "id";
            if bare_id && seen_specifier {
                reserved_name_err(diags, src, child);
                break;
            }
            if TYPE_SPECIFIER_KINDS.contains(&child.kind()) {
                seen_specifier = true;
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_reserved_names(child, src, diags);
    }
}

/// Every name a declaration declares, as the identifier node spelling it.
///
/// Walks the declaration's *direct* children, skips the type specifiers and
/// qualifiers, and takes the first identifier inside each remaining
/// declarator -- so `int a, id;` yields both, and a declaration with no
/// declarator at all (`(void)`, an abstract parameter type) yields nothing.
///
/// First rather than last: in `void (*id)(int count)` the declarator's own
/// name comes before its parameter list, and the last identifier there is
/// `count`.
fn reserved_name_err(diags: &mut Vec<Diagnostic>, src: &str, node: Node) {
    err(
        diags,
        src,
        node,
        "'id' is a reserved word (Objective-C's untyped object pointer type) and cannot be used \
         as a declared name -- rename it (e.g. 'identity')",
    );
}

/// The five selectors Clang refuses under `-fobjc-arc`, and which user code
/// therefore cannot send.
///
/// Not a list of "discouraged" spellings: ARC is always enabled here, and
/// every Clang path in this repo passes `-fobjc-arc`
/// (`cmake/oz2c.cmake`, `cmake/ObjcClang.cmake`,
/// `tests/tools/compile_and_run.py`, `tests/common/mod.rs`,
/// `scripts/regen_zephyr_tests.py`, and `scripts/objz_check_compile_db.py`'s
/// `REQUIRED_FLAGS`), under which each of these is a compile error. oz2c
/// parses with tree-sitter rather than Clang, which is the only reason they
/// were ever reachable (#428).
///
/// `retainCount` is here too, as of #436, and adding it is what made the
/// list's rule statable in one line: **this is exactly the set Clang refuses
/// under `-fobjc-arc`.** Nothing is weighed per selector any more.
///
/// It was left out originally on the grounds that reading a count takes and
/// gives no ownership, so it is not a second ownership model. True, and not
/// the question -- ARC forbids the *send* regardless, which a probe settles
/// rather than an argument: declared or undeclared, Clang answers
/// `ARC forbids explicit message send of 'retainCount'`. Reading a refcount
/// is still supported, through the spelling that was always the sanctioned
/// one: `oz_retain_count()`, a plain C call, which #418 made the
/// single entry point and which `include/oz_sdk/Foundation/OZObject.h` has
/// described as "the only refcount entry point Objective-C source may spell"
/// all along. That sentence was true of the design and false of the
/// implementation until #436.
const ARC_FORBIDDEN_SELECTORS: &[&str] =
    &["retain", "release", "autorelease", "dealloc", "retainCount"];

/// Reject a send of `-retain`, `-release`, `-autorelease`, `-dealloc` or
/// `-retainCount` -- the selectors Clang refuses under `-fobjc-arc`.
///
/// **Deliberate, not accidental.** `[obj autorelease]` was already refused
/// before this check existed, but only as a by-product of method lookup
/// ("class 'X' has no method matching 'autorelease'"), which says nothing
/// about the rule being broken; `[obj release]` was not refused at all, and
/// `emit::released_by_hand` was built to *accommodate* it, making manual
/// retain/release a second ownership model reachable only because the
/// primary parser is more permissive than the oracle (#428).
///
/// Here rather than in one of the three body-scoped entry points
/// (`check_method_body`, `check_function_body`, `check_macro_body`), for the
/// same reason `check_reserved_names` is: this is a fact about the *send*
/// and not about any body. `walk_for_reject` stops at a `block_literal`
/// (the block is opaque to the capture check), never sees an ivar block or a
/// file-scope initializer, and would have to be entered twice; a whole-root
/// walk sees every position there is, which is what "enumerate the
/// siblings rather than fixing the one" requires of a rejection that must
/// catch every spelling.
///
/// The receiver's shape is not consulted at all, which is the point. A
/// plain local, `self`, `super`, a bare ivar, `self->_ivar`, a subscript, a
/// chained send, a cast and an `id`-typed reference all reach the same arm,
/// because the question is which selector was sent and not what it was sent
/// to -- the "key ownership on the reference, never on a syntactic form"
/// rule, applied to a rejection.
/// Reject `@autoreleasepool { ... }`.
///
/// **Not because it misbehaves.** The block really is an ordinary ARC
/// scope: `emit` dropped the token and ran the same `arc_enter`/`arc_exit`
/// bookkeeping as `render_scoped_block`, so everything allocated inside was
/// released at the closing brace. Behaviour was correct.
///
/// It is rejected because the keyword promises a mechanism that does not
/// exist here and cannot (#430):
///
///   - **There is no `-autorelease`.** It is in no SDK header, and a send of
///     it is a hard located error -- one of the five selectors ARC forbids
///     (`ARC_FORBIDDEN_SELECTORS`, #428/#436). So no reference can ever be
///     *pending* at the closing brace, and a drain would have nothing to do.
///   - **There is no pool object.** `OZAutoreleasePool` exists only under
///     `src/runtime_legacy/`, which no CMake file references.
///
/// So the construct was accepted for its syntax alone, and a reader porting
/// Cocoa code got immediate reclaim at scope exit where the keyword promises
/// deferred reclaim at a drain point. In Cocoa those differ observably; here
/// the difference is unreachable, because the mechanism that creates it is
/// refused. Accepting a keyword whose meaning is a mechanism the backend
/// does not have is exactly what the never-silently-degrade rule forbids --
/// and "it happens to behave correctly" is the argument that kept
/// `__objc_refcount_get` public for a year (#418).
///
/// A whole-root walk for the reason `check_manual_memory_sends` gives: this
/// is a fact about the construct, not about the body it sits in, and
/// `walk_for_reject` treats a block literal as opaque.
///
/// tree-sitter-objc gives the construct no node kind of its own -- it parses
/// as a `compound_statement` whose first child is the literal token
/// `@autoreleasepool`, ahead of the usual `{`. That shape test used to live
/// in `emit::is_autoreleasepool_shape`, which this replaces.
/// The two bridging casts that **transfer** a reference, and are therefore
/// refused rather than treated like the one that does not.
///
/// ARC spec § 1.3.4 defines three, and they mean three different things:
///
/// | cast | ownership |
/// |---|---|
/// | `(__bridge T)` | transfers nothing; the source keeps its reference |
/// | `(__bridge_retained T)` | ARC **retains**, handing a `+1` to the recipient |
/// | `(__bridge_transfer T)` | ARC takes over a `+1` and **releases** it |
///
/// `arc::is_bridging_cast` matches all three by name and treats them
/// identically -- as a signal to hold ownership back, so none of the three
/// questions looks through them. That is right for the first and wrong for
/// the other two, in opposite directions (#460):
///
///   * `__bridge_retained` emitted no retain, so the local was still
///     released at scope exit and the C side was handed a freed slot.
///     Measured under ASan as `heap-use-after-free` -- and the read of the
///     stale pointer *succeeded* first, printing the right value, which is
///     the trap `docs/STATUS.md` records: a use-after-free is silent until
///     the allocator reuses the block.
///   * `__bridge_transfer` emitted no release, so the `+1` it took over
///     from C was stranded. A leak.
///
/// Refused rather than implemented, and the decision was measured rather
/// than argued. Across `src/`, `include/`, `samples/`, `tests/` and
/// px-keyboard there are **zero** uses of either -- all six bridging casts
/// in the tree are plain `__bridge`, and all six are correct:
/// `px-keyboard/src/PXLEDController.m:61,143` round-trips `self` through a
/// Zephyr `k_timer` user_data, and `samples/smp_shared/src/main.m:207-208`
/// casts through `__bridge` to drive a refcount by hand via the C API that
/// #437 made a deliberate escape hatch. So this refuses nothing that
/// exists, and converts two silent memory bugs into build errors.
///
/// Same precedent as #430 and #458: a spelling whose meaning is a
/// mechanism the backend does not have must not be quietly accepted.
/// Implementing them stays available and is strictly better *if* CF-style
/// hand-off to C is meant to be supported -- that is a product question,
/// not a correctness one, and the refusal does not foreclose it. It would
/// also need new emission, which wanted sequencing after #462's respelling
/// of the emitted ABI rather than before it.
///
/// Plain `__bridge` is deliberately **not** here, and
/// `arc::is_bridging_cast` keeps naming all three: the list there is what
/// holds a bridging cast opaque to the ownership questions, and narrowing
/// it to one spelling would make the other two fall through to the
/// non-bridging cast path -- which looks *through* the cast and is how
/// #332 double-released. Refusing them at the bar and keeping them opaque
/// at the analysis are complementary, not redundant.
const TRANSFERRING_BRIDGE_CASTS: &[&str] = &["__bridge_retained", "__bridge_transfer"];

/// Refuse a transferring bridging cast wherever it appears.
///
/// A whole-root walk, for the reason `check_manual_memory_sends`,
/// `check_autoreleasepool` and `check_ownership_attributes` all give: this
/// is a fact about the cast, not about the body it sits in, and
/// `walk_for_reject` treats a block literal as opaque.
pub fn check_bridging_casts(root: Node, src: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_bridging_casts(root, src, &mut diags);
    diags
}

fn walk_bridging_casts(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    if node.kind() == "cast_expression" {
        if let Some(kind) = transferring_bridge_kind(node, src) {
            let (verb, direction, consequence) = match kind {
                "__bridge_retained" => (
                    "retain its operand, handing a '+1' to the C side",
                    "the reference is still released at the end of this scope",
                    "so the pointer C keeps is freed -- a use-after-free",
                ),
                _ => (
                    "release the '+1' it takes over from the C side",
                    "no release is emitted for it anywhere",
                    "so that reference is stranded -- a leak",
                ),
            };
            /* Split across the tiers rather than fused into one sentence
             * (#457). This refusal landed while #457 was still open, so
             * it went in through the single-string `err`; the wording is
             * #460's own, only its tier moved. */
            err_detailed(
                diags,
                src,
                node,
                Rejection {
                    message: format!("'{kind}' is not supported: it transfers a reference"),
                    note: Some(format!(
                        "ARC would {verb}, and oz2c emits no such traffic -- \
                         {direction}, {consequence}"
                    )),
                    help: vec![
                        "use a plain '(__bridge T)' cast, which transfers no ownership"
                            .to_string(),
                        "keep the object alive on the Objective-C side independently -- an \
                         instance variable, or a singleton adopting \
                         'OZSingletonProtocol', which is what 'px-keyboard' does for the \
                         pointer it hands to a Zephyr callback"
                            .to_string(),
                    ],
                },
            );
            return;
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_bridging_casts(child, src, diags);
    }
}

/// Which transferring bridge qualifier `cast` carries, if any.
///
/// Reads the `type_qualifier` children of the cast's `type_descriptor`,
/// the shape `arc::is_bridging_cast` already relies on -- named exactly
/// rather than matched on a `__bridge` prefix, so plain `__bridge` cannot
/// be caught by accident.
fn transferring_bridge_kind(cast: Node, src: &str) -> Option<&'static str> {
    let mut cursor = cast.walk();
    let descriptor = cast.children(&mut cursor).find(|c| c.kind() == "type_descriptor")?;
    let mut cursor = descriptor.walk();
    let qualifiers: Vec<Node> = descriptor.children(&mut cursor).collect();
    qualifiers.into_iter().find_map(|child| {
        if child.kind() != "type_qualifier" {
            return None;
        }
        let text = node_text(child, src).trim();
        TRANSFERRING_BRIDGE_CASTS.iter().copied().find(|k| *k == text)
    })
}

/// The ARC attributes that *contradict* an ownership answer oz2c
/// derives some other way, and are therefore refused rather than ignored.
///
/// Each one exists to override a convention: `ns_returns_not_retained`
/// takes a create-rule selector *out* of the owning set,
/// `ns_returns_retained` puts an arbitrary one *in*, `ns_consumed` and
/// `ns_consumes_self` move an argument's release to the callee, and
/// `objc_method_family` reassigns a selector's family outright.
///
/// oz2c reads none of them, and silently ignoring them is not a
/// harmless gap -- it is a use-after-free (#458). Measured:
///
/// ```objc
/// - (Thing *)copy __attribute__((ns_returns_not_retained))
/// {
///         return _shared;        /* a borrowed ivar */
/// }
/// ```
///
/// ARC reads the attribute and says `+0`, so the caller owes nothing.
/// `copy` is a create-rule selector, so oz2c says `+1` and releases
/// at scope exit -- freeing an object the caller never owned, while the
/// ivar still holds it. `-dealloc` then releases the freed block again.
/// ASan reports `heap-use-after-free`, from a source
/// `clang -fobjc-arc -Weverything` accepts with **zero** diagnostics.
///
/// Refused rather than implemented, and the reason is the same one #430
/// gives for `@autoreleasepool`: a spelling whose meaning is a mechanism
/// the backend does not have must not be quietly accepted. Implementing
/// them needs the Clang AST read for attributes, which `astinfo.rs` does
/// not do yet (#453) -- so until it does, the honest answer is a located
/// error. Nothing in `src/`, `include/`, `samples/`, `tests/` or
/// px-keyboard uses any of the five, so this refuses nothing that exists.
///
/// **And refusing them is what makes the family rule safe to widen.**
/// `arc::create_rule_family_of` now matches ARC's families rather than six
/// exact spellings, which is only sound while no attribute can contradict
/// the family a selector is spelled into. The two halves of #458 are one
/// change for that reason and must not be separated.
///
/// `objc_precise_lifetime` and `objc_externally_retained` are deliberately
/// **not** here. They constrain ARC's freedom to move traffic rather than
/// reassigning ownership, and every release this backend emits is already
/// precise -- so ignoring them changes no answer. They are #461's, which is
/// about a different defect: they reach the generated C unlowered.
const OWNERSHIP_ATTRIBUTES: &[&str] = &[
    "ns_returns_retained",
    "ns_returns_not_retained",
    "ns_consumed",
    "ns_consumes_self",
    "objc_method_family",
];

/// Refuse an ARC ownership attribute wherever it appears.
///
/// A whole-root walk for the reason `check_manual_memory_sends` and
/// `check_autoreleasepool` both give: this is a fact about the
/// declaration, not about the body it sits in, and none of `staticbar`'s
/// body-scoped entry points sees a method *declaration* in an
/// `@interface` at all.
///
/// The attribute parses as an `attribute_specifier` whose `argument_list`
/// holds the name as an `identifier` -- `objc_method_family(none)` nests
/// one level deeper, so the walk looks at every identifier beneath the
/// specifier rather than only its first child.
/// `__weak`, wherever it is written.
///
/// The qualifier is prohibited by design: nothing can zero a weak reference
/// without a runtime, so it would behave as an unretained strong reference
/// -- silently, and that is the exact bug the qualifier exists to prevent.
/// It is deliberately absent from `emit::STRIPPED_ARC_QUALIFIERS` so that it
/// fails rather than being quietly dropped.
///
/// **It failed in one position out of ten.** Measured on this tree before
/// the fix, `__weak` reached the generated C verbatim from a local, a
/// `static` local, a file-scope declaration, a `for`-header declaration, a
/// method parameter, a plain-C-function parameter, a block parameter, a C
/// struct field, and the *type* of a property. Only an ivar was refused.
///
/// The issue reporting this counted two covered positions, an ivar and a
/// property, and the property half does not survive measurement: what
/// `collect.rs` refuses is the property **attribute** `weak`
/// (`@property (weak) T *w;`), which is not a `type_qualifier` node at all.
/// The type-qualifier spelling on the same property
/// (`@property () __weak T *w;`) reached the output. Two different
/// spellings, one refused and one not, and the report read them as one
/// position -- so the attribute check stays where it is and this walk does
/// not touch it.
///
/// A whole-tree walk for the same reason as the five checks beside it: the
/// qualifier is a fact about the declaration it qualifies, not about the
/// body it sits in, and no body-scoped entry point sees a file-scope
/// declaration, a struct field, an `@interface` method parameter, or the
/// inside of a block literal.
///
/// **This needs nothing from #453.** A `type_qualifier` node carries a byte
/// range, so the diagnostic is located from the CST alone; the author wrote
/// the token, and pointing at it is the whole job. What would need Clang's
/// resolved declarations is the *inferred* qualifier on a plain `T **`
/// (#461, ARC §2.7.2), where there is no token to point at.
pub fn check_refused_qualifiers(root: Node, src: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_weak_qualifier(root, src, &mut diags);
    diags
}

/// The two ownership qualifiers oz2c refuses, and the reason each gives.
///
/// Both are refused rather than stripped, and the distinction matters:
/// `emit::STRIPPED_ARC_QUALIFIERS` drops `__strong` and
/// `__unsafe_unretained` because each *is* the behaviour this target
/// already has, so dropping the word changes nothing. Neither of these two
/// has that property -- one would need a runtime and the other a pool, and
/// quietly dropping a word whose mechanism does not exist is the silent
/// degrade the project's standing rule forbids.
const REFUSED_QUALIFIERS: &[(&str, &str)] = &[
    (
        "__weak",
        "'__weak' is not supported: nothing zeroes a weak reference without a runtime, so it \
         would silently behave as an unretained strong reference -- which is the bug the \
         qualifier exists to prevent. Use '__unsafe_unretained' and clear it explicitly",
    ),
    (
        "__autoreleasing",
        "'__autoreleasing' has no meaning in the static subset: there is no pool to release \
         into -- ARC forbids '-autorelease', so nothing can ever be pending, and \
         '@autoreleasepool' is refused for the same reason. Delete the qualifier: the \
         declaration then carries the default '__strong', and ARC releases what it owns at \
         the closing brace, which is the only timing this target has",
    ),
];

fn walk_weak_qualifier(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    if node.kind() == "type_qualifier" {
        let text = node_text(node, src).trim();
        if let Some((_, message)) = REFUSED_QUALIFIERS.iter().find(|(name, _)| *name == text) {
            err(diags, src, node, *message);
            return;
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_weak_qualifier(child, src, diags);
    }
}

/// A `message_expression` that is not a well-formed send.
///
/// The bar's own reason for existing: **the malformation was reaching code
/// that assumed it away.** `emit::parse_message` had no way to say "not a
/// send", so ten callers across five modules were written against the
/// assumption, and a missing colon did one of two things (#494):
///
///   * `[s isEqual other]` -- a **panic**, `index out of bounds: the len is
///     3 but the index is 3`, from `collect::prescan_reflection` before any
///     check could run. Nothing written to the output directory.
///   * `[s take:n n]` -- the trailing token **silently dropped** and
///     `T_take_(self, n)` emitted. Measured: that generated C compiles with
///     **zero** errors, so nothing downstream catches it either. Worse than
///     the panic, which at least stops the build.
///
/// Refusing it here is what lets `parse_message` return `None` safely: the
/// program never reaches `emit`, so every `None` arm is defence rather than
/// a behaviour anyone depends on.
///
/// Clang reports the same source as `expected ':'` and points at the
/// column, so this is a rule oz2c owes rather than one it delegates -- and
/// on the paths that dump an AST, Clang's refusal is not a backstop we can
/// rely on: the panic reproduced with a dump present.
/// The single term of a `[receiver]` send -- one where the selector is a
/// MISSING node rather than text.
///
/// `[ value]` and `[obj]` are the whole of this shape: tree-sitter
/// recovers both as the receiver plus a missing identifier. `[]` and
/// `[self :1]` are *not* -- those come back as `ERROR` nodes and never
/// reach here at all, which is why `MUTATIONS.md` grades M06 and M10 as
/// caught by Clang rather than by oz2c.
///
/// Returns the term's text, because the two remedies have to quote it:
/// the author meant it as a selector or as a receiver, and nothing in the
/// source says which.
fn lone_term_send(node: Node, src: &str) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<Node> =
        node.children(&mut cursor).filter(|c| c.kind() != "[" && c.kind() != "]").collect();
    if children.len() != 2 || !children[1].is_missing() {
        return None;
    }
    Some(node_text(children[0], src).to_string())
}

pub fn check_malformed_sends(root: Node, src: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_malformed_sends(root, src, &mut diags);
    diags
}

fn walk_malformed_sends(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    if node.kind() == "message_expression" && crate::emit::parse_message(node, src).is_none() {
        if let Some(term) = lone_term_send(node, src) {
            /* A different malformation, and it used to borrow the other
             * one's message. `[ value]` has no missing colon -- it has
             * one term where a send needs two, and the ':' advice cannot
             * be acted on (#551). */
            err_detailed(
                diags,
                src,
                node,
                Rejection {
                    message: format!(
                        "'{}' names one term where a message send needs two -- \
                         '[receiver selector]'",
                        crate::emit::one_line(node_text(node, src))
                    ),
                    note: Some(format!(
                        "the parse binds the single term as the *receiver*, so it is the \
                         selector that is absent. Clang reads it the same way and reports \
                         `use of undeclared identifier '{}'` rather than a missing selector, \
                         which is why neither tool can tell you which half you meant to write",
                        term
                    )),
                    help: vec![
                        format!(
                            "if '{}' is the selector, name the receiver it belongs to: \
                             '[self {}]'",
                            term, term
                        ),
                        format!(
                            "if '{}' is the receiver, add the selector to send it: \
                             '[{} someSelector]'",
                            term, term
                        ),
                    ],
                },
            );
        } else {
            err(
                diags,
                src,
                node,
                format!(
                    "'{}' is not a well-formed message send: each keyword needs a ':' before \
                     its argument. Clang reports the same source as `expected ':'`",
                    crate::emit::one_line(node_text(node, src))
                ),
            );
        }
        /* One diagnostic per send. A malformed send's children are not a
         * reliable shape, so descending to look for a nested one inside it
         * would report positions the author cannot act on until the outer
         * send is fixed. */
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_malformed_sends(child, src, diags);
    }
}

pub fn check_ownership_attributes(root: Node, src: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_ownership_attributes(root, src, &mut diags);
    diags
}

fn walk_ownership_attributes(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    if node.kind() == "attribute_specifier" {
        if let Some(name) = named_ownership_attribute(node, src) {
            err(
                diags,
                src,
                node,
                format!(
                    "'{name}' is not supported: it overrides an ownership answer oz2c                      derives from the selector and the method's return type, and oz2c does                      not read it -- so accepting it silently means ARC and the attribute                      disagree, which is a use-after-free rather than a leak (#458). Remove the                      attribute and let the create rule decide: name a '+1' factory 'copy',                      'new', 'mutableCopy' or 'alloc' (or any selector whose first component                      begins with one of those), and anything else is '+0'"
                ),
            );
            /* One diagnostic per specifier: a second identifier under the
             * same `__attribute__((...))` is the same mistake. */
            return;
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_ownership_attributes(child, src, diags);
    }
}

/// The ownership attribute named under `spec`, if any.
fn named_ownership_attribute(spec: Node, src: &str) -> Option<&'static str> {
    fn search(node: Node, src: &str) -> Option<&'static str> {
        if node.kind() == "identifier" {
            let text = node_text(node, src).trim();
            if let Some(found) = OWNERSHIP_ATTRIBUTES.iter().copied().find(|a| *a == text) {
                return Some(found);
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = search(child, src) {
                return Some(found);
            }
        }
        None
    }
    search(spec, src)
}

/// Refuse a `+1` written through a dereferenced pointer (#461).
///
/// ARC spec § 2.6.5 makes `T __autoreleasing *` an out-parameter written by
/// pass-by-writeback, and § 2.7.2 says an unqualified `T *` parameter
/// *infers* `__autoreleasing`. So `*out = <+1>` is a store into an
/// ARC-managed slot -- and it is the one strong destination nothing here
/// walks. `render_strong_ivar_assign` and `render_strong_local_assign`
/// between them cover an ivar, a managed local, a `static` local and a
/// file-scope object; a `*p` lvalue is none of the four. On the caller's
/// side the variable is written only through a pointer, so
/// `managed_object_locals`' "something `Owning` is ever stored" test never
/// fires and it joins no scope either. The reference is created and nobody
/// owns it: a leak, and the #359 shape at a fifth site.
///
/// **Refused rather than implemented, and keyed on the `+1` rather than on
/// the shape.** ARC's own answer is writeback through a temporary that is
/// autoreleased, and there is no pool here to autorelease into -- the
/// #430 precedent, where a keyword whose mechanism does not exist is
/// refused rather than quietly accepted.
///
/// Keying on the shape would have been wrong, and the SDK proves it:
/// `OZArray.h:28` and `OZDictionary.h:30` declare
/// `objects:(__unsafe_unretained id *)stackbuf`, a pointer-to-object-pointer
/// parameter the callee writes through, and it is correct -- the buffer
/// takes borrowed references. A refusal keyed on "a pointer written
/// through" refuses fast enumeration. The question is whether a `+1` goes
/// in, which is `arc::binds_ownership` -- the same predicate
/// `classify_store` asks about a named local.
///
/// Parses its own tree for the reason `generics::check_program` does: this
/// needs `program.owning_methods`, which `arc::analyze` fills in after
/// `collect` has run, so it cannot sit beside the refusals `collect`
/// registers.
/// Two defects a method *declaration* can carry that nothing looked for:
/// a variadic ellipsis (#538) and a parameter name used twice (#549).
///
/// Both were silent here and surfaced from **GCC, on generated C** -- the
/// failure shape #501, #205 and OZ-001/002/004 all share, and the one this
/// project exists to avoid.
///
/// **A deferred check, not a hard gate**, and that placement is #540's
/// rule rather than a preference. `collect`'s root scans gate the pipeline
/// because a diagnostic there means the `Program` may be unwalkable -- a
/// `superclass` that is not a key in `classes`. Neither of these does that:
/// the class table is fine, one signature is merely wrong. So they sit with
/// `check_out_parameter_stores` where the diagnostics are carried forward,
/// and an author who writes both a duplicate parameter and a `@try` sees
/// both in one build. Putting them in `collect` would have made them the
/// earliest masker in the pipeline, which is the thing #540 fixed.
/// A block literal the parser had to **guess the end of**, and an `OZFN`
/// argument that is not a block or a function name (#548).
///
/// `OZFN(...)` expands to `0` for Clang and to `__VA_ARGS__` for C, so its
/// argument is invisible to Clang *by construction* -- `OZMacro.h` explains
/// why it has to be: to reach a static initializer the expansion must be a
/// null pointer constant, and the block has to go unparsed. oz2c is
/// therefore the only gate on it, and it validated nothing.
///
/// The worst shape was a block missing its closing brace. tree-sitter
/// recovers by **inserting** one, so oz2c received a well-formed
/// `block_literal` ending at the macro's `)`, hoisted it, and wrote the
/// hoisted name back -- exit 0, no diagnostic, and a function body the
/// author never wrote. That is the same "quietly shortened" failure #494
/// removed for sends, still live inside a macro argument. Outside one the
/// same mutation is caught, because Clang sees it there.
///
/// Detected from the parser rather than by counting braces: a recovered
/// node is marked `is_missing()`, so the question "did the author close
/// this block?" has an exact answer and needs no lexing of our own.
pub fn check_macro_and_block_syntax(source: &str) -> Vec<Diagnostic> {
    let tree = crate::parse::parse(source);
    let mut diags = Vec::new();
    walk_macro_and_block_syntax(tree.root_node(), source, &mut diags);
    diags
}

fn walk_macro_and_block_syntax(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    if node.kind() == "block_literal" {
        check_block_is_closed(node, src, diags);
    }
    if node.kind() == "call_expression" {
        check_ozfn_argument(node, src, diags);
    }
    let mut cursor = node.walk();
    let kids: Vec<Node> = node.children(&mut cursor).collect();
    for child in kids {
        walk_macro_and_block_syntax(child, src, diags);
    }
}

/// A `block_literal` whose closing `}` the parser inserted.
fn check_block_is_closed(block: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    /* A descendant, not a direct child: the inserted `}` belongs to the
     * `compound_statement` the block wraps, not to the block itself. A
     * direct-child test found nothing and the headline case went
     * unreported -- confirmed by dumping the tree, which is the only way
     * these questions have ever been settled here.
     *
     * Descent stops at a nested `block_literal`, because that one reports
     * itself on its own visit and would otherwise be blamed twice. */
    if !has_inserted_close_brace(block, true) {
        return;
    }
    err_detailed(
        diags,
        src,
        block,
        Rejection {
            message: "this block literal is missing its closing '}'".to_string(),
            note: Some(
                "the parser inserted one to carry on, so the block ended wherever the                  enclosing construct did -- inside a macro argument that is the                  closing ')', and the body hoisted from it is not the one written here"
                    .to_string(),
            ),
            help: vec![
                "close the block with '}' before the enclosing ')' or ';'".to_string(),
                "an 'OZFN'/'OZM' argument is the one place this is not caught for you:                  the macro hides it from Clang by design, so oz2c is the only check on it"
                    .to_string(),
            ],
        },
    );
}

/// Does this subtree contain a `}` the parser inserted, without crossing
/// into a nested block literal?
fn has_inserted_close_brace(node: Node, is_root: bool) -> bool {
    if !is_root && node.kind() == "block_literal" {
        return false;
    }
    if node.is_missing() && node.kind() == "}" {
        return true;
    }
    let mut cursor = node.walk();
    let kids: Vec<Node> = node.children(&mut cursor).collect();
    kids.into_iter().any(|child| has_inserted_close_brace(child, false))
}

/// `OZFN`'s argument must be a block literal or the name of a function.
///
/// `OZFN(42)` reached the output verbatim: in a typed callback slot GCC
/// then reported `initialization of 'void (*)(struct k_timer *)' from
/// 'int'` against generated C, and in a discarded expression --
/// `(void)OZFN(42)` -- nothing complained at all. `OZFN()` reached it too
/// and produced `expected expression before ')'`.
///
/// `OZM` is deliberately not checked here: its first argument is the target
/// macro's *name* and the rest are that macro's own arguments, so it has a
/// different contract and no single shape to assert.
fn check_ozfn_argument(call: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    let mut cursor = call.walk();
    let kids: Vec<Node> = call.children(&mut cursor).collect();
    let Some(callee) = kids.first() else { return };
    if callee.kind() != "identifier" || node_text(*callee, src) != "OZFN" {
        return;
    }
    let Some(args) = kids.iter().find(|c| c.kind() == "argument_list") else { return };
    let mut c2 = args.walk();
    let given: Vec<Node> = args
        .children(&mut c2)
        .filter(|c| !matches!(c.kind(), "(" | ")" | ","))
        .collect();
    let bad = match given.as_slice() {
        [] => Some("it is empty"),
        [one] if matches!(one.kind(), "block_literal" | "identifier") => None,
        [_one] => Some("it is neither a block literal nor the name of a function"),
        _ => None,
    };
    let Some(why) = bad else { return };
    err_detailed(
        diags,
        src,
        *args,
        Rejection {
            message: format!("'OZFN' takes one block literal or function name, and {}", why),
            note: Some(
                "'OZFN' expands to '0' for Clang so that it is a null pointer constant                  in a static initializer, which is exactly why Clang never sees the                  argument -- oz2c is the only thing that can check it, and an argument                  it cannot use reaches the C compiler as written"
                    .to_string(),
            ),
            help: vec![
                "pass a block literal -- 'OZFN(^void(struct k_timer *t) { ... })'"
                    .to_string(),
                "or the name of a plain C function with the right signature".to_string(),
            ],
        },
    );
}

pub fn check_method_declarations(source: &str) -> Vec<Diagnostic> {
    let tree = crate::parse::parse(source);
    let mut diags = Vec::new();
    walk_method_declarations(tree.root_node(), source, &mut diags);
    diags
}

fn walk_method_declarations(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    if matches!(node.kind(), "method_declaration" | "method_definition") {
        check_variadic_parameter(node, src, diags);
        check_duplicate_parameter_names(node, src, diags);
    }
    let mut cursor = node.walk();
    let kids: Vec<Node> = node.children(&mut cursor).collect();
    for child in kids {
        walk_method_declarations(child, src, diags);
    }
}

/// `- (int)sumOf:(int)count, ...` -- the ellipsis was **dropped**, not
/// refused (#538).
///
/// The declaration was altered rather than rejected, which is the part that
/// matters: `extract_method_sig` rebuilds the C signature from `params`
/// alone, so `, ...` could not reappear, and the comment the emitter writes
/// above the function preserved it verbatim while the signature did not. A
/// body that never reaches for `va_start` would compile silently as a
/// fixed-arg function; the one that does failed on GCC's
/// `'va_start' used in function with fixed arguments`, naming a cause that
/// is a *symptom* of the drop.
///
/// Refused rather than implemented: the dispatch shims declare one concrete
/// signature per selector and `-performSelector:`'s wrapper has a fixed
/// shape, so a variadic Objective-C method has nowhere to go. `OZLog` is
/// variadic and works because it is a plain C function (`src/OZLog.c`) --
/// the obvious counter-example, and worth naming before a reader finds it.
fn check_variadic_parameter(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    let mut cursor = node.walk();
    let kids: Vec<Node> = node.children(&mut cursor).collect();
    /* The node kind is the anonymous token `"..."`, **not**
     * `variadic_parameter`. That type does exist in tree-sitter-objc's
     * `node-types.json` and belongs to a plain C parameter list; an
     * Objective-C method's ellipsis is filed as a bare token child of the
     * `method_declaration`. Settled by dumping the tree for the fragment,
     * because the grammar's own type list points the other way -- the same
     * trap `id<Proto>` set for #367, where the obvious `generic_specifier`
     * was wrong and the answer was `typedefed_specifier`. */
    let Some(ellipsis) = kids.into_iter().find(|c| c.kind() == "...") else {
        return;
    };
    err_detailed(
        diags,
        src,
        ellipsis,
        Rejection {
            message: "a variadic Objective-C method is not supported".to_string(),
            note: Some(
                "the ellipsis was silently dropped before this check existed, so the generated \
                  C declared a fixed-arg function and a 'va_start' in the body failed on GCC \
                  instead of here"
                    .to_string(),
            ),
            help: vec![
                "pass the arguments as an OZArray, or add a counted parameter and a pointer to \
                  the values"
                    .to_string(),
                "a variadic plain C function is still available -- 'OZLog' is one \
                  ('src/OZLog.c'); it is an Objective-C *method* that cannot be, because a \
                  dispatch shim declares one concrete signature per selector"
                    .to_string(),
            ],
        },
    );
}

/// `- (int)addA:(int)amount andB:(int)amount` -- accepted here, rejected by
/// GCC on `oz2c_dispatch.h`, a file the author never opened (#549).
///
/// Located at the **second** occurrence, which is the one to rename: the
/// first is where the reader expects the name to be introduced.
fn check_duplicate_parameter_names(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    let mut cursor = node.walk();
    let kids: Vec<Node> = node.children(&mut cursor).collect();
    let mut seen: Vec<String> = Vec::new();
    for child in kids {
        if child.kind() != "method_parameter" {
            continue;
        }
        /* The same accessor `collect::extract_method_sig` uses, so the two
         * cannot disagree about which identifier is the parameter's name. */
        let mut c2 = child.walk();
        let name = child
            .children(&mut c2)
            .find(|n| n.kind() == "identifier")
            .map(|n| node_text(n, src).to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        if seen.contains(&name) {
            err_detailed(
                diags,
                src,
                child,
                Rejection {
                    message: format!(
                        "parameter '{}' is declared more than once in this method",
                        name
                    ),
                    note: Some(
                        "each selector component's parameter becomes a separate C parameter of \
                          one function, so two of the same name is a redefinition -- which GCC \
                          used to report against 'oz2c_dispatch.h', a generated file with no \
                          line the author wrote"
                            .to_string(),
                    ),
                    help: vec![format!("rename this '{}' to something distinct", name)],
                },
            );
        } else {
            seen.push(name);
        }
    }
}

pub fn check_out_parameter_stores(source: &str, program: &Program) -> Vec<Diagnostic> {
    let tree = crate::parse::parse(source);
    let mut diags = Vec::new();
    walk_out_parameter_stores(tree.root_node(), source, program, &mut diags);
    diags
}

/// Is `node` a dereference -- `*p` rather than `&p`?
///
/// `pointer_expression` is the grammar's node for both, distinguished by
/// its `operator` field. Taken from tree-sitter-objc 3.0.2's own
/// `node-types.json` rather than from tree-sitter-c, whose spelling this
/// grammar does not always share.
fn is_dereference(node: Node, src: &str) -> bool {
    node.kind() == "pointer_expression"
        && node
            .child_by_field_name("operator")
            .is_some_and(|op| node_text(op, src).trim() == "*")
}

fn walk_out_parameter_stores(
    node: Node,
    src: &str,
    program: &Program,
    diags: &mut Vec<Diagnostic>,
) {
    if node.kind() == "assignment_expression" {
        let mut cursor = node.walk();
        let parts: Vec<Node> = node.children(&mut cursor).collect();
        if let (Some(lhs), Some(rhs)) = (parts.first(), parts.last()) {
            if is_dereference(*lhs, src)
                && crate::arc::binds_ownership(*rhs, src, program, &program.owning_methods)
            {
                /* Tiered rather than fused, per #457: the diagnosis, the
                 * reason, and one line per remedy. This refusal names the
                 * *store* the author made, where the qualifier refusal
                 * names the token they wrote -- two sites, one rule. */
                err_detailed(
                    diags,
                    src,
                    node,
                    Rejection {
                        message: "a '+1' stored through a dereferenced pointer is owned \
                                  by nobody"
                            .to_string(),
                        note: Some(
                            "ARC would hand it to the caller's variable by writeback \
                             through an autoreleased temporary, and the static subset \
                             has no pool to autorelease into. So the store releases \
                             nothing, and the caller's variable -- written only through \
                             the pointer -- joins no scope either: the reference leaks"
                                .to_string(),
                        ),
                        help: vec![
                            "return the object instead, so the '+1' travels as a return \
                             value that the caller's own scope owns and releases"
                                .to_string(),
                            "or, if the pointer is a borrowed buffer rather than an \
                             owning out-parameter, declare it '__unsafe_unretained' and \
                             store a borrowed reference -- which is what \
                             'countByEnumeratingWithState:objects:count:' does"
                                .to_string(),
                        ],
                    },
                );
                return;
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_out_parameter_stores(child, src, program, diags);
    }
}

pub fn check_autoreleasepool(root: Node, src: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_autoreleasepool(root, src, &mut diags);
    diags
}

fn walk_autoreleasepool(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    if node.kind() == "compound_statement" {
        let mut cursor = node.walk();
        if node.children(&mut cursor).next().map(|c| c.kind()) == Some("@autoreleasepool") {
            err(
                diags,
                src,
                node,
                "'@autoreleasepool' has no meaning in the static subset: there is no \
                 '-autorelease' -- ARC forbids the send, so nothing can ever be pending -- and \
                 no pool object, so a drain would have nothing to drain. Delete the keyword and \
                 keep the braces: a plain braced scope is what already ran, and ARC releases \
                 everything the scope owns at the closing brace",
            );
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_autoreleasepool(child, src, diags);
    }
}

pub fn check_manual_memory_sends(root: Node, src: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_manual_memory_sends(root, src, &mut diags);
    diags
}

fn walk_manual_memory_sends(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    if node.kind() == "message_expression" {
        if let Some(selector) = unary_selector(node, src) {
            if ARC_FORBIDDEN_SELECTORS.contains(&selector.as_str()) {
                err_detailed(diags, src, node, arc_forbidden_selector(&selector));
            }
        }
    }
    /* Declaring or defining one of the three is refused as well, and for
     * the reason `INTRINSIC_SELECTORS` gives: with every send of it a
     * located error, such a body could never run, and silently ignoring a
     * method someone wrote is the degradation this module exists to
     * prevent. Real ARC refuses the override too. `-dealloc` is the
     * exception in both places -- it is the cleanup hook, the deallocation
     * path calls it, and an override is how a class does its own teardown. */
    if matches!(node.kind(), "method_declaration" | "method_definition") {
        if let Some(name) = unary_method_name(node, src) {
            if ARC_FORBIDDEN_SELECTORS.contains(&name.as_str()) && name != "dealloc" {
                err(
                    diags,
                    src,
                    node,
                    format!(
                        "'-{name}' cannot be declared or defined: ARC is always enabled in the                          static subset and owns that selector, so every send of it is a located                          error -- this body could never run. Real ARC refuses the override too.                          (A '-dealloc' override *is* supported: it is the cleanup hook, and the                          chain above it is called automatically.)",
                        name = name
                    ),
                );
            }
        }
    }
    /* No early return on any kind: a send inside a block literal, inside a
     * `@synchronized` body, inside a nested initializer or inside a
     * `-dealloc` override is the same send. */
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children {
        walk_manual_memory_sends(child, src, diags);
    }
}

/// The selector of a message that takes no arguments, whatever its
/// receiver looks like.
///
/// Deliberately *not* `message_selector`: that one finds the receiver by
/// looking for the first `identifier` child, so a receiver that is not a
/// bare identifier -- `self->_ivar`, `arr[0]`, `[Thing alloc]`, `(Thing *)t`
/// -- leaves the selector's own identifier to be mistaken for the receiver
/// and yields an empty string. This reads the shape the grammar actually
/// produces, the way `emit::parse_message` does: `[`, receiver, selector,
/// `]`, so exactly two children once the brackets are filtered out.
fn unary_selector(node: Node, src: &str) -> Option<String> {
    let mut cursor = node.walk();
    let parts: Vec<Node> =
        node.children(&mut cursor).filter(|c| c.kind() != "[" && c.kind() != "]").collect();
    if parts.len() != 2 {
        return None;
    }
    Some(node_text(parts[1], src).trim().to_string())
}

/// The name of a method that takes no arguments -- a `method_declaration`
/// or `method_definition` with no `method_parameter` child at all.
///
/// A parameterized selector cannot be one of `ARC_FORBIDDEN_SELECTORS`, and
/// only the unary shape needs recognising, so the test is "no
/// `method_parameter`, and exactly one `identifier`". Read off the node's
/// own children the way `collect::extract_method_sig` does, so the two
/// agree about what a selector is.
fn unary_method_name(node: Node, src: &str) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    if children.iter().any(|c| c.kind() == "method_parameter") {
        return None;
    }
    let mut names = children.iter().filter(|c| c.kind() == "identifier");
    let first = names.next()?;
    if names.next().is_some() {
        return None;
    }
    Some(node_text(*first, src).trim().to_string())
}

/// What to say about a send of one of `ARC_FORBIDDEN_SELECTORS`.
///
/// Worded to agree with #430's `@autoreleasepool` rejection, since a reader
/// who hits one will hit the other: name the mechanism that is missing, and
/// name the thing to write instead.
/// The rejection for a send of a selector ARC owns.
///
/// Was one string with the diagnosis and every remedy fused into it --
/// 90 words for `-retain`, and a reader had to find the imperative
/// clause inside the prose. The parts are separate now, so the renderer
/// can put each on its own line (#457). The *text* of each is unchanged,
/// which is what keeps the suite's assertions about the remedy being
/// offered describing the same thing.
fn arc_forbidden_selector(selector: &str) -> Rejection {
    let message = format!("'-{}' cannot be sent here -- ARC owns this reference", selector);
    let note = "ARC is always enabled in the static subset: every Clang path here passes \
                -fobjc-arc, under which this is a compile error"
        .to_string();

    if selector == "retainCount" {
        /* The one forbidden selector with a direct replacement, so the
         * remedy is a rewrite rather than a deletion. */
        return Rejection {
            message,
            note: Some(note),
            help: vec![
                "reading a refcount is still supported -- call \
                 'oz_retain_count(obj)', a plain C function, which is the only \
                 refcount entry point Objective-C source may spell (#418)"
                    .to_string(),
                "it takes and gives no ownership, so nothing about the lifetime changes; \
                 only the spelling does"
                    .to_string(),
            ],
        };
    }

    if selector == "dealloc" {
        return Rejection {
            message,
            note: Some(format!(
                "{note}, and the deallocation path calls '-dealloc' for you \
                 (oz_release) with the superclass chain above an override called \
                 automatically -- so '[super dealloc]' is redundant, not required",
                note = note
            )),
            help: vec!["delete this line; keep the rest of the '-dealloc' body for cleanup \
                        that is not a reference release"
                .to_string()],
        };
    }

    Rejection {
        message,
        note: Some(format!(
            "{note}, so manual retain/release is a second ownership model -- and a release \
             ARC also emits is a double free",
            note = note
        )),
        help: vec![
            "let scope-based ARC manage the lifetime: a local is released at the end of its \
             scope, a store into a strong slot releases what it replaced, and an owned \
             object ivar is released with its owner"
                .to_string(),
            "to opt one slot out, declare the reference '__unsafe_unretained'".to_string(),
            "to read a refcount without taking ownership, call \
             'oz_retain_count(obj)', a plain C call"
                .to_string(),
        ],
    }
}

fn declared_name_nodes<'a>(decl: Node<'a>, out: &mut Vec<Node<'a>>) {
    let mut cursor = decl.walk();
    for child in decl.children(&mut cursor) {
        if !child.is_named() || DECL_TYPE_KINDS.contains(&child.kind()) {
            continue;
        }
        if let Some(name) = first_identifier(child) {
            out.push(name);
        }
    }
}

fn first_identifier<'a>(node: Node<'a>) -> Option<Node<'a>> {
    // `field_identifier` as well as `identifier`: a plain C struct spells a
    // field name with the former (`struct thing { uint8_t id; }`), an
    // ordinary declarator with the latter.
    if matches!(node.kind(), "identifier" | "field_identifier") {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = first_identifier(child) {
            return Some(found);
        }
    }
    None
}

fn find_capture(
    node: Node,
    src: &str,
    scope: &MethodScope,
    own_names: &HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    if node.kind() == "identifier" {
        let name = node_text(node, src);
        if own_names.contains(name) || scope.block_locals.contains(name) {
            return;
        }
        if name == "self" || scope.class_ivars.contains(name) || scope.locals.contains(name) {
            err(
                diags,
                src,
                node,
                format!(
                    "block captures '{}' from the enclosing scope; the static subset only accepts non-capturing blocks",
                    name
                ),
            );
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_capture(child, src, scope, own_names, diags);
    }
}

/// Objective-C node kinds that must not appear inside a `#define` body.
///
/// A `string_literal` is deliberately absent: the kind covers both `"foo"`
/// and `@"foo"`, and only the second is Objective-C. It is asked about
/// separately, via `emit::is_boxed_string_literal`.
const MACRO_BODY_OBJC_KINDS: &[&str] = &[
    "message_expression",
    "at_expression",
    "array_literal",
    "dictionary_literal",
    "selector_expression",
    "block_literal",
];

/// True if `node` or any descendant is Objective-C, for the macro-body probe.
fn contains_objc(node: Node) -> Option<Node> {
    if MACRO_BODY_OBJC_KINDS.contains(&node.kind()) {
        return Some(node);
    }
    if node.kind() == "string_literal" && crate::emit::is_boxed_string_literal(node) {
        return Some(node);
    }
    let children: Vec<Node> = {
        let mut cursor = node.walk();
        node.children(&mut cursor).collect()
    };
    for child in children {
        if let Some(found) = contains_objc(child) {
            return Some(found);
        }
    }
    None
}

/// True if the probe parse hit anything the grammar could not read.
///
/// The whole detector rests on this. tree-sitter is error-tolerant, so a
/// body that is not a valid fragment still yields a tree -- and that tree
/// can contain a `message_expression` the grammar guessed at from a `[`,
/// which would reject a macro containing nothing but C. So a body that does
/// not parse cleanly keeps today's behaviour of being emitted verbatim,
/// rather than becoming a spurious error.
fn probe_has_errors(node: Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let children: Vec<Node> = {
        let mut cursor = node.walk();
        node.children(&mut cursor).collect()
    };
    children.into_iter().any(probe_has_errors)
}

/// Reject Objective-C inside a `#define` body (#238).
///
/// tree-sitter-objc parses a replacement list as a single opaque
/// `preproc_arg` token with no structure inside it, so the walk never
/// descends into it and the body is emitted verbatim. Objective-C written
/// there therefore reaches the C compiler unchanged:
///
/// ```text
/// src2.c:102:2: error: expected expression
///   102 |         GREET_VIA_BODY(c);
/// src2.h:6:29: note: expanded from macro 'GREET_VIA_BODY'
///     6 | #define GREET_VIA_BODY(obj) [obj greet]
/// ```
///
/// Loud rather than silent, so nothing was ever miscompiled -- but the error
/// names generated code the user did not write, and no oz2c diagnostic
/// pointed at the `#define` responsible. The standing rule settles what to do
/// about that: never silently degrade, so this is a named, located hard error
/// at the `#define` itself.
///
/// **Detection is a probe re-parse**, not a regex: the `preproc_arg` text is
/// wrapped in a function body and parsed with the same grammar, and the
/// result searched for `MACRO_BODY_OBJC_KINDS`. Letting the grammar decide is
/// what distinguishes a real send from a C subscript -- `arr[i] + arr[j]` and
/// `[obj greet]` are not tellable apart by shape.
///
/// A *statement* wrapper rather than an expression one, which is wider than
/// the detector prototyped on #238: an expression wrapper cannot parse
/// `do { [o greet]; } while (0)`, and a macro body wrapping statements in
/// `do { ... } while (0)` is an ordinary idiom rather than an exotic one. It
/// still parses every C shape the prototype measured, since an expression is
/// also a statement.
///
/// Line continuations are stripped first. `preproc_arg` keeps the `\` and the
/// newline, which the probe grammar cannot read, so every multi-line macro
/// would fail to parse and be silently skipped by the guard above --
/// `OZ_SLAB_DEFINE`, `OZ_MEM_BLOCKS_DEFINE`, `oz_assert_msg` and
/// `OZ_AUTO_INIT` are all that shape. A detector that quietly ignores the
/// longest macro bodies in the tree is worse than one that says it cannot
/// read them.
///
/// Macro *arguments* are a different question and are already correct: an
/// argument is a real `message_expression` inside a `call_expression`, so the
/// ordinary expression renderer reaches it and the invocation is preserved
/// unexpanded. That is the deliberate advantage of parsing source rather than
/// a Clang AST, and it is pinned by `macro_bodies::objc_in_macro_argument_*`.
///
/// Full transpilation of macro bodies stays out of scope, and not only for
/// size: a macro body need not be a complete expression, and a macro
/// parameter has no type, so a send to one cannot resolve a receiver class at
/// all.
pub fn check_macro_body(node: Node, src: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let children: Vec<Node> = {
        let mut cursor = node.walk();
        node.children(&mut cursor).collect()
    };
    let Some(arg) = children.iter().find(|c| c.kind() == "preproc_arg") else {
        return diags;
    };
    let body = node_text(*arg, src).replace("\\\n", "\n").replace("\\\r\n", "\n");
    if body.trim().is_empty() {
        return diags;
    }

    let probe = format!("void _oz_macro_body_probe(void) {{\n{}\n;\n}}\n", body);
    let tree = crate::parse::parse(&probe);
    let root = tree.root_node();
    if probe_has_errors(root) {
        return diags;
    }
    let Some(found) = contains_objc(root) else {
        return diags;
    };

    // Located at the `#define`, not inside the probe: the probe's own
    // coordinates are meaningless to a reader, being offsets into a string
    // this function invented.
    let name = children
        .iter()
        .find(|c| c.kind() == "identifier")
        .map(|c| node_text(*c, src).to_string())
        .unwrap_or_else(|| "<anonymous>".to_string());
    let kind = if found.kind() == "string_literal" { "boxed string literal" } else { found.kind() };
    err(
        &mut diags,
        src,
        node,
        format!(
            "Objective-C in the body of macro '{}' is not supported: a #define body is \
             not transpiled, so the {} would reach the C compiler unchanged. Move the \
             Objective-C to a function, or pass it as a macro *argument* -- an argument \
             is transpiled and the macro invocation is preserved.",
            name, kind
        ),
    );
    diags
}

pub fn check_method_body(
    body: Node,
    src: &str,
    program: &Program,
    class_info: &ClassInfo,
    params: &[(String, String)],
    selector: &str,
    sizing: Option<&Sizing>,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    if INTRINSIC_SELECTORS.contains(&selector) {
        err(
            &mut diags,
            src,
            body,
            format!(
                "'{}' is answered at compile time from the whole-program class set and cannot be overridden -- this body would never run",
                selector
            ),
        );
    }
    let ivar_names: HashSet<String> =
        program.all_ivars(&class_info.name).into_iter().map(|(n, _)| n).collect();
    let owned_ivars: HashSet<String> =
        program.owned_object_ivar_names(&class_info.name).into_iter().collect();
    let managed = crate::emit::managed_object_locals(body, src, program);
    let mut scope = MethodScope {
        class_ivars: &ivar_names,
        owned_object_ivars: &owned_ivars,
        arc_managed_locals: &managed,
        locals: HashSet::new(),
        block_locals: HashSet::new(),
        pools: sizing.map(|s| s.pools),
        types: sizing.map(|s| s.types.clone()).unwrap_or_default(),
    };
    for (name, ty) in params {
        scope.locals.insert(name.clone());
        /* A parameter shadows an ivar of the same name, here as in C, so
         * this overwrites rather than `or_insert`. */
        scope.types.insert(name.clone(), ty.clone());
    }
    walk_for_reject(body, src, program, &mut scope, false, false, &mut diags);
    diags
}

/// The same accept/reject scan, over a plain top-level C function's body.
///
/// A `.m` file's file-scope functions -- `main()` above all -- can contain
/// Objective-C, and `emit` transpiles it there exactly as it does in a
/// method. The bar, however, was entered from one place only: the
/// `@implementation` method-body renderer. So every check was skipped for
/// code in a free function -- not just the allocation rule but `@try`,
/// reflection selectors, `@selector`/`@protocol`, `@synchronized` bodies with
/// an escaping jump, and block captures of stack locals.
///
/// Most of those fail loudly anyway, by reaching `emit` and producing C that
/// does not compile. The allocation rule was the one with a *silent*
/// consequence: pool sizing counts a site once however many times it runs, so
/// an unbounded loop in `main()` was sized as though it allocated once, and
/// that surfaced at run time as an unexpected nil rather than at build time
/// as a diagnostic.
///
/// No `MethodScope` mode is needed for this. `class_ivars` distinguishes an
/// ivar from anything else -- `find_capture` asks whether a name a block
/// closes over is one, and `assignment_escape` asks whether a store's
/// destination is a strong slot the emitter manages (#405, #423) -- and a
/// free function has no ivars, so the empty set is not a stand-in but the
/// truth: a store there is to a local, a parameter or a file-scope variable,
/// which is what those arms then conclude. Seeding it from some nearby class instead would
/// invent captures: `samples/gpio_demo`'s `[led toggle]` inside a block in
/// `main` would be flagged the moment any class in that file declared an ivar
/// named `led`.
pub fn check_function_body(
    body: Node,
    src: &str,
    program: &Program,
    sizing: Option<&Sizing>,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let no_ivars: HashSet<String> = HashSet::new();
    let managed = crate::emit::managed_object_locals(body, src, program);
    let mut scope = MethodScope {
        class_ivars: &no_ivars,
        owned_object_ivars: &no_ivars,
        arc_managed_locals: &managed,
        locals: HashSet::new(),
        block_locals: HashSet::new(),
        pools: sizing.map(|s| s.pools),
        types: sizing.map(|s| s.types.clone()).unwrap_or_default(),
    };
    walk_for_reject(body, src, program, &mut scope, false, false, &mut diags);
    diags
}

/// Is `node` (an `at_expression`) shaped like `@defs(Name)`?
///
/// The twin of `emit::is_protocol_literal_shape`, and the same node kind:
/// this grammar has no `@defs` rule at all, so `@defs(P)` parses as a
/// generic `at_expression` wrapping a `call_expression` whose callee
/// `identifier` is the text `defs`. `@protocol(P)` is byte-for-byte the
/// same tree with `protocol` in that position, so the identifier text is
/// the *only* thing separating an accepted construct from a refused one.
///
/// Two consequences worth stating, both learned by dumping the tree
/// (#563):
///
/// - This cannot be a check on `at_expression` alone. Four constructs
///   share that node -- `@42`, `@YES`, `@protocol(...)` and `@defs(...)`
///   -- and the first three are accepted.
/// - `@import` is *not* one of them. It has its own `module_import` node,
///   so no rule keyed on `at_expression` can reach it, which matters
///   because `@import` is deliberately left to Clang
///   (`oz2c-challenges/MUTATIONS.md` grades M68 `CLANG`, and that grade is
///   correct -- modules are a front-end feature, not a lowering gap).
fn is_defs_shape(node: Node, src: &str) -> bool {
    let mut cursor = node.walk();
    let Some(inner) = node.children(&mut cursor).find(|c| c.kind() != "@") else {
        return false;
    };
    if inner.kind() != "call_expression" {
        return false;
    }
    let mut c2 = inner.walk();
    let callee = inner.children(&mut c2).find(|c| c.kind() == "identifier");
    callee.is_some_and(|f| node_text(f, src) == "defs")
}

/// Is `node` an `ERROR` standing in for a `@try` with no handler?
///
/// tree-sitter builds a `try_statement` only once a `@catch` or `@finally`
/// follows, so the existing `try_statement` refusal never sees a bare
/// `@try` -- the parser hands back an `ERROR` node whose first child is
/// the `@try` token, with the block parsed normally underneath. #563 is
/// explicit that this is the same refusal, not a syntax complaint: the
/// construct is exceptions, and exceptions have no unwinding info here
/// whether or not a handler was written.
///
/// Matching `ERROR` is narrow on purpose -- the *first child* must be the
/// `@try` token. A general `ERROR` arm would turn every parse failure in
/// the file into an exceptions diagnostic.
fn is_handlerless_try(node: Node) -> bool {
    node.kind() == "ERROR" && node.child(0).is_some_and(|c| c.kind() == "@try")
}

/// The `@`-keywords oz2c forwards into the generated C, refused here
/// instead (#563).
///
/// # The gap this closes
///
/// The static subset accepts a *positive list* of `@`-keywords -- boxed
/// numeric and boolean literals, `@selector`, `@protocol(...)` as
/// `-conformsToProtocol:`'s argument -- and refuses a few more by name
/// (`@try`/`@catch`, `@synchronized`). A keyword in neither list was
/// **neither accepted nor refused**: tree-sitter parses it, no pass has a
/// case for it, and `emit`'s catch-all copies the source text through. The
/// first thing in the toolchain that understands it is then GCC, which
/// says `stray '@' in program` about a line the author did write and a
/// file they never wrote -- the generated `.c`.
///
/// None of the five has a meaning in this backend to lower to:
///
/// - `@encode` -- no runtime type strings; there is no reflection.
/// - `@throw` -- no unwinding info, and `@try`/`@catch` is already refused.
/// - `@available` -- a single-target static build has no OS to inquire of.
/// - `@defs` -- no ivar-layout object to hand out.
/// - a handler-less `@try` -- exceptions, same as the handled form.
///
/// # Why a named list and not "every unrecognised `@`-keyword"
///
/// Because the category is wrong even though it is tempting. `@import` is
/// an unrecognised `@`-keyword and must **not** be refused here: it is a
/// front-end feature, Clang's own dump rejects it, and
/// `oz2c-challenges/MUTATIONS.md` grades it `CLANG` -- delegated, and
/// correctly so. `@class` is another, and #564 *supports* it in the same
/// change rather than refusing it. So the set is five names, each with its
/// own reason, not a sweep over what the grammar happens not to cover.
///
/// # Placement
///
/// One walk over the whole tree rather than arms in `walk_for_reject`,
/// because position is exactly what this family gets wrong. `@defs` in the
/// corpus fixture sits at **file scope**, and `walk_for_reject` is entered
/// only with a method or function *body* -- so a body-only arm would have
/// left the reported case untouched while appearing to cover it. The three
/// expression keywords can appear in either place.
///
/// Deferred rather than gating, on #540's rule: none of these makes the
/// class table unwalkable, so each should co-report with whatever else the
/// file is wrong about.
pub fn check_at_keywords(source: &str) -> Vec<Diagnostic> {
    let tree = crate::parse::parse(source);
    let mut diags = Vec::new();
    walk_at_keywords(tree.root_node(), source, &mut diags);
    diags
}

fn walk_at_keywords(node: Node, src: &str, diags: &mut Vec<Diagnostic>) {
    let refusal: Option<Rejection> = match node.kind() {
        "encode_expression" => Some(Rejection {
            message: "'@encode' is not in the static subset -- there are no runtime type \
                      strings to read"
                .to_string(),
            note: Some(
                "'@encode(T)' is a compile-time operator yielding Objective-C's type \
                 encoding for T, which only a runtime that reads those strings can use. \
                 This backend has no reflection and no type-encoding table, so there is \
                 nothing to lower it to -- left alone it reaches the C compiler as \
                 `stray '@' in program`, pointing into the generated file"
                    .to_string(),
            ),
            help: vec![
                "if a type's size is what is wanted, use 'sizeof'".to_string(),
                "if a fixed tag is what is wanted, write the string literal directly"
                    .to_string(),
            ],
        }),
        "throw_statement" => Some(Rejection {
            message: "'@throw' is not in the static subset -- exceptions have no unwinding \
                      information here"
                .to_string(),
            note: Some(
                "the matching '@try'/'@catch' is refused for the same reason, so a throw \
                 could never be caught. Nothing unwinds the stack, and an escaping throw \
                 would leave every ARC release in every frame it passed unrun"
                    .to_string(),
            ),
            help: vec![
                "return a status value, or an out-parameter error, and check it at the \
                 call site"
                    .to_string(),
            ],
        }),
        "available_expression" => Some(Rejection {
            message: "'@available' is not in the static subset -- a single-target static \
                      build has no OS version to inquire against"
                .to_string(),
            note: Some(
                "'@available' asks the host's Objective-C runtime which OS it is running \
                 on, and answers at run time. This backend compiles to C for one fixed \
                 target chosen at build time, so the question has no subject"
                    .to_string(),
            ),
            help: vec![
                "branch on a build-time condition instead -- a Kconfig option, or '#if' \
                 on a macro the build defines"
                    .to_string(),
            ],
        }),
        "at_expression" if is_defs_shape(node, src) => Some(Rejection {
            message: "'@defs' is not in the static subset -- there is no ivar-layout \
                      object to hand out"
                .to_string(),
            note: Some(
                "'@defs(C)' is a GNU operator expanding to C's ivar layout so it can be \
                 embedded in a plain struct. oz2c already emits each class as a plain \
                 'struct C' whose ivars are ordinary members, so the layout is directly \
                 available and the operator has nothing to add"
                    .to_string(),
            ),
            help: vec!["name the generated type directly -- 'struct C'".to_string()],
        }),
        _ if is_handlerless_try(node) => Some(Rejection {
            message: "'@try' is not in the static subset -- exceptions have no unwinding \
                      information here"
                .to_string(),
            note: Some(
                "this '@try' has no '@catch' or '@finally', which the parser cannot build \
                 a 'try_statement' from -- so it arrives as a syntax error rather than as \
                 the construct it is. The refusal is the same either way; a handled \
                 '@try' is refused too"
                    .to_string(),
            ),
            help: vec![
                "remove the '@try' and keep its body, then return a status value for the \
                 failure it was guarding"
                    .to_string(),
            ],
        }),
        _ => None,
    };
    if let Some(rejection) = refusal {
        err_detailed(diags, src, node, rejection);
        /* One diagnostic per construct. A handler-less `@try` is an
         * `ERROR` node and its block parses normally underneath, so
         * descending would report anything inside it a second time
         * against a span the author cannot act on separately. */
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_at_keywords(child, src, diags);
    }
}

/// The per-class identifiers the generated C emits, as a set (#571).
///
/// # Why a set and not a prefix rule
///
/// Everything here is `<Class>` or `<Class>_something`, so a prefix test
/// looks equivalent and is not: a macro named `Widget_MAX` shares the
/// prefix and collides with nothing, because the generated C never emits
/// that name. Refusing it would reject ordinary source to catch an exotic
/// case. The set is exact, so the check can only fire on a name that is
/// really there.
///
/// # The drift this has to survive
///
/// These spellings live in `companion.rs` as `format!` strings, and a new
/// synthesized member added there would not appear here -- the set would
/// silently stop covering the thing it exists to cover. That is why
/// `macro_shadowing.rs::the_emitted_identifier_set_still_covers_the_output`
/// greps a real transpile's output for `<Class>`-prefixed identifiers and
/// drives the check with each one: the guard fails when `companion.rs`
/// grows a shape rather than when someone remembers to update this. It was
/// verified by sabotage -- dropping `oz_slab_` from the set below makes it
/// fail, naming that identifier.
///
/// `oz_slab_<Class>` is the one that does not start with the class name,
/// which is why this cannot be written as a suffix test either.
fn emitted_class_identifiers(program: &crate::model::Program) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
    for (class, info) in &program.classes {
        /* The struct tag. This is the one M93 trips: `struct <Class>` is a
         * bare token in the generated C, so an object-like macro named
         * `<Class>` rewrites it -- while `<Class>_run` is a *single* token
         * the preprocessor cannot reach into, so that one does not move. */
        out.insert(class.clone());
        out.insert(format!("oz_slab_{}", class));
        for suffix in [
            "oz_alloc",
            "oz_free",
            "oz_init",
            "oz_auto_init",
            "oz_release_ivars",
            "oz_dynamic_alloc_with_heap",
        ] {
            out.insert(format!("{}_{}", class, suffix));
        }
        /* Every declared method, through the same mangler emit calls, so
         * the two cannot disagree about `:` -> `_` or the `_cls` suffix. */
        for m in &info.methods {
            out.insert(crate::emit::method_fn_name(class, &m.selector, m.is_class_method));
        }
    }
    out
}

/// A `#define` whose name is an identifier the generated C emits (#571).
///
/// # What goes wrong
///
/// oz2c copies a file-scope `#define` into the generated header verbatim --
/// deliberately, because a macro may be a constant the emitted C needs --
/// and emits its own identifiers from the raw source token. When the two
/// namespaces collide, the copied macro rewrites *some* of oz2c's output
/// and not the rest, and the result does not type-check.
///
/// The reported case is `#define MT93Alias MT93Probe` over
/// `@interface MT93Alias`. What makes it fail is the **interaction with
/// oz2c's own include order**, which is worth stating because the copy
/// alone would be harmless:
///
/// ```text
/// #include "oz2c_dispatch.h"   <- declares MT93Alias_run(struct MT93Alias *)
/// #define MT93Alias MT93Probe  <- copied from source, AFTER that include
/// struct MT93Alias { ... };    <- now reads `struct MT93Probe`
/// int MT93Alias_run(struct MT93Alias *self);   <- and so does this one
/// ```
///
/// The function *name* is a single token the preprocessor cannot reach
/// into, so both declarations are called `MT93Alias_run` -- while their
/// parameter types are now `struct MT93Alias` and `struct MT93Probe`. Four
/// `conflicting types` errors, on generated lines, naming mangled symbols
/// the author never wrote.
///
/// # Why refused rather than dropped or namespaced
///
/// #571 offers two remedies: do not copy such a `#define`, or namespace
/// every emitted identifier so a user macro cannot reach it.
///
/// Namespacing is the larger change and the wrong one here -- the emitted
/// names *are* the runtime ABI, declared by hand in
/// `samples/smp_shared/main.m`, so renaming them is a break for every
/// consumer to buy back one exotic case.
///
/// Silently dropping the `#define` is worse than either: the macro may be
/// load-bearing for the author's own C, and removing it would turn a
/// compile error into a different compile error somewhere else, which is
/// exactly the "silently degrades" mode this transpiler does not have.
///
/// So it is refused, located at the `#define`, per the standing rule that
/// anything outside the subset is a hard located error. The remedy is the
/// one the issue already records: spell the class by its own name.
pub fn check_macro_shadows_emitted_name(
    node: Node,
    src: &str,
    program: &crate::model::Program,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let Some(name_node) = children.iter().find(|c| c.kind() == "identifier") else {
        return diags;
    };
    let name = node_text(*name_node, src);
    if !emitted_class_identifiers(program).contains(name) {
        return diags;
    }
    let is_class = program.is_class(name);
    let note = if is_class {
        format!(
            "'{name}' is a class in this program, so the generated C spells its type as \
             the two tokens `struct {name}` -- which this macro rewrites. Its methods are \
             single tokens like '{name}_...', which the preprocessor cannot reach into, so \
             those do not move. The generated header is also included *before* this \
             '#define' is copied in, so the shared dispatch declares the same functions \
             against the unrewritten type: the two disagree and neither is wrong on its own"
        )
    } else {
        format!(
            "'{name}' is a name oz2c synthesizes for a class in this program, so the \
             generated C already defines it. A macro of the same name rewrites the \
             occurrences that stand alone as tokens and leaves the rest, which the C \
             compiler then sees as two different declarations of one symbol"
        )
    };
    err_detailed(
        &mut diags,
        src,
        node,
        Rejection {
            message: format!(
                "macro '{}' has the same name as an identifier the generated C emits, so \
                 it would rewrite oz2c's own output",
                name
            ),
            note: Some(note),
            help: vec![
                if is_class {
                    "spell the class by its own name and delete the macro -- a class \
                     name reached through a '#define' is what this cannot support"
                        .to_string()
                } else {
                    format!("rename the macro so it does not collide with '{}'", name)
                },
                "or move the macro into a plain C header that the generated code does \
                 not include"
                    .to_string(),
            ],
        },
    );
    diags
}
