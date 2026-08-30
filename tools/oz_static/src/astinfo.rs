// SPDX-License-Identifier: Apache-2.0
//
// astinfo.rs - facts read from a Clang AST JSON dump that oz_static cannot
// safely derive on its own.
//
// oz_static parses with tree-sitter, which gives it syntax but no type
// resolution: it can see that an ivar is written `id _thing`, not whether
// `_thing` is an object the class owns. That distinction decides whether a
// generated dealloc releases the ivar, and getting it wrong is not a
// cosmetic matter -- releasing a non-object corrupts memory, and the
// conservative alternative (skip anything not obviously a class pointer)
// silently leaks every `id`-typed ivar.
//
// Clang already knows. With `-fobjc-arc` it writes the ARC ownership
// qualifier directly into each declaration's `qualType`, so the AST dump the
// Python pipeline already produces is an authoritative answer to exactly the
// question oz_static cannot answer alone. This module reads that answer;
// nothing else here depends on Clang, and with no AST supplied oz_static
// falls back to its own conservative rule (see
// `model::Program::owned_object_ivars`).

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// Per-class ivar ownership, keyed `(class, ivar)`.
#[derive(Debug, Default)]
pub struct AstFacts {
    owned_object: HashMap<(String, String), bool>,
    /// Classes the dump actually described, so a caller can tell "Clang says
    /// this ivar is not owned" from "Clang never saw this class" -- only the
    /// former is a fact worth acting on.
    classes: HashSet<String>,
}

