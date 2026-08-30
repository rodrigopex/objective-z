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
    /// `(class, selector)` for every method the dump shows with a real body.
    /// A selector declared in an `@interface` and never defined in any
    /// `@implementation` is absent -- which is the point: emitting a call to
    /// one produces a link error, not a compile error, so the mistake
    /// surfaces at the wrong end of the pipeline.
    defined_methods: HashSet<(String, String)>,
    /// Classes the dump actually described, so a caller can tell "Clang says
    /// this ivar is not owned" from "Clang never saw this class" -- only the
    /// former is a fact worth acting on.
    classes: HashSet<String>,
    /// Classes the dump saw an `@implementation` *for*, which is a stricter
    /// thing than `classes` and the only sound basis for concluding a method
    /// is undefined.
    ///
    /// Seeing a class's `@interface` says nothing about where its methods
    /// are defined: Clang preprocesses `#import`s, so a dump of one `.m`
    /// carries every interface it imports and no other file's
    /// implementations. Treating "interface seen" as "I would have seen the
    /// bodies" made oz_static drop the declaration of everything the SDK
    /// implements in `src/*.m` -- `OZ_PROTOCOL_SEND_cDescription_maxLength_`
    /// among them -- while still emitting the calls, so the generated C
    /// stopped compiling. Now the guard abstains unless this dump really
    /// covered the class's implementation.
    implemented_classes: HashSet<String>,
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
        if kind == "ObjCImplementationDecl" {
            if let Some(name) = node.get("name").and_then(Value::as_str) {
                self.implemented_classes.insert(name.to_string());
            }
        }
        if kind == "ObjCMethodDecl" {
            // A definition carries its body as an inner CompoundStmt; a bare
            // `@interface` declaration has none. A `@synthesize`d accessor
            // also has none, which is why callers must ask
            // `Program::method_is_defined` rather than reading this directly
            // -- oz_static generates those itself.
            if let (Some(class), Some(selector)) =
                (owner, node.get("name").and_then(Value::as_str))
            {
                let has_body = node
                    .get("inner")
                    .and_then(Value::as_array)
                    .is_some_and(|children| {
                        children.iter().any(|c| {
                            c.get("kind").and_then(Value::as_str) == Some("CompoundStmt")
                        })
                    });
                if has_body {
                    self.defined_methods.insert((class.to_string(), selector.to_string()));
                }
                self.classes.insert(class.to_string());
            }
        }
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

    /// Fold another dump's facts into this one.
    ///
    /// A program built from several `.m` files needs one dump per file: a
    /// dump of `main.m` sees every `@interface` it imports but only the
    /// `@implementation`s written in that one file, so on its own it would
    /// report every *other* class's methods as never defined. Unioning is
    /// the right operation for every set here -- each dump is a partial
    /// view, and none contradicts another.
    pub fn merge(&mut self, other: Self) {
        self.owned_object.extend(other.owned_object);
        self.defined_methods.extend(other.defined_methods);
        self.classes.extend(other.classes);
        self.implemented_classes.extend(other.implemented_classes);
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

    /// Did the dump cover `class`'s `@implementation`? See the field of the
    /// same name for why this, and not `knows_class`, gates any conclusion
    /// that a method has no definition.
    pub fn knows_implementation_of(&self, class: &str) -> bool {
        self.implemented_classes.contains(class)
    }

    /// Does the dump show `class` defining `selector` with a body?
    ///
    /// Only meaningful together with `knows_class`: `false` for a class the
    /// dump never mentioned means "no information", not "not defined".
    pub fn has_method_body(&self, class: &str, selector: &str) -> bool {
        self.defined_methods.contains(&(class.to_string(), selector.to_string()))
    }

    pub fn is_empty(&self) -> bool {
        self.owned_object.is_empty() && self.defined_methods.is_empty()
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

    /// `countByEnumeratingWithState:objects:count:` is the real case this
    /// exists for: declared in `OZArray.h`, never defined in `OZArray.m`.
    /// The `iterIdx` row is the trap -- no body either, but only because it
    /// is `@synthesize`d, and oz_static does emit that accessor.
    #[test]
    fn distinguishes_definitions_from_bare_declarations() {
        let json = r#"{
          "kind": "TranslationUnitDecl",
          "inner": [
            {"kind": "ObjCInterfaceDecl", "name": "OZArray", "inner": [
              {"kind": "ObjCMethodDecl", "name": "count"},
              {"kind": "ObjCMethodDecl", "name": "countByEnumeratingWithState:objects:count:"},
              {"kind": "ObjCMethodDecl", "name": "iterIdx"}
            ]},
            {"kind": "ObjCImplementationDecl", "name": "OZArray", "inner": [
              {"kind": "ObjCMethodDecl", "name": "count",
               "inner": [{"kind": "CompoundStmt"}]},
              {"kind": "ObjCMethodDecl", "name": "iterIdx"}
            ]}
          ]
        }"#;
        let facts = AstFacts::from_json(json).expect("parses");
        assert!(facts.has_method_body("OZArray", "count"));
        assert!(!facts.has_method_body("OZArray", "countByEnumeratingWithState:objects:count:"));
        assert!(!facts.has_method_body("OZArray", "iterIdx"));
        assert!(facts.knows_class("OZArray"));
    }

    #[test]
    fn rejects_input_that_is_not_json() {
        assert!(AstFacts::from_json("this is not json").is_err());
    }
}
