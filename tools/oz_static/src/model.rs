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

    /// Compile-time-fixed class id (index into class_order), used only for
    /// the dealloc const-vtable — never mutated at runtime.
    pub fn class_id(&self, name: &str) -> Option<usize> {
        self.class_order.iter().position(|n| n == name)
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
    /// too. The set of selectors `OZ_PROTOCOL_SEND_*` dispatch functions
    /// get generated for.
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
