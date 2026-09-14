// SPDX-License-Identifier: Apache-2.0
//
// astinfo.rs - facts read from a Clang AST JSON dump that oz2c cannot
// safely derive on its own.
//
// oz2c parses with tree-sitter, which gives it syntax but no type
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
// question oz2c cannot answer alone. This module reads that answer;
// nothing else here depends on Clang, and with no AST supplied oz2c
// falls back to its own conservative rule (see
// `model::Program::owned_object_ivars`).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// The handful of fields the oracle reads out of a Clang AST node.
///
/// Deserializing into this rather than `serde_json::Value` is what keeps
/// these dumps affordable. A `Value` tree allocates a `Map<String, Value>`
/// per node and a `String` per key -- `id`, `loc`, `range`, `mangledName`,
/// `valueCategory` and the rest -- all of which the walk immediately
/// discards. On px-keyboard that was 742 MB of JSON becoming a peak of
/// 1.30 GB resident (#299). Everything not named here is skipped by serde
/// without being materialised, and the two string fields borrow out of the
/// file text.
///
/// `Cow` rather than `&str`: a borrowed `&str` cannot represent a JSON
/// string that needs unescaping, and serde fails rather than allocating.
/// Neither a C identifier nor a type spelling should ever contain an
/// escape, but failing to parse a dump over a hypothetical one would be a
/// hard error in the ownership oracle, and that is not a trade worth
/// making to save an allocation that almost never happens.
#[derive(serde::Deserialize)]
struct Node<'a> {
    #[serde(borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    name: Option<Cow<'a, str>>,
    #[serde(rename = "type", borrow, default)]
    ty: Option<TypeRef<'a>>,
    /// Clang's own node identity, and the only way to tell one node from a
    /// second printing of the same node -- see `WalkState::seen_marks`.
    #[serde(borrow, default)]
    id: Option<Cow<'a, str>>,
    /// `ARCProduceObject` and its siblings ride on an `ImplicitCastExpr`'s
    /// `castKind`, which is the whole of what makes them findable.
    #[serde(rename = "castKind", borrow, default)]
    cast_kind: Option<Cow<'a, str>>,
    #[serde(default)]
    loc: Option<Loc>,
    #[serde(default)]
    range: Option<Range>,
    #[serde(borrow, default)]
    inner: Vec<Node<'a>>,
}

#[derive(serde::Deserialize)]
struct TypeRef<'a> {
    #[serde(rename = "qualType", borrow, default)]
    qual_type: Option<Cow<'a, str>>,
}

/// One of Clang's source locations, as it is actually written in the dump.
///
/// **Every field is optional, and that is the point.** Clang delta-encodes
/// these against the location it printed last: a dump repeats `file` only
/// when the file changes and `line` only when the line changes, so the
/// overwhelming majority of locations carry nothing but `offset` and `col`.
/// Measured on a 701 KB dump of a 29-line source: 1,389 locations carry a
/// position, **10** of them name a file, 333 name a line, and 1,056 are an
/// offset alone. Reading one of these nodes on its own therefore yields a
/// position that cannot be resolved; resolving it is a stateful fold over
/// the walk, which is what `WalkState` is (#453).
///
/// No lifetime parameter: `file` appears on ~10 nodes in a whole dump, so
/// owning those few strings costs nothing measurable and keeps this type
/// free of the borrow plumbing every other field here needs.
#[derive(serde::Deserialize, Default)]
struct Loc {
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    offset: Option<u64>,
    /// Where the token came from inside a macro *definition*. Absorbed for
    /// its delta effect and never adopted as a position: a reader told that
    /// an ownership question sits inside `OZ_LOG`'s body cannot act on it.
    #[serde(rename = "spellingLoc", default)]
    spelling_loc: Option<Box<Loc>>,
    /// Where the macro was *used*. This is the position a reader can act
    /// on, so it is what a macro-nested location resolves to.
    ///
    /// Not a corner: **888 of 32,465 locations across 22 real dumps are
    /// nested this way** rather than carrying flat fields. Reading them as
    /// empty would silently attribute every mark inside a macro expansion
    /// to whatever enclosing statement happened to have a flat location --
    /// approximately right, and wrong without saying so.
    #[serde(rename = "expansionLoc", default)]
    expansion_loc: Option<Box<Loc>>,
}

/// A node's source range. Only `begin` is read: it is the position a
/// diagnostic points at, and an expression's `end` is what its last
/// subexpression already reports.
#[derive(serde::Deserialize, Default)]
struct Range {
    #[serde(default)]
    begin: Option<Loc>,
    #[serde(default)]
    end: Option<Loc>,
}