impl AstFacts {
    /// Parse a `clang -Xclang -ast-dump=json` dump.
    ///
    /// Only `ObjCIvarDecl` nodes are of interest, so the whole tree is
    /// walked but nothing else is retained -- these dumps run to megabytes
    /// (6.8 MB for one Foundation-importing case) and almost none of it is
    /// about ownership.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let root: Value =
            serde_json::from_str(text).map_err(|e| format!("not valid Clang AST JSON: {}", e))?;
        let mut facts = AstFacts::default();
        facts.walk(&root, None);
        Ok(facts)
    }

    fn walk(&mut self, node: &Value, owner: Option<&str>) {
        let kind = node.get("kind").and_then(Value::as_str).unwrap_or("");
        // An @implementation re-declares its class's ivars, so both node
        // kinds establish the same owner; taking either is correct.
        let owner = if matches!(kind, "ObjCInterfaceDecl" | "ObjCImplementationDecl") {
            node.get("name").and_then(Value::as_str).or(owner)
        } else {
            owner
        };
        if kind == "ObjCIvarDecl" {
            if let (Some(class), Some(ivar)) = (owner, node.get("name").and_then(Value::as_str)) {
                let qual = node
                    .get("type")
                    .and_then(|t| t.get("qualType"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                self.classes.insert(class.to_string());
                self.owned_object
                    .insert((class.to_string(), ivar.to_string()), is_owned_object(qual));
            }
        }
        if let Some(children) = node.get("inner").and_then(Value::as_array) {
            for child in children {
                self.walk(child, owner);
            }
        }
    }

    /// Whether `class`'s `ivar` is an object the class owns, or `None` if
    /// this dump says nothing about it -- an unknown class, or an ivar Clang
    /// never saw. Callers must not read `None` as "not owned".
    pub fn is_owned_object_ivar(&self, class: &str, ivar: &str) -> Option<bool> {
        self.owned_object.get(&(class.to_string(), ivar.to_string())).copied()
    }

    /// Did the dump describe `class` at all?
    pub fn knows_class(&self, class: &str) -> bool {
        self.classes.contains(class)
    }

    pub fn is_empty(&self) -> bool {
        self.owned_object.is_empty()
    }
}

/// Does this `qualType` describe an object the declaring class owns, and so
/// must release when an instance is deallocated?
///
/// Under `-fobjc-arc` Clang spells the ownership qualifier into the type,
/// and *where* it sits is what distinguishes the cases -- verified against
/// real dumps of every shape in this codebase:
///
/// | `qualType`                     | owned | why                             |
/// |--------------------------------|-------|---------------------------------|
/// | `__strong id`                  | yes   | the ivar is the object          |
/// | `OZObject *__strong`           | yes   | the pointer itself is strong    |
/// | `__unsafe_unretained id`       | no    | unowned backref                 |
/// | `__strong id *`                | no    | a *buffer* of objects, not one  |
/// | `const char *`, `int`          | no    | not an object                   |
/// | `void (^__strong)(__strong id)`| no    | see below                       |
///
/// The `__strong id *` row is why this reads position rather than merely
/// searching for `__strong`: there the qualifier belongs to the pointee, and
/// the ivar is a raw buffer that `OZArray`/`OZDictionary` free as memory
/// rather than release as an object.
///
/// Block ivars are excluded even though ARC does own them. oz_static lowers
/// a block to a plain C function pointer (`emit::lower_ivar_decl`), so there
/// is no object to release and passing one to a release call would treat
/// code as a heap object.
pub fn is_owned_object(qual_type: &str) -> bool {
    let qual = qual_type.trim();
    if qual.contains("(^") {
        return false;
    }
    match qual.rfind('*') {
        // No pointer: a bare object type such as `id`, whose qualifier is
        // written as a prefix.
        None => qual.split_whitespace().any(|token| token == "__strong"),
        // With a pointer, only a qualifier *after* the last `*` describes the
        // ivar itself; anything before it describes what is pointed at.
        Some(star) => qual[star + 1..].contains("__strong"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row of the table in `is_owned_object`'s doc comment, each string
    /// copied from a real `clang -ast-dump=json` run over this repo's own
    /// sources rather than invented.
    #[test]
    fn ownership_rule_matches_real_qualtypes() {
        assert!(is_owned_object("__strong id"));
        assert!(is_owned_object("OZObject *__strong"));
        assert!(is_owned_object("Item *__strong"));
        assert!(is_owned_object("OZDefer *__strong"));

        assert!(!is_owned_object("__unsafe_unretained id"));
        assert!(!is_owned_object("__unsafe_unretained id *"));
        assert!(!is_owned_object("__strong id *"));
        assert!(!is_owned_object("const char *"));
        assert!(!is_owned_object("int"));
        assert!(!is_owned_object("unsigned int"));
        assert!(!is_owned_object("uint16_t"));
        assert!(!is_owned_object("void (^__strong)(__strong id)"));
    }

    /// An unqualified object type means the dump was produced without
    /// `-fobjc-arc`, so it carries no ownership information at all. Reporting
    /// "not owned" is the safe reading: it leaks rather than double-frees.
    #[test]
    fn unqualified_object_type_is_not_treated_as_owned() {
        assert!(!is_owned_object("id"));
        assert!(!is_owned_object("OZObject *"));
    }

    #[test]
    fn walks_ivars_and_attributes_them_to_their_class() {
        let json = r#"{
          "kind": "TranslationUnitDecl",
          "inner": [
            {"kind": "ObjCInterfaceDecl", "name": "Holder", "inner": [
              {"kind": "ObjCIvarDecl", "name": "_item", "type": {"qualType": "Item *__strong"}},
              {"kind": "ObjCIvarDecl", "name": "_value", "type": {"qualType": "int"}}
            ]},
            {"kind": "ObjCInterfaceDecl", "name": "Watcher", "inner": [
              {"kind": "ObjCIvarDecl", "name": "_seen",
               "type": {"qualType": "__unsafe_unretained id"}}
            ]}
          ]
        }"#;
        let facts = AstFacts::from_json(json).expect("parses");
        assert_eq!(facts.is_owned_object_ivar("Holder", "_item"), Some(true));
        assert_eq!(facts.is_owned_object_ivar("Holder", "_value"), Some(false));
        assert_eq!(facts.is_owned_object_ivar("Watcher", "_seen"), Some(false));
        // Absent, as opposed to known-not-owned.
        assert_eq!(facts.is_owned_object_ivar("Holder", "_nope"), None);
        assert_eq!(facts.is_owned_object_ivar("Nobody", "_x"), None);
        assert!(facts.knows_class("Holder"));
        assert!(!facts.knows_class("Nobody"));
    }

    #[test]
    fn rejects_input_that_is_not_json() {
        assert!(AstFacts::from_json("this is not json").is_err());
    }
}
