// SPDX-License-Identifier: Apache-2.0
//
// model.rs - data model for the OZ-091 Track B static-subset spike.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct MethodSig {
    pub is_class_method: bool,
    pub selector: String,
    pub return_type: String,
    pub params: Vec<(String, String)>, // (name, c_type)
    /// Was `return_type` spelled `instancetype` in source? `return_type`
    /// itself already resolved that to `struct {declaring_class} *` (see
    /// `collect::extract_method_sig`) -- callers dispatching this method
    /// through a *subclass*-typed receiver need this flag to know the
    /// call's real result type covaries with the receiver, not with the
    /// declaring class, and must be cast back up accordingly.
    pub returns_instancetype: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ownership {
    #[default]
    Strong,
    Assign,
    UnsafeUnretained,
}

/// A `@property` declaration, resolved against its `@synthesize` (explicit,
/// implicit-bare, or absent entirely) by the end of `collect::collect` --
/// `ivar_name` is only `None` transiently, between parsing the
/// `@property` and the property-resolution pass running (see
/// `collect::resolve_properties`). Mirrors the Python pipeline's
/// `OZProperty` (`tools/oz_transpile/model.py`).
#[derive(Debug, Clone)]
pub struct PropertyInfo {
    pub name: String,
    pub c_type: String,
    /// Whether `c_type` is an object pointer (a known class, or `id`/`void
    /// *`) -- only object properties get retain/release in a synthesized
    /// strong setter.
    pub is_object: bool,
    pub is_readonly: bool,
    pub is_nonatomic: bool,
    pub ownership: Ownership,
    pub getter_sel: Option<String>,
    pub setter_sel: Option<String>,
    pub ivar_name: Option<String>,
    /// Source location of the `@property` declaration itself, for
    /// diagnostics raised during property resolution (after parsing has
    /// moved past the original `Node`).
    pub decl_line: usize,
    pub decl_col: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ClassInfo {
    pub name: String,
    pub superclass: Option<String>,
    pub own_ivars: Vec<(String, String)>, // (name, c_type)
    pub methods: Vec<MethodSig>,
    pub has_class_initialize: bool,
    /// Protocol names declared directly on this class's `@interface`
    /// (`<Protocol, ...>`) -- not resolved through protocol inheritance;
    /// use `Program::protocol_methods` for that.
    pub conforms: Vec<String>,
    pub properties: Vec<PropertyInfo>,
    /// Ivars this class must NOT release when an instance is deallocated:
    /// those declared `__unsafe_unretained` in source, and those backing a
    /// property whose ownership is `assign`/`unsafe_unretained`. Tracked
    /// separately from `own_ivars` because `emit::lower_ivar_decl` strips
    /// the qualifier on the way into the generated struct (it means nothing
    /// to C), which would otherwise lose the only record that a reference
    /// is unowned -- and releasing an unowned backref is exactly the
    /// double-free the qualifier exists to prevent.
    pub unretained_ivars: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProtocolInfo {
    pub name: String,
    /// Protocols this one extends (`@protocol Name <Super, ...>`).
    pub super_protocols: Vec<String>,
    /// Methods declared directly by this protocol -- not resolved through
    /// `super_protocols`; use `Program::protocol_methods` for that.
    pub methods: Vec<MethodSig>,
}

#[derive(Debug, Default)]
pub struct Program {
    pub classes: HashMap<String, ClassInfo>,
    pub class_order: Vec<String>,
    pub protocols: HashMap<String, ProtocolInfo>,
    /// Methods whose every return path hands back a +1 reference, so a
    /// caller's local holding one must be released at scope exit -- see
    /// `arc`. Empty until `lib::transpile*` fills it in.
    pub owning_methods: crate::arc::OwningMethods,
    /// Ownership facts read from a Clang AST dump, when one was supplied
    /// (`--ast`). Clang resolves types; tree-sitter does not, so this is the
    /// only authority on whether an `id`-typed ivar is an object the class
    /// owns -- see `astinfo` and `owned_object_ivar_names`.
    pub ast: Option<crate::astinfo::AstFacts>,
}

impl Program {
    /// All ivars for `class_name`, root-first (superclass ivars before own).
    pub fn all_ivars(&self, class_name: &str) -> Vec<(String, String)> {
        let mut chain = Vec::new();
        let mut cur = Some(class_name.to_string());
        while let Some(name) = cur {
            let Some(info) = self.classes.get(&name) else {
                break;
            };
            chain.push(name.clone());
            cur = info.superclass.clone();
        }
        chain.reverse();
        let mut ivars = Vec::new();
        for name in chain {
            ivars.extend(self.classes[&name].own_ivars.clone());
        }
        ivars
    }

    pub fn is_class(&self, name: &str) -> bool {
        self.classes.contains_key(name)
    }

    /// The C access path from a `self` typed as `struct {from_class} *` to
    /// reach `ivar_name`: "_x" if `from_class` declares it itself, or
    /// "base._x" / "base.base._x" etc. if an ancestor does (struct
    /// embedding uses a named `base` field, not anonymous, so inherited
    /// members aren't directly reachable without the hop prefix).
    pub fn ivar_access_path(&self, from_class: &str, ivar_name: &str) -> Option<String> {
        let mut cur = Some(from_class.to_string());
        let mut hops = 0;
        while let Some(name) = cur {
            let info = self.classes.get(&name)?;
            if info.own_ivars.iter().any(|(n, _)| n == ivar_name) {
                return Some(format!("{}{}", "base.".repeat(hops), ivar_name));
            }
            cur = info.superclass.clone();
            hops += 1;
        }
        None
    }

    /// Every object ivar an instance of `class_name` owns, as the C access
    /// path to reach it from a `struct {class_name} *self` -- the ivars that
    /// have to be released when the instance is deallocated.
    ///
    /// Walks the whole superclass chain, because deallocating a subclass has
    /// to release what its ancestors own too. Excluded:
    ///
    ///   * anything in a class's `unretained_ivars` (declared
    ///     `__unsafe_unretained`, or backing an `assign`/`unsafe_unretained`
    ///     property) -- an unowned reference, whose release would be a
    ///     double-free;
    ///   * the synthesized tracking fields, which are not objects;
    ///   * `id`-typed ivars. `id` lowers to `void *`
    ///     (`collect::render_type`), which is indistinguishable from a
    ///     non-object pointer, and releasing a non-object crashes whereas
    ///     failing to release an object only leaks. The oracle releases
    ///     `id` ivars because Clang tells it which are objects; without
    ///     that this stays conservative rather than guessing.
    pub fn owned_object_ivars(&self, class_name: &str) -> Vec<String> {
        self.owned_object_ivar_names(class_name)
            .into_iter()
            .filter_map(|ivar| self.ivar_access_path(class_name, &ivar))
            .collect()
    }

    /// `owned_object_ivars` as plain ivar names rather than access paths --
    /// what `staticbar` needs to recognise one being released by hand.
    ///
    /// With a Clang AST supplied (`--ast`), Clang decides: it resolves types
    /// and, under `-fobjc-arc`, states each ivar's ownership outright, so an
    /// `id`-typed ivar is classified correctly instead of being skipped.
    /// Without one, the fallback below can only go on the spelling
    /// tree-sitter gives it, which is why it is deliberately narrow.
    pub fn owned_object_ivar_names(&self, class_name: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut cur = Some(class_name.to_string());
        while let Some(name) = cur {
            let Some(info) = self.classes.get(&name) else {
                break;
            };
            chain.push(name.clone());
            cur = info.superclass.clone();
        }
        let mut out = Vec::new();
        // Subclass-first: an owner releases what it added before what it
        // inherited, mirroring the oracle's dealloc-then-chain-to-parent.
        for name in chain {
            let info = &self.classes[&name];
            for (ivar, c_type) in &info.own_ivars {
                // Clang's answer wins wherever it has one. It knows the
                // resolved type and the ARC qualifier; the fallback knows
                // neither, and disagreeing with Clang here means either
                // leaking an object or releasing something that is not one.
                if let Some(facts) = &self.ast {
                    if let Some(owned) = facts.is_owned_object_ivar(&name, ivar) {
                        if owned {
                            out.push(ivar.clone());
                        }
                        continue;
                    }
                }
                if info.unretained_ivars.contains(ivar) {
                    continue;
                }
                if !c_type.trim_start().starts_with("struct ") || !c_type.contains('*') {
                    continue;
                }
                let Some(target) = c_type.trim().strip_prefix("struct ") else {
                    continue;
                };
                let target = target.trim_end_matches('*').trim();
                if !self.is_class(target) {
                    continue;
                }
                out.push(ivar.clone());
            }
        }
        out
    }

    /// Compile-time-fixed class id (index into class_order), used only for
    /// the dealloc const-vtable — never mutated at runtime.
    pub fn class_id(&self, name: &str) -> Option<usize> {
        self.class_order.iter().position(|n| n == name)
    }

    /// Does any class in the program declare an atomic (non-`nonatomic`)
    /// property? Determines whether the root struct needs an
    /// `oz_prop_lock` field at all -- see `collect::resolve_properties`.
    pub fn has_atomic_property(&self) -> bool {
        self.classes.values().any(|c| c.properties.iter().any(|p| !p.is_nonatomic))
    }

    pub fn root_class(&self) -> Option<&str> {
        self.class_order
            .iter()
            .find(|n| self.classes[*n].superclass.is_none())
            .map(|s| s.as_str())
    }

    /// Every method `protocol_name` requires, resolved transitively
    /// through `super_protocols` and deduped by (selector, is_class_method).
    /// Real Objective-C protocols aren't a runtime dispatch mechanism --
    /// they're a compile-time contract -- so this is used for conformance
    /// validation and for typing a protocol-typed variable, not for
    /// deciding which classes a generated dispatch function should route
    /// to (that's purely "who implements this selector," see
    /// `companion::render`).
    pub fn protocol_methods(&self, protocol_name: &str) -> Vec<MethodSig> {
        let mut seen: HashSet<(String, bool)> = HashSet::new();
        let mut out = Vec::new();
        let mut stack = vec![protocol_name.to_string()];
        let mut visited: HashSet<String> = HashSet::new();
        while let Some(name) = stack.pop() {
            if !visited.insert(name.clone()) {
                continue;
            }
            if let Some(p) = self.protocols.get(&name) {
                for m in &p.methods {
                    if seen.insert((m.selector.clone(), m.is_class_method)) {
                        out.push(m.clone());
                    }
                }
                stack.extend(p.super_protocols.clone());
            }
        }
        out
    }

    /// Every method declared by any protocol in the program, transitively
    /// resolved, deduped by (selector, is_class_method) across protocols
    /// too. One input to `dynamic_dispatch_methods` below, which is the
    /// actual set `OZ_PROTOCOL_SEND_*` dispatch functions get generated
    /// for (protocol-declared selectors are only part of that set).
    pub fn all_protocol_methods(&self) -> Vec<MethodSig> {
        let mut seen: HashSet<(String, bool)> = HashSet::new();
        let mut out = Vec::new();
        for name in self.protocols.keys() {
            for m in self.protocol_methods(name) {
                if seen.insert((m.selector.clone(), m.is_class_method)) {
                    out.push(m);
                }
            }
        }
        out
    }

    /// Is `selector` declared by any protocol in the program? Used as the
    /// fallback dispatch route when a message send's receiver type is
    /// known but doesn't itself (or via its superclass chain) implement
    /// the selector -- e.g. a root-typed variable holding some unknown
    /// conforming subclass, mirroring how a real ObjC protocol-typed
    /// receiver's concrete class isn't known statically either.
    pub fn is_protocol_selector(&self, selector: &str, is_class_method: bool) -> bool {
        self.all_protocol_methods()
            .iter()
            .any(|m| m.selector == selector && m.is_class_method == is_class_method)
    }

    /// Does this selector need a dynamic (`class_id`-switch) dispatch
    /// function generated for it at all? True when it's
    /// protocol-declared, when it's one of a fixed set of selectors that
    /// are always polymorphic by design (meaningful only via whatever the
    /// receiver's *actual* class overrides -- an object's own
    /// `-isEqual:`/`-cDescription:maxLength:`), or when more than one
    /// class in the program implements it. Class methods never qualify --
    /// a class-method receiver is always a literal class name, always
    /// statically known.
    ///
    /// This answers "which selectors get an `OZ_PROTOCOL_SEND_*`
    /// function", which is a program-wide question. Whether a given
    /// *call site* uses that function is decided separately, by class
    /// hierarchy analysis over the receiver's declared type (see
    /// `has_overriding_subclass`); the two differ, e.g. a selector
    /// implemented by two unrelated classes qualifies here, yet each
    /// call against either concrete type still compiles to a direct call.
    ///
    /// This is close to, but no longer identical with, the Python
    /// pipeline's `_classify_dispatch` (`tools/oz_transpile/resolve.py`),
    /// which additionally forces `dealloc` and `init` to be dynamic.
    /// oz_static needs neither: `dealloc` has its own const-vtable
    /// mechanism, and an overridden `init` is caught by the same
    /// hierarchy analysis as any other selector.
    pub fn is_dynamically_dispatched(&self, selector: &str, is_class_method: bool) -> bool {
        const ALWAYS_DYNAMIC: &[&str] = &["isEqual:", "cDescription:maxLength:"];
        if is_class_method {
            return false;
        }
        if self.is_protocol_selector(selector, false) {
            return true;
        }
        if ALWAYS_DYNAMIC.contains(&selector) {
            return true;
        }
        self.class_order
            .iter()
            .filter(|name| {
                self.classes[*name].methods.iter().any(|m| m.selector == selector && !m.is_class_method)
            })
            .count()
            > 1
    }

    /// Will `class_name`'s `selector` exist as a callable function in the
    /// generated output?
    ///
    /// A selector declared in an `@interface` and never defined anywhere is
    /// not callable, and emitting a call to it fails at *link* time with an
    /// undefined symbol rather than at transpile time with a located
    /// message. `countByEnumeratingWithState:objects:count:` is the real
    /// instance: declared by `OZArray.h`/`OZDictionary.h` (Foundation's
    /// NSFastEnumeration shape) and given no body by either `.m`, since
    /// neither pipeline's for-in uses it.
    ///
    /// Answerable only with a Clang AST; without one everything is assumed
    /// defined, which is the previous behavior. A `@synthesize`d accessor
    /// has no body in the AST either, so property accessors are treated as
    /// defined -- oz_static emits those itself
    /// (`emit::render_synthesized_accessor`).
    pub fn method_is_defined(&self, class_name: &str, selector: &str) -> bool {
        let Some(facts) = &self.ast else {
            return true;
        };
        if !facts.knows_class(class_name) {
            return true;
        }
        if facts.has_method_body(class_name, selector) {
            return true;
        }
        let Some(info) = self.classes.get(class_name) else {
            return true;
        };
        info.properties.iter().any(|prop| {
            prop.getter_sel.as_deref() == Some(selector)
                || prop.setter_sel.as_deref() == Some(selector)
                || prop.name == selector
                || crate::collect::default_setter_sel(&prop.name) == selector
        })
    }

    /// Is `name` a strict descendant of `ancestor` (i.e. `ancestor`
    /// appears somewhere up `name`'s superclass chain, `name` itself
    /// excluded)?
    pub fn is_descendant_of(&self, name: &str, ancestor: &str) -> bool {
        let mut current = self.classes.get(name).and_then(|c| c.superclass.clone());
        while let Some(sup) = current {
            if sup == ancestor {
                return true;
            }
            current = self.classes.get(&sup).and_then(|c| c.superclass.clone());
        }
        false
    }

    /// Does `class_name`, or any class up its superclass chain, conform
    /// to `protocol` -- directly (`ClassInfo::conforms`) or via a
    /// protocol that one extends (`ProtocolInfo::super_protocols`)?
    ///
    /// Used by `generics::check_program` to validate an `id<Proto>`-
    /// constrained value's concrete class -- the same question
    /// `render_interface`'s own conformance check answers for a class's
    /// *declared* protocols, generalized here to protocol inheritance
    /// and to an arbitrary value rather than a whole class's contract.
    /// Mirrors the oracle's `_class_conforms_to`
    /// (`tools/oz_transpile/resolve.py`), except that one does not walk
    /// protocol inheritance -- only a class's own declared list, checked
    /// up the superclass chain. Following inheritance too is strictly
    /// more correct and costs nothing extra to compute here, so this
    /// implementation isn't held back to match that gap.
    pub fn class_conforms_to(&self, class_name: &str, protocol: &str) -> bool {
        let extends = |declared: &[String]| -> bool {
            let mut stack: Vec<String> = declared.to_vec();
            let mut seen: HashSet<String> = HashSet::new();
            while let Some(p) = stack.pop() {
                if p == protocol {
                    return true;
                }
                if !seen.insert(p.clone()) {
                    continue;
                }
                if let Some(info) = self.protocols.get(&p) {
                    stack.extend(info.super_protocols.iter().cloned());
                }
            }
            false
        };
        let mut current = Some(class_name.to_string());
        while let Some(name) = current {
            let Some(info) = self.classes.get(&name) else { break };
            if extends(&info.conforms) {
                return true;
            }
            current = info.superclass.clone();
        }
        false
    }

    /// Does any strict subclass of `class_name` implement `selector`?
    ///
    /// This is the class-hierarchy-analysis test that decides whether a
    /// message send against a receiver *declared* as `class_name` can be
    /// devirtualized into a direct call. A declared type is only an upper
    /// bound on the receiver's real class -- `Base *b = (Base *)[Sub
    /// alloc];` is still a `Sub` -- so a direct call to the declared
    /// type's own implementation is sound only when no subclass could
    /// have overridden it. oz_static sees the whole program as one
    /// translation unit, so this analysis is exact rather than
    /// conservative.
    pub fn has_overriding_subclass(&self, class_name: &str, selector: &str) -> bool {
        self.class_order.iter().any(|name| {
            self.is_descendant_of(name, class_name)
                && self.classes[name]
                    .methods
                    .iter()
                    .any(|m| m.selector == selector && !m.is_class_method)
        })
    }

    /// Every distinct dynamically-dispatched instance selector in the
    /// program (see `is_dynamically_dispatched`), each with a
    /// representative signature (params/return type) taken from
    /// whichever class declares it first in source order -- callers
    /// only need the signature to render one dispatch function per
    /// selector, not to know every implementor (see
    /// `companion::render_protocol_dispatch`, which looks up
    /// implementors itself).
    pub fn dynamic_dispatch_methods(&self) -> Vec<MethodSig> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut out = Vec::new();
        for name in &self.class_order {
            for m in &self.classes[name].methods {
                if m.is_class_method || seen.contains(&m.selector) {
                    continue;
                }
                if self.is_dynamically_dispatched(&m.selector, false) {
                    seen.insert(m.selector.clone());
                    out.push(m.clone());
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>, line: usize, col: usize) -> Self {
        Diagnostic { message: message.into(), line, col }
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}