/// A resolved source position: which file, which line, which byte.
///
/// `offset` as well as `line` because ARC's marks are per-expression and a
/// line routinely holds several: `Slot *s = [Slot alloc];` carries both the
/// `+1` and the binding that consumes it. The offset is what distinguishes
/// two sites on one line, and it is also what
/// `imports::ResolvedSource::source_location` speaks, so it is the common
/// currency between Clang's view of the file and oz2c's merged buffer.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AstPos {
    pub file: String,
    pub line: u32,
    pub offset: u64,
}

impl std::fmt::Display for AstPos {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}", self.file, self.line)
    }
}

/// An ARC transfer Clang marked, and the syntactic position it sits in.
///
/// The position is not decoration: **it is the whole discriminator.**
/// Measured over 22 dumps of the `arc`, `memory` and `lifecycle` corpora,
/// `ARCProduceObject` on a `ReturnStmt` appears identically on a method
/// returning `[Thing alloc]`, one returning a borrowed ivar, and one
/// returning its own parameter -- and identically again whether or not the
/// selector is in the create-rule family, so it says nothing about the
/// caller-side transfer (`newThing`, `copyWithZone:` and `makeThing` carry
/// the same pair). What separates them is the *other* mark and where it
/// sits: a `+1` consumed inside the body shows as `ARCConsumeObject` at the
/// position that consumed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArcMark {
    /// `ARCProduceObject`, `ARCConsumeObject`, `ARCReclaimReturnedObject`
    /// or `ARCExtendBlockObject`, verbatim as Clang spells it.
    pub kind: String,
    /// The nearest enclosing node kind that an ownership question is asked
    /// in -- `VarDecl`, `BinaryOperator`, `ReturnStmt`, `ObjCMessageExpr`,
    /// `CStyleCastExpr` and so on. Derived from real dumps rather than
    /// invented; see `POSITION_KINDS`.
    pub position: String,
    pub at: AstPos,
}

/// An ownership qualifier Clang wrote into a declaration's type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnershipQual {
    /// `VarDecl`, `ParmVarDecl`, `FieldDecl` or `ObjCIvarDecl`.
    pub decl_kind: String,
    pub name: String,
    /// `__strong`, `__weak`, `__unsafe_unretained` or `__autoreleasing`.
    pub qualifier: String,
    pub at: AstPos,
}

/// The node kinds that count as "a position an ownership question is asked
/// in", for the purpose of attributing a mark.
///
/// Every entry was observed carrying a mark in a dump of this repo's own
/// corpora, except the compound-assignment, literal and control-flow kinds,
/// which are here because oz2c asks the ownership question in them
/// (`arc::hoists_owning_operand`, the collection literals of #449) and
/// their absence from the corpus is a gap in the corpus rather than a fact
/// about ARC. A kind that is *not* listed leaves the position inherited
/// from further out, which is the conservative direction: the mark is still
/// recorded, attributed to the nearest position that is named.
const POSITION_KINDS: &[&str] = &[
    "VarDecl",
    "ParmVarDecl",
    "FieldDecl",
    "ObjCIvarDecl",
    "ReturnStmt",
    "BinaryOperator",
    "CompoundAssignOperator",
    "ObjCMessageExpr",
    "CallExpr",
    "CStyleCastExpr",
    "ArraySubscriptExpr",
    "InitListExpr",
    "ConditionalOperator",
    "ObjCBoxedExpr",
    "ObjCArrayLiteral",
    "ObjCDictionaryLiteral",
    "ObjCForCollectionStmt",
    "IfStmt",
    "WhileStmt",
    "ForStmt",
    "SwitchStmt",
    "CompoundStmt",
];

/// The ARC transfer marks, as Clang's `castKind` spells them.
const ARC_MARK_KINDS: &[&str] = &[
    "ARCProduceObject",
    "ARCConsumeObject",
    "ARCReclaimReturnedObject",
    "ARCExtendBlockObject",
];

/// The four ARC ownership qualifiers, longest first.
///
/// Order matters: `__unsafe_unretained` contains no other entry as a
/// substring, but matching must not stop at a prefix, and searching
/// longest-first makes that impossible to get wrong by rearranging.
const OWNERSHIP_QUALIFIERS: &[&str] =
    &["__unsafe_unretained", "__autoreleasing", "__strong", "__weak"];

