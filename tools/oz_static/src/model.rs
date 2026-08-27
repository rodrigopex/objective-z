// SPDX-License-Identifier: Apache-2.0
//
// model.rs - data model for the OZ-091 Track B static-subset spike.

use std::collections::HashMap;

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
}

#[derive(Debug, Default)]
pub struct Program {
    pub classes: HashMap<String, ClassInfo>,
    pub class_order: Vec<String>,
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