/// The carried state a location fold needs: the last file and line Clang
/// printed, and the node ids whose marks have already been recorded.
///
/// **The dedupe set is load-bearing, not hygiene.** Clang prints a
/// `VarDecl`'s initializer twice -- once in the enclosing method's
/// declaration list and again under its `CompoundStmt`'s `DeclStmt` -- with
/// the same node `id` both times. Measured: a dump of one three-line method
/// yields 6 mark nodes at 5 distinct ids. Without the set every binding's
/// `+1` is counted twice and an audit reports a discrepancy that does not
/// exist.
#[derive(Default)]
struct WalkState {
    file: Option<String>,
    line: Option<u32>,
    seen_marks: HashSet<String>,
}

impl WalkState {
    /// Apply one location to the carried state, returning it resolved if it
    /// names a byte at all.
    ///
    /// Called in the order Clang prints the fields -- `loc`, then
    /// `range.begin`, then `range.end` -- because that order *is* the
    /// encoding: each location is a delta against whatever was printed
    /// immediately before it, not against its parent.
    fn absorb(&mut self, loc: &Loc) -> Option<AstPos> {
        if let Some(file) = &loc.file {
            self.file = Some(file.clone());
        }
        if let Some(line) = loc.line {
            self.line = Some(line);
        }
        if let Some(offset) = loc.offset {
            return Some(AstPos {
                file: self.file.clone()?,
                line: self.line?,
                offset,
            });
        }
        /* No flat position. Either the location is empty -- 2,050 of the
         * 32,465 measured are `{}` -- or it is macro-nested, in which case
         * the expansion is the position a reader can act on. The spelling
         * is absorbed first so its delta effect is not lost, then
         * discarded. */
        if let Some(spelling) = &loc.spelling_loc {
            self.absorb(spelling);
        }
        if let Some(expansion) = &loc.expansion_loc {
            return self.absorb(expansion);
        }
        None
    }
}

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
    /// bodies" made oz2c drop the declaration of everything the SDK
    /// implements in `src/*.m` -- `OZ_PROTOCOL_SEND_getDescription_maxLength_`
    /// among them -- while still emitting the calls, so the generated C
    /// stopped compiling. Now the guard abstains unless this dump really
    /// covered the class's implementation.
    implemented_classes: HashSet<String>,
    /// Every ARC transfer Clang marked, in walk order, deduplicated by node
    /// id. Recorded for *every* file the dump covers, not only the source
    /// under transpilation: this module does not know which file a caller
    /// cares about, and filtering here would throw away the only copy.
    /// A caller narrows with `marks_in`.
    ///
    /// Worth knowing what the ratio is before reading a total: of 152 marks
    /// over the `arc`, `memory` and `lifecycle` corpora, **45 -- 29.6% --
    /// were in the SDK's own sources rather than the case under test**, and
    /// all 45 were the same shape (`ARCProduceObject` on a `ReturnStmt`).
    /// An unfiltered report's largest row is SDK boilerplate.
    arc_marks: Vec<ArcMark>,
    /// Every declaration Clang wrote an ownership qualifier into, in walk
    /// order. Same whole-dump scope as `arc_marks`, and the same skew: the
    /// first fifteen in a dump of a 29-line case were all in
    /// `include/oz_sdk`. Bounded by measurement rather than hope -- a real
    /// Zephyr build's 56 MB of dumps carry 954 qualifier occurrences in
    /// total.
    ownership_quals: Vec<OwnershipQual>,
}

impl AstFacts {
    /// Parse a `clang -Xclang -ast-dump=json` dump.
    ///
    /// Only a few node kinds are of interest, so the whole tree is walked
    /// but nothing else is retained -- and almost none of it is about
    /// ownership. The dumps are far larger than "megabytes": one
    /// `#include <zephyr/kernel.h>` in a 485-line file produces **117 MB**,
    /// because Clang serialises the entire header closure (#299).
    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut facts = AstFacts::default();
        /* A stream rather than one document, because `-ast-dump-filter`
         * emits one top-level object per matching declaration, concatenated
         * -- `serde_json::from_str` rejects that as "trailing characters".
         * A single document is a stream of one, so this is a strict
         * superset of what was accepted before. */
        let mut stream = serde_json::Deserializer::from_str(text).into_iter::<Node>();
        let mut saw_any = false;
        /* One `WalkState` across the whole stream, not one per document:
         * the location encoding is a delta against the last location
         * *printed*, and `-ast-dump-filter` concatenating several top-level
         * declarations does not restart that. */
        let mut state = WalkState::default();
        for doc in &mut stream {
            let node = doc.map_err(|e| format!("not valid Clang AST JSON: {}", e))?;
            facts.walk(&node, None, &mut state, None, None);
            saw_any = true;
        }
        if !saw_any {
            return Err("not valid Clang AST JSON: no top-level declaration".to_string());
        }
        Ok(facts)
    }

    /// `from_json` for a dump on disk, reading and dropping it here.
    ///
    /// The caller used to read every dump into a `Vec<String>` and hold all
    /// of them while each was parsed in turn, so peak memory was the whole
    /// set at once -- 742 MB of text plus the tree built over it. Reading
    /// one, parsing it, and letting both go keeps the peak at a single
    /// dump.
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read '{}': {}", path.display(), e))?;
        Self::from_json(&text)
    }

    /// Walk one node, carrying four things down: which class we are inside
    /// (`owner`), the mutable location fold (`state`), the innermost
    /// resolved position (`here`), and the innermost syntactic position an
    /// ownership question is asked in (`position`).
    ///
    /// `here` is inherited rather than read per node because **the mark
    /// node carries no location of its own.** An `ImplicitCastExpr` -- the
    /// node `castKind` rides on -- has a `range` and no `loc`, and in the
    /// dumps measured for #453 even the range resolved only through the
    /// carried file. A mark's position is therefore its nearest enclosing
    /// node's, which is also the position a reader would point at.
    fn walk(
        &mut self,
        node: &Node,
        owner: Option<&str>,
        state: &mut WalkState,
        here: Option<AstPos>,
        position: Option<&str>,
    ) {
        let kind = node.kind.as_deref().unwrap_or("");
        /* Absorb in Clang's own print order -- see `WalkState::absorb`. */
        let mut here = here;
        if let Some(loc) = &node.loc {
            if let Some(pos) = state.absorb(loc) {
                here = Some(pos);
            }
        }
        if let Some(range) = &node.range {
            if let Some(begin) = &range.begin {
                if let Some(pos) = state.absorb(begin) {
                    here = Some(pos);
                }
            }
            /* Absorbed for its delta effect on the carried file and line,
             * and deliberately not adopted as the position: a node's end is
             * not where a diagnostic should point. */
            if let Some(end) = &range.end {
                state.absorb(end);
            }
        }
        if let Some(cast) = node.cast_kind.as_deref() {
            if ARC_MARK_KINDS.contains(&cast) {
                let id = node.id.as_deref().unwrap_or("");
                /* An empty id cannot be deduplicated, so it is recorded:
                 * a mark reported twice is a false discrepancy, but a mark
                 * dropped is a missed one, and only the latter is silent. */
                if id.is_empty() || state.seen_marks.insert(id.to_string()) {
                    if let Some(at) = here.clone() {
                        self.arc_marks.push(ArcMark {
                            kind: cast.to_string(),
                            position: position.unwrap_or("<unattributed>").to_string(),
                            at,
                        });
                    }
                }
            }
        }
        if matches!(kind, "VarDecl" | "ParmVarDecl" | "FieldDecl" | "ObjCIvarDecl") {
            let qual = node.ty.as_ref().and_then(|t| t.qual_type.as_deref()).unwrap_or("");
            if let Some(found) = OWNERSHIP_QUALIFIERS.iter().find(|q| qual.contains(**q)) {
                if let (Some(name), Some(at)) = (node.name.as_deref(), here.clone()) {
                    self.ownership_quals.push(OwnershipQual {
                        decl_kind: kind.to_string(),
                        name: name.to_string(),
                        qualifier: (*found).to_string(),
                        at,
                    });
                }
            }
        }
        let position = if POSITION_KINDS.contains(&kind) { Some(kind) } else { position };
        // An @implementation re-declares its class's ivars, so both node
        // kinds establish the same owner; taking either is correct.
        let owner = if matches!(kind, "ObjCInterfaceDecl" | "ObjCImplementationDecl") {
            node.name.as_deref().or(owner)
        } else {
            owner
        };
        if kind == "ObjCImplementationDecl" {
            if let Some(name) = node.name.as_deref() {
                self.implemented_classes.insert(name.to_string());
            }
        }
        if kind == "ObjCMethodDecl" {
            // A definition carries its body as an inner CompoundStmt; a bare
            // `@interface` declaration has none. A `@synthesize`d accessor
            // also has none, which is why callers must ask
            // `Program::method_is_defined` rather than reading this directly
            // -- oz2c generates those itself.
            if let (Some(class), Some(selector)) = (owner, node.name.as_deref()) {
                let has_body = node
                    .inner
                    .iter()
                    .any(|c| c.kind.as_deref() == Some("CompoundStmt"));
                if has_body {
                    self.defined_methods.insert((class.to_string(), selector.to_string()));
                }
                self.classes.insert(class.to_string());
            }
        }
        if kind == "ObjCIvarDecl" {
            if let (Some(class), Some(ivar)) = (owner, node.name.as_deref()) {
                let qual = node
                    .ty
                    .as_ref()
                    .and_then(|t| t.qual_type.as_deref())
                    .unwrap_or("");
                self.classes.insert(class.to_string());
                self.owned_object
                    .insert((class.to_string(), ivar.to_string()), is_owned_object(qual));
            }
        }
        for child in &node.inner {
            self.walk(child, owner, state, here.clone(), position);
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
        self.arc_marks.extend(other.arc_marks);
        self.ownership_quals.extend(other.ownership_quals);
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

    /// Every ARC transfer mark the dumps carry, in walk order.
    pub fn arc_marks(&self) -> &[ArcMark] {
        &self.arc_marks
    }

    /// Every ownership-qualified declaration the dumps carry.
    pub fn ownership_quals(&self) -> &[OwnershipQual] {
        &self.ownership_quals
    }

    /// The marks whose file path ends with `suffix`.
    ///
    /// A suffix rather than an equality test because the two sides spell
    /// the same file differently: Clang echoes the path as it was given on
    /// its command line (`tests/behavior/cases/arc/x.m`) while a caller
    /// holds whatever path *it* was given, which may be absolute. Matching
    /// on the tail is what makes those meet without either side
    /// canonicalising a path that may not exist on this machine.
    ///
    /// This is the filter that turns a dump-wide total into a statement
    /// about one source -- see `arc_marks` for why 29.6% of a corpus's
    /// marks are not in the file under test.
    pub fn marks_in(&self, suffix: &str) -> Vec<&ArcMark> {
        self.arc_marks.iter().filter(|m| m.at.file.ends_with(suffix)).collect()
    }

    /// The ownership-qualified declarations whose file path ends with
    /// `suffix`. Same matching rule as `marks_in`.
    pub fn quals_in(&self, suffix: &str) -> Vec<&OwnershipQual> {
        self.ownership_quals.iter().filter(|q| q.at.file.ends_with(suffix)).collect()
    }

    /// Every fact the merged dumps carry, as sorted lines -- what
    /// `oz2c --dump-ast-facts` prints.
    ///
    /// This exists to make a change to the AST path *provable*. Ingesting
    /// these dumps is 97.5% of oz2c's wall clock on px-keyboard, so it is
    /// where the optimisation pressure is (#299), and the failure mode of
    /// getting it wrong is the oracle answering one question fewer -- which
    /// is a silent leak, not a failed build. Diffing the generated C is the
    /// weaker check: it only catches facts that happen to matter for one
    /// program today. Diffing this catches the oracle itself weakening.
    ///
    /// All four sets are included, not just the two with live callers
    /// (`is_owned_object_ivar`, `has_method_body`): a refactor that dropped
    /// `classes` or `implemented_classes` would be invisible in generated C
    /// until something started reading them again.
    ///
    /// Sorted, so two runs over the same dumps compare byte for byte.
    pub fn dump_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for ((class, ivar), owned) in &self.owned_object {
            let owned = if *owned { "owned" } else { "unowned" };
            lines.push(format!("ivar {} {} {}", class, ivar, owned));
        }
        for (class, selector) in &self.defined_methods {
            lines.push(format!("method {} {}", class, selector));
        }
        for class in &self.classes {
            lines.push(format!("class {}", class));
        }
        for class in &self.implemented_classes {
            lines.push(format!("impl {}", class));
        }
        /* The marks and the qualifiers join the fact set for the reason
         * stated above: a refactor that stopped reading them would
         * otherwise be invisible in generated C, because nothing in the
         * emitted output depends on them -- `--check-arc` is an audit, not
         * a codegen input.
         *
         * The byte offset is printed as well as the line, and the lines are
         * **not** deduplicated. Both follow from what this baseline is
         * diffed across: two builds of oz2c over the *same* dumps, where
         * the offset is fixed and is the only thing distinguishing two
         * marks on one line -- `Slot *s = [Slot alloc];` carries the `+1`
         * and the binding that consumes it. Collapsing equal lines would
         * hide one of a pair, which is the silent direction. */
        for mark in &self.arc_marks {
            lines.push(format!(
                "mark {} {} {}+{}",
                mark.kind, mark.position, mark.at, mark.at.offset
            ));
        }
        for qual in &self.ownership_quals {
            lines.push(format!(
                "qual {} {} {} {}+{}",
                qual.decl_kind, qual.name, qual.qualifier, qual.at, qual.at.offset
            ));
        }
        lines.sort();
        lines
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
/// Block ivars are excluded even though ARC does own them. oz2c lowers
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
    /// The `enumerationIndex` row is the trap -- no body either, but only because it
    /// is `@synthesize`d, and oz2c does emit that accessor.
    #[test]
    fn distinguishes_definitions_from_bare_declarations() {
        let json = r#"{
          "kind": "TranslationUnitDecl",
          "inner": [
            {"kind": "ObjCInterfaceDecl", "name": "OZArray", "inner": [
              {"kind": "ObjCMethodDecl", "name": "count"},
              {"kind": "ObjCMethodDecl", "name": "countByEnumeratingWithState:objects:count:"},
              {"kind": "ObjCMethodDecl", "name": "enumerationIndex"}
            ]},
            {"kind": "ObjCImplementationDecl", "name": "OZArray", "inner": [
              {"kind": "ObjCMethodDecl", "name": "count",
               "inner": [{"kind": "CompoundStmt"}]},
              {"kind": "ObjCMethodDecl", "name": "enumerationIndex"}
            ]}
          ]
        }"#;
        let facts = AstFacts::from_json(json).expect("parses");
        assert!(facts.has_method_body("OZArray", "count"));
        assert!(!facts.has_method_body("OZArray", "countByEnumeratingWithState:objects:count:"));
        assert!(!facts.has_method_body("OZArray", "enumerationIndex"));
        assert!(facts.knows_class("OZArray"));
    }

    /// A dump excerpt in the shape Clang really emits: **the file is named
    /// once**, and every location after it carries an offset alone.
    ///
    /// Written this way deliberately. A fixture that repeated `"file"` on
    /// each location would pass whether or not the fold exists, which is
    /// the vacuous-test failure this repo has paid for before -- so the
    /// only location here that names a file is the first, and the two marks
    /// below it can only resolve if the carried state works.
    ///
    /// The `ImplicitCastExpr` nodes carry `range` and no `loc`, and the
    /// binding's initializer is printed twice under one id, both copied
    /// from a real `clang -Xclang -ast-dump=json -fobjc-arc` run over
    /// `tests/behavior/cases/arc/reassign_releases_old.m`.
    fn delta_encoded_dump() -> &'static str {
        r#"{
          "kind": "TranslationUnitDecl",
          "inner": [
            {
              "id": "0x1",
              "kind": "ObjCImplementationDecl",
              "name": "ArcReassignTest",
              "loc": { "file": "cases/arc/reassign_releases_old.m", "line": 17, "offset": 300 },
              "inner": [
                {
                  "id": "0x2",
                  "kind": "ObjCMethodDecl",
                  "name": "run",
                  "loc": { "line": 18, "offset": 320 },
                  "inner": [
                    {
                      "id": "0x3",
                      "kind": "VarDecl",
                      "name": "s",
                      "loc": { "line": 20, "offset": 347 },
                      "type": { "qualType": "Slot *__strong" },
                      "inner": [
                        {
                          "id": "0xDUP",
                          "kind": "ImplicitCastExpr",
                          "castKind": "ARCConsumeObject",
                          "range": { "begin": { "offset": 352 }, "end": { "offset": 366 } }
                        }
                      ]
                    },
                    {
                      "id": "0x4",
                      "kind": "CompoundStmt",
                      "inner": [
                        {
                          "id": "0x5",
                          "kind": "DeclStmt",
                          "inner": [
                            {
                              "id": "0x3",
                              "kind": "VarDecl",
                              "name": "s",
                              "loc": { "line": 20, "offset": 347 },
                              "type": { "qualType": "Slot *__strong" },
                              "inner": [
                                {
                                  "id": "0xDUP",
                                  "kind": "ImplicitCastExpr",
                                  "castKind": "ARCConsumeObject",
                                  "range": { "begin": { "offset": 352 } }
                                }
                              ]
                            }
                          ]
                        },
                        {
                          "id": "0x6",
                          "kind": "BinaryOperator",
                          "range": { "begin": { "line": 21, "offset": 369 } },
                          "inner": [
                            {
                              "id": "0x7",
                              "kind": "ImplicitCastExpr",
                              "castKind": "ARCConsumeObject",
                              "range": { "begin": { "offset": 371 } }
                            }
                          ]
                        }
                      ]
                    }
                  ]
                }
              ]
            }
          ]
        }"#
    }

    /// The fold resolves a location that names neither file nor line.
    ///
    /// This is the constraint the whole design rests on: measured on a
    /// 701 KB dump of a 29-line source, 10 of 1,389 positions name a file
    /// and 1,056 are an offset alone, so a per-node read yields nothing
    /// resolvable.
    #[test]
    fn resolves_locations_clang_delta_encoded() {
        let facts = AstFacts::from_json(delta_encoded_dump()).expect("parses");
        let marks = facts.arc_marks();
        assert_eq!(marks.len(), 2, "two distinct marks: {:#?}", marks);
        for mark in marks {
            assert_eq!(
                mark.at.file, "cases/arc/reassign_releases_old.m",
                "the file was named once, on a node far above this mark"
            );
        }
        /* The binding's mark inherits line 20 from the `VarDecl`; the
         * reassignment's own range named line 21, so it must not have
         * inherited. */
        assert_eq!((marks[0].at.line, marks[0].at.offset), (20, 352));
        assert_eq!((marks[1].at.line, marks[1].at.offset), (21, 371));
    }

    /// A mark printed twice under one node id is recorded once.
    ///
    /// Clang prints a `VarDecl`'s initializer both in the enclosing
    /// method's declaration list and under its `CompoundStmt`'s
    /// `DeclStmt`. Counting it twice would report a discrepancy at every
    /// binding in the program -- and every binding is where the corpus's
    /// largest population of marks lives (49 of 107 in-file marks).
    #[test]
    fn a_node_printed_twice_is_one_mark() {
        let facts = AstFacts::from_json(delta_encoded_dump()).expect("parses");
        let at_352: Vec<_> =
            facts.arc_marks().iter().filter(|m| m.at.offset == 352).collect();
        assert_eq!(at_352.len(), 1, "id 0xDUP appears twice in the dump: {:#?}", at_352);
    }

    /// A mark is attributed to the position it sits in, not to its own node
    /// kind -- every mark is an `ImplicitCastExpr`, which says nothing.
    ///
    /// The position is the discriminator, and that is measured rather than
    /// assumed: `ARCProduceObject` on a `ReturnStmt` appears identically on
    /// a method returning `[Thing alloc]`, one returning a borrowed ivar,
    /// and one returning its own parameter, and identically again whether
    /// or not the selector is in the create-rule family.
    #[test]
    fn a_mark_is_attributed_to_its_enclosing_position() {
        let facts = AstFacts::from_json(delta_encoded_dump()).expect("parses");
        let positions: Vec<&str> =
            facts.arc_marks().iter().map(|m| m.position.as_str()).collect();
        assert_eq!(positions, vec!["VarDecl", "BinaryOperator"]);
    }

    /// The qualifier on a declaration is recorded with its position.
    #[test]
    fn records_the_ownership_qualifier_on_a_local() {
        let facts = AstFacts::from_json(delta_encoded_dump()).expect("parses");
        let quals = facts.ownership_quals();
        assert_eq!(quals.len(), 2, "the VarDecl is printed twice: {:#?}", quals);
        assert_eq!(quals[0].decl_kind, "VarDecl");
        assert_eq!(quals[0].name, "s");
        assert_eq!(quals[0].qualifier, "__strong");
        assert_eq!(quals[0].at.line, 20);
    }

    /// `marks_in` narrows a dump-wide total to one source.
    ///
    /// Not a convenience: 45 of 152 marks over the `arc`, `memory` and
    /// `lifecycle` corpora -- 29.6% -- are in the SDK's own sources rather
    /// than the case under test, and all 45 are the same shape. An
    /// unfiltered report's largest single row is boilerplate.
    #[test]
    fn marks_in_filters_to_one_file() {
        let facts = AstFacts::from_json(delta_encoded_dump()).expect("parses");
        assert_eq!(facts.marks_in("reassign_releases_old.m").len(), 2);
        assert_eq!(facts.marks_in("src/OZObject.m").len(), 0);
        /* A suffix match, because Clang echoes the path it was given while
         * a caller may hold an absolute one. */
        assert_eq!(facts.marks_in("arc/reassign_releases_old.m").len(), 2);
    }

    /// Reading the rest of the dump does not weaken what it already read.
    ///
    /// `dump_lines` is the provable record (#299), so the new rows must be
    /// additive: every ivar, method, class and impl line the oracle
    /// produced before is still produced.
    #[test]
    fn the_new_rows_are_additive() {
        let facts = AstFacts::from_json(delta_encoded_dump()).expect("parses");
        let lines = facts.dump_lines();
        assert!(lines.iter().any(|l| l == "impl ArcReassignTest"), "{:#?}", lines);
        assert!(lines.iter().any(|l| l.starts_with("mark ARCConsumeObject VarDecl ")));
        assert!(lines.iter().any(|l| l.starts_with("qual VarDecl s __strong ")));
        /* Two marks and two qualifier rows, neither collapsed: equal lines
         * would mean two sites, and hiding one is the silent direction. */
        assert_eq!(lines.iter().filter(|l| l.starts_with("mark ")).count(), 2);
        assert_eq!(lines.iter().filter(|l| l.starts_with("qual ")).count(), 2);
    }

    /// A location nested inside a macro expansion resolves to the
    /// expansion, not the spelling.
    ///
    /// 888 of 32,465 locations across 22 real dumps are shaped this way, so
    /// reading them as empty is not a corner case. The concrete instance in
    /// this repo: `+ (instancetype)alloc { return nil; }` -- `nil` is a
    /// macro, so the mark on its cast is macro-nested, and resolving the
    /// expansion moves the reported position from the `return` keyword
    /// (offset 2632) onto `nil` itself (2639).
    #[test]
    fn a_macro_nested_location_resolves_to_the_expansion() {
        let dump = r#"{
          "kind": "TranslationUnitDecl",
          "loc": { "file": "src/OZObject.m", "line": 82, "offset": 2600 },
          "inner": [
            {
              "id": "0x1",
              "kind": "ObjCMethodDecl",
              "name": "alloc",
              "loc": { "line": 83, "offset": 2620 },
              "inner": [
                {
                  "id": "0x2",
                  "kind": "ReturnStmt",
                  "range": { "begin": { "line": 84, "offset": 2632 } },
                  "inner": [
                    {
                      "id": "0x3",
                      "kind": "ImplicitCastExpr",
                      "castKind": "ARCProduceObject",
                      "range": {
                        "begin": {
                          "spellingLoc": { "file": "stubs/objc.h", "line": 9, "offset": 120 },
                          "expansionLoc": { "file": "src/OZObject.m", "line": 84, "offset": 2639 }
                        }
                      }
                    }
                  ]
                }
              ]
            }
          ]
        }"#;
        let facts = AstFacts::from_json(dump).expect("parses");
        let marks = facts.arc_marks();
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].at.file, "src/OZObject.m", "not the macro's own header");
        assert_eq!(marks[0].at.offset, 2639, "`nil`, not the `return` above it");
        assert_eq!(marks[0].position, "ReturnStmt");
    }

    /// An empty `loc` leaves the inherited position alone rather than
    /// dropping the mark.
    ///
    /// 2,050 of the 32,465 measured locations are `{}`. Dropping a mark for
    /// want of a position of its own would lose it silently, which is the
    /// one direction this transpiler is not allowed to fail in.
    #[test]
    fn an_empty_location_inherits_rather_than_drops() {
        let dump = r#"{
          "kind": "TranslationUnitDecl",
          "loc": { "file": "x.m", "line": 3, "offset": 30 },
          "inner": [
            {
              "id": "0x1",
              "kind": "VarDecl",
              "name": "s",
              "loc": { "line": 4, "offset": 44 },
              "type": { "qualType": "Thing *__strong" },
              "inner": [
                {
                  "id": "0x2",
                  "kind": "ImplicitCastExpr",
                  "castKind": "ARCConsumeObject",
                  "loc": {},
                  "range": { "begin": {} }
                }
              ]
            }
          ]
        }"#;
        let facts = AstFacts::from_json(dump).expect("parses");
        let marks = facts.arc_marks();
        assert_eq!(marks.len(), 1, "the mark is kept: {:#?}", marks);
        assert_eq!((marks[0].at.line, marks[0].at.offset), (4, 44), "the VarDecl's");
        assert_eq!(marks[0].position, "VarDecl");
    }

    #[test]
    fn rejects_input_that_is_not_json() {
        assert!(AstFacts::from_json("this is not json").is_err());
    }
}
