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
    /// Was this declared under a protocol's `@optional` marker (#536)?
    ///
    /// Only ever true for a method collected from a `@protocol` body; a
    /// class's own declarations have no such marker and are all `false`.
    /// The marker is **sticky** and applies to everything after it until a
    /// `@required` resets it, which is how Objective-C defines it and how
    /// tree-sitter models it -- each marker gets its own
    /// `qualified_protocol_interface_declaration` wrapping the
    /// declarations that follow, as a *sibling* of any previous one rather
    /// than nested inside it, so the value is read on entry and not
    /// inherited.
    ///
    /// Exactly one consumer: `emit::render_interface`'s conformance check,
    /// which must not require an optional member of a conformer. Nothing
    /// else may filter on it, and in particular
    /// `Program::all_protocol_methods` must keep returning optional
    /// members -- they still need an `OZ_PROTOCOL_SEND_*` dispatch
    /// function, because `-respondsToSelector:` is how a caller tests for
    /// one and then sends it.
    pub is_optional: bool,
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

/// Which of Objective-C's three `@interface`/`@implementation` block shapes a
/// declaration is.
///
/// `collect::class_header` used to answer this with `Option<String>` -- the
/// category's name, or `None` -- and that cannot tell a **class extension**
/// (`@interface Foo ()`, an unnamed category) apart from the class's own
/// primary `@interface Foo`. In tree-sitter-objc's grammar the category name
/// is an optional field of one shared `class_interface` node, so both shapes
/// came back `None`, and an extension was therefore treated as a second,
/// complete declaration of the class: a second `struct Foo` with a *different
/// layout*, a second set of prototypes, and a second `Foo_oz_alloc` body
/// (#529).
///
/// The three-way distinction is not cosmetic, because the two parenthesised
/// shapes disagree on the one question the emitter has to answer. A class
/// extension is part of the class: it **may** declare ivars, and its
/// properties **do** get a backing ivar and a synthesized accessor. A
/// category may declare neither -- it has no storage of its own to add one to,
/// and adding one to the extended class changes that class's layout behind the
/// back of every other translation unit (#530).
///
/// So ask `may_declare_ivars`, never "did the header have parentheses":
/// parentheses are true of *both* parenthesised shapes, and keying on them
/// strips the storage from exactly the one case that needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceKind {
    /// `@interface Foo : Bar` / `@implementation Foo` -- the declaration that
    /// brings the class into existence.
    Primary,
    /// `@interface Foo ()` -- an unnamed category, i.e. a class extension. It
    /// declares no new class; everything in it merges into the primary
    /// declaration.
    Extension,
    /// `@interface Foo (Name)` / `@implementation Foo (Name)` -- a named
    /// category, carrying its name for the diagnostics that spell it back.
    Category(String),
}

impl InterfaceKind {
    pub fn is_primary(&self) -> bool {
        matches!(self, InterfaceKind::Primary)
    }

    pub fn is_extension(&self) -> bool {
        matches!(self, InterfaceKind::Extension)
    }

    pub fn is_category(&self) -> bool {
        matches!(self, InterfaceKind::Category(_))
    }

    /// The name of a named category, for a diagnostic that has to spell
    /// `Foo(Name)` back to the reader. `None` for both unnamed shapes.
    pub fn category_name(&self) -> Option<&str> {
        match self {
            InterfaceKind::Category(name) => Some(name.as_str()),
            _ => None,
        }
    }

    /// Whether this block is what brings the class into existence.
    ///
    /// Only the primary declaration does. This is why pass 1 records a bare
    /// `@interface Foo ()` as a *use* of `Foo` rather than a declaration of
    /// it: before #529 an extension on an undeclared class silently
    /// fabricated the class, which is the same hole `category_sites` was
    /// added to close for a named category (#501).
    pub fn declares_class(&self) -> bool {
        self.is_primary()
    }

    /// Whether ivars named in this block -- declared directly in an
    /// `instance_variables` list, or implied as a `@property`'s backing
    /// storage -- belong to the class.
    ///
    /// True for the primary declaration and for a class extension, false for
    /// a category. **This is the predicate to reach for**, and the reason it
    /// exists as a named method rather than an inline `matches!`: the obvious
    /// spellings are both wrong. Testing for parentheses strips storage from
    /// an extension, which does own its ivars; testing `category.is_some()`
    /// happens to be right only because an extension used to come back as
    /// `None`, so it would have gone quietly wrong the moment this enum
    /// replaced that `Option`.
    pub fn may_declare_ivars(&self) -> bool {
        !self.is_category()
    }
}

/// Where a `@property` was declared, which is what decides whether it owns
/// storage.
///
/// Carried per-property rather than derived at emit time from whichever
/// `@implementation` block is being rendered, because the two do not line up.
/// A category's properties **merge into the extended class's `ClassInfo`**, so
/// by the time the primary `@implementation` is rendered its `properties` list
/// holds the category's alongside its own with nothing left to tell them
/// apart -- which is how a category property came to add a backing ivar to
/// the extended class's struct and have its getter synthesized there, on top
/// of the real one the category's own translation unit defines (#530).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PropertyOrigin {
    /// The class's own `@interface`, a class extension, or an adopted
    /// protocol -- all of which the class itself provides storage for, so the
    /// property gets a backing ivar and, absent a hand-written one, a
    /// synthesized accessor body.
    #[default]
    Class,
    /// A category. A category cannot add an ivar to the class it extends, so
    /// the property declares its accessors and nothing else: no field in the
    /// struct, and no synthesized body -- the category's own
    /// `@implementation` is the definition.
    Category,
}

impl PropertyOrigin {
    /// Whether the declaring block can give this property a backing ivar in
    /// the class's struct, and therefore whether an accessor body can be
    /// synthesized against one.
    pub fn may_add_storage(self) -> bool {
        matches!(self, PropertyOrigin::Class)
    }
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
    /// Byte offset of the `@property` declaration itself in the merged
    /// buffer, for diagnostics raised during property resolution (after
    /// parsing has moved past the original `Node`).
    ///
    /// An offset rather than the `(line, col)` pair it used to be, so a
    /// diagnostic built from it can be resolved back to a real file like
    /// every other located one (#456).
    pub decl_offset: usize,
    /// The kind of block this `@property` was declared in, which decides
    /// whether it owns storage -- see `PropertyOrigin`. Set by
    /// `collect::collect`'s pass 2 where the property is pushed onto its
    /// class, which is the only place the enclosing `InterfaceKind` is in
    /// hand; `extract_property` sees the declaration alone and cannot tell.
    pub origin: PropertyOrigin,
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
    /// Array extents, by ivar name: `"_values"` -> `"[4]"`.
    ///
    /// Tracked separately from `own_ivars` for the same reason
    /// `unretained_ivars` is: an ivar is a `(name, c_type)` pair, and C
    /// spells an array's extent *after* the name, so there is nowhere in
    /// the type to put it. An ivar declared in an `@interface` keeps its
    /// extent because `emit::lower_ivar_decl` copies the declaration
    /// through verbatim; one declared in an `@implementation` block is
    /// rebuilt from this pair, and without the extent recorded here the
    /// struct field silently became a scalar while every use of it kept
    /// its subscript (#287).
    ///
    /// The text is stored, not a count, and deliberately: `_values[SLOTS]`
    /// is as valid as `_values[4]`, and nothing here can evaluate the
    /// former. Every consumer either copies it into a declaration or
    /// derives the count with `sizeof`.
    pub array_extents: HashMap<String, String>,
    /// `(selector, is_class_method)` for every method an `@implementation`
    /// in the parsed source really defines -- as opposed to `methods`, which
    /// also holds everything only *declared* in an `@interface`.
    ///
    /// oz2c is a whole-program transpiler: it emits a definition for a
    /// method exactly when it parsed one, or when it synthesizes one (a
    /// property accessor). So this is the authority on what the generated C
    /// will actually contain, and referencing anything else produces a link
    /// error rather than a compile error -- the mistake surfacing at the
    /// wrong end of the pipeline. See `Program::method_is_defined`.
    pub defined_selectors: HashSet<(String, bool)>,
    /// Does a primary `@implementation` for this class appear in this
    /// source? Categories and class extensions do not count -- only the
    /// block that claims to define the class's own methods.
    ///
    /// This is the difference between a selector that is *missing* and one
    /// that is merely *elsewhere*, and `emit::reject_undefined_target`
    /// turns on it (#566). With the primary implementation here, a
    /// declaration with no body is an omission oz2c can name; without it,
    /// the class is implemented in another translation unit or by
    /// hand-written C providing `Foo_bar()`, which the whole-program model
    /// cannot see and must not refuse. `method_is_defined`'s own doc sets
    /// that boundary out.
    pub has_primary_implementation: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProtocolInfo {
    pub name: String,
    /// Protocols this one extends (`@protocol Name <Super, ...>`).
    pub super_protocols: Vec<String>,
    /// Methods declared directly by this protocol -- not resolved through
    /// `super_protocols`; use `Program::protocol_methods` for that.
    pub methods: Vec<MethodSig>,
    /// Properties declared directly by this protocol, same non-transitive
    /// rule as `methods`.
    ///
    /// Collected since #498. Before that a protocol `@property` was
    /// recorded **nowhere** -- `collect_protocol_methods` matched only
    /// `method_declaration` -- so `@synthesize` of one was refused with a
    /// message saying no such property was declared, which was wrong about
    /// the cause: it was declared, in a protocol nobody looked in.
    ///
    /// Note what this is *not* wired into: protocol **conformance**. A
    /// class that adopts a protocol and provides neither accessor nor
    /// `@synthesize` is still accepted, where an unmet protocol *method*
    /// is refused. Making that an error would newly reject programs that
    /// build today, so it is a separate decision rather than a
    /// consequence of collecting the data (#498).
    pub properties: Vec<PropertyInfo>,
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
    /// Every top-level C function's declared return type, by name, in the
    /// C spelling the output uses (`Thing *` -> `struct Thing *`).
    ///
    /// Collected because a *call result* is a legitimate message receiver
    /// and had no type at all: `render_expr` had no `call_expression` arm,
    /// so `[makeThing() poke]` was refused while
    /// `Thing *t = makeThing(); [t poke];` compiled (#355). See
    /// `collect::function_return_types`, which also explains why
    /// prototypes count.
    ///
    /// A fact about the source rather than an option, unlike `heap_support`
    /// and its neighbours below.
    pub function_return_types: HashMap<String, String>,
    /// Which preprocessor conditional arms are part of the program.
    ///
    /// A fact about the source, like `function_return_types` above.
    /// Carried on `Program` because `emit` needs the same verdicts
    /// `collect` used -- re-deriving them there would be a second oracle
    /// able to disagree with the first, and the arm a class was collected
    /// from must be the arm it is emitted from.
    pub preproc: crate::preproc::Liveness,
    /// Ownership facts read from a Clang AST dump, when one was supplied
    /// (`--ast`). Clang resolves types; tree-sitter does not, so this is the
    /// only authority on whether an `id`-typed ivar is an object the class
    /// owns -- see `astinfo` and `owned_object_ivar_names`.
    pub ast: Option<crate::astinfo::AstFacts>,
    /// Whether `--heap-support` was given, enabling `+dynamicAllocWithHeap:` and the
    /// heap-aware free path. Every class then also gets a heap allocator, the
    /// root's `_meta` a `heap_allocated` flag, and the companion the two functions
    /// the PAL declares but leaves to generated code.
    ///
    /// A program-wide option rather than a fact about the source, kept here
    /// for the same reason `ast` is: `Program` is what every emitter already
    /// has in hand, and threading a second parameter through each of them
    /// would say nothing extra.
    pub heap_support: bool,
    /// `--introspection` (`CONFIG_OBJZ_INTROSPECTION`): whether
    /// `-isKindOfClass:` and `-conformsToProtocol:` are available. An
    /// option, not a fact about the source.
    pub introspection: bool,
    /// `--reflection` (`CONFIG_OBJZ_REFLECTION`): whether `@selector`,
    /// `SEL`, `-respondsToSelector:` and the `-performSelector:` family
    /// are available. An option, not a fact about the source.
    pub reflection: bool,
    /// Selectors named by a `@selector(...)` anywhere in the source.
    ///
    /// Unlike the introspection facts -- which `emit` accumulates as it
    /// writes (`emit::IntrospectionUse`) -- this has to be known *before*
    /// the dispatch tables are generated, because it decides which
    /// selectors get an `OZ_PROTOCOL_SEND_*` function at all (see
    /// `is_dynamically_dispatched`). So it is prescanned from the CST in
    /// `collect` instead.
    pub reflected_selectors: std::collections::BTreeSet<String>,
    /// Does anything in the source send a `-performSelector:` variant?
    ///
    /// A `SEL` is a first-class value here, so there is no way to prove
    /// which selector reaches which `-performSelector:` call site. That
    /// makes performability a whole-program property: if the program
    /// performs at all, every reflectively-named selector needs a uniform-
    /// shape wrapper, and one that cannot have it is a located error. If
    /// it never performs, no wrapper is generated and no dispatch function
    /// is forced into existence for a reflected selector.
    pub uses_perform_selector: bool,
    /// Selectors a `-performSelector:` site names with a literal
    /// `@selector(...)` at the site itself.
    pub performed_selectors: std::collections::BTreeSet<String>,
    /// Does any `-performSelector:` site take its selector from a value
    /// rather than a literal -- a local, an ivar, a parameter, a cast?
    ///
    /// When nothing does, exactly `performed_selectors` can reach a
    /// perform, so only those need a uniform-shape wrapper and only those
    /// have to be performable. One site taking a value makes it
    /// undecidable, and the requirement widens to every reflectively-named
    /// selector.
    pub performs_via_value: bool,
    /// Does anything send `-respondsToSelector:`? Gates `oz_responds` and
    /// the per-selector `responds` bitmaps.
    pub uses_responds_to_selector: bool,
    /// Does the program use `@synchronized` anywhere? When it does, the root
    /// struct gains an `oz_sync_lock` so `@synchronized(obj)` can lock
    /// storage owned by `obj` rather than a fresh lock on the caller's own
    /// stack, which serialized nothing across cores.
    ///
    /// A fact about the source rather than an option, unlike `heap_support`,
    /// but kept here for the same reason: the root-struct emitter already has
    /// `Program` in hand and nothing else would carry it.
    pub uses_synchronized: bool,
    /// Every class name a `@class` forward declaration names.
    ///
    /// The raw fact, deliberately -- *not* "only forward-declared". A
    /// name here may also have a real `@interface` in the same
    /// translation unit, in which case it is an ordinary class and the
    /// forward declaration adds nothing. Whether a name is only
    /// forward-declared is `is_forward_declared_only`, which is the
    /// question a caller actually has.
    ///
    /// Collected so a send through such a name can name *that* as the
    /// cause. Without it the receiver's type had already degraded by the
    /// time the send was refused, and the error reported the fallback --
    /// "receiver type is 'id'" -- for a receiver the author had spelled
    /// with a class name (#557).
    pub forward_declared: std::collections::BTreeSet<String>,
    /// Did the source parse without an `ERROR` or a missing token?
    ///
    /// Computed once in `collect` and carried, rather than re-derived by
    /// each pass that needs it -- the walk is over the whole tree and the
    /// answer cannot change.
    ///
    /// **What it is for: an *absence* claim is only sound over a tree that
    /// parsed.** Two checks assert one, and both were wrong on malformed
    /// input until #567's regression was swept out of the mutation corpus:
    ///
    /// - "no `@interface` for this name exists in this source" (#567). A
    ///   nameless `@interface : OZObject` puts an `ERROR` on the stray `:`
    ///   and leaves `OZObject` as the first `identifier`, so
    ///   `class_header` reads the superclass as the class name and the
    ///   real class is declared nowhere.
    /// - "this selector is declared and defined nowhere" (#566). An
    ///   unclosed bracket stops a method body parsing, so a defined method
    ///   looks undefined.
    ///
    /// In both cases the conclusion is true of the parse, derived from the
    /// syntax error, and points at a construct that is not the mistake. A
    /// file that failed to parse loses no coverage by being skipped: the
    /// AST requirement refuses it, or Clang does at dump time, and either
    /// names the cause. `oz2c-challenges` grades all four such fixtures
    /// (M04, M05, M09, M21) `CLANG` for exactly that reason.
    ///
    /// A *presence* claim needs no such guard, which is why this is not a
    /// blanket "stop checking": a construct that is really there is really
    /// there whatever else failed to parse.
    pub source_parsed: bool,
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

    /// Is `name` declared *only* by an `@class` forward declaration --
    /// the name exists, but no `@interface` ever gave it a shape?
    ///
    /// `forward_declared` is the raw set and says nothing about whether a
    /// real declaration followed, so every caller wants this and not the
    /// field. Both halves matter: a name with an `@interface` is an
    /// ordinary class, and a name in neither was never declared at all --
    /// which is #501's diagnostic, not this one.
    pub fn is_forward_declared_only(&self, name: &str) -> bool {
        self.forward_declared.contains(name) && !self.is_class(name)
    }

    /// Is `name` a type oz2c spells with a `struct` tag?
    ///
    /// True for a real class and for one only forward-declared (#564) --
    /// and the distinction between those two is exactly why this is
    /// separate from `is_class` rather than folded into it. `is_class` has
    /// fourteen callers in `emit` alone, and most of them are asking
    /// something a forward declaration cannot answer: does this class have
    /// a slab, an allocator, ivars, a dispatch slot, a place in the
    /// `class_order`? A forward-declared name has none of that, so
    /// widening `is_class` would hand `pools`, `companion` and `arc` a
    /// class with no shape.
    ///
    /// What a forward declaration *does* settle is the spelling: C reaches
    /// an undefined type through a tag, `struct A;` is legal with nothing
    /// ever defining `struct A`, and that is the whole meaning of `@class`.
    /// So this is the predicate for the spelling sites only -- the
    /// `type_identifier` arm of `render_expr` and the bare-ivar lowering --
    /// and nothing else should reach for it.
    pub fn spells_with_struct_tag(&self, name: &str) -> bool {
        self.is_class(name) || self.is_forward_declared_only(name)
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
    /// Each owned object ivar as `(access path, array extent)`. The extent
    /// is `None` for the ordinary single-object case, and `Some("[2]")` for
    /// an array of them -- which decides whether the release is one call or
    /// a loop over the elements (#287).
    pub fn owned_object_ivars(&self, class_name: &str) -> Vec<(String, Option<String>)> {
        self.owned_object_ivar_names(class_name)
            .into_iter()
            .filter_map(|ivar| {
                let path = self.ivar_access_path(class_name, &ivar)?;
                Some((path, self.array_extent_of(class_name, &ivar)))
            })
            .collect()
    }

    /// The array extent recorded for `ivar_name`, searching the class's own
    /// declarations and then its ancestors' -- the same walk
    /// `ivar_access_path` does, and for the same reason.
    pub fn array_extent_of(&self, from_class: &str, ivar_name: &str) -> Option<String> {
        let mut cur = Some(from_class.to_string());
        while let Some(name) = cur {
            let info = self.classes.get(&name)?;
            if let Some(extent) = info.array_extents.get(ivar_name) {
                return Some(extent.clone());
            }
            cur = info.superclass.clone();
        }
        None
    }

    /// Does `class_name` or any ancestor define `selector` itself?
    ///
    /// The same walk as `array_extent_of`, for protocol conformance: an
    /// inherited implementation satisfies a protocol requirement, in
    /// Objective-C and here. `render_interface`'s conformance check read
    /// only the class's own `methods` until #307, which made the base
    /// protocol every other protocol should adopt impossible to adopt --
    /// `OZArray <OZIteratorProtocol>` was told it "doesn't implement
    /// 'isEqual:'" for nine methods it inherits from `OZObject` and never
    /// needed to restate.
    pub fn implements_selector(
        &self,
        class_name: &str,
        selector: &str,
        is_class_method: bool,
    ) -> bool {
        let mut cur = Some(class_name.to_string());
        while let Some(name) = cur {
            let Some(info) = self.classes.get(&name) else {
                return false;
            };
            if info
                .methods
                .iter()
                .any(|m| m.selector == selector && m.is_class_method == is_class_method)
            {
                return true;
            }
            cur = info.superclass.clone();
        }
        false
    }

    /// `owned_object_ivars` as plain ivar names rather than access paths.
    ///
    /// It existed for `staticbar::check_dealloc_body`, which recognised an
    /// owned ivar being released by hand; that check went when a
    /// `-release` send became a located error wherever it appears (#428).
    /// The live reader is `companion::render_release_ivars`, through
    /// `owned_object_ivars`.
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
                if self.fallback_owns_object_ivar(&name, ivar, c_type) {
                    out.push(ivar.clone());
                }
            }
        }
        out
    }

    /// The syntactic rule used when Clang has no answer for an ivar --
    /// extracted so it can be *asked* rather than only taken.
    ///
    /// `oz2c --check-arc` diffs this against Clang's verdict for every
    /// ivar, which is the one place the two models answer the same question
    /// independently. Keeping it inline would have meant the audit
    /// comparing against a second copy of the rule, and a copy that drifts
    /// reports agreement it has not checked.
    ///
    /// What it cannot see is the whole reason `--ast` is required: it reads
    /// the *spelling* of a lowered C type, so an `id`-typed ivar -- which
    /// lowers to no `struct` at all -- is skipped, and skipping it leaks the
    /// object. On px-keyboard that was 4 of 46 generated files silently
    /// different (#299).
    pub fn fallback_owns_object_ivar(
        &self,
        class_name: &str,
        ivar: &str,
        c_type: &str,
    ) -> bool {
        let Some(info) = self.classes.get(class_name) else {
            return false;
        };
        if info.unretained_ivars.contains(ivar) {
            return false;
        }
        if !c_type.trim_start().starts_with("struct ") || !c_type.contains('*') {
            return false;
        }
        let Some(target) = c_type.trim().strip_prefix("struct ") else {
            return false;
        };
        self.is_class(target.trim_end_matches('*').trim())
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
    /// `-isEqual:`/`-getDescription:maxLength:`), or when more than one
    /// class in the program implements it. Class methods never qualify --
    /// a class-method receiver is always statically known.
    ///
    /// It is no longer always a *literal class name*, which is how this
    /// used to be argued. Since #534, `self` and `super` in a `+` method
    /// resolve to the `class:C` receiver form too (`emit::render_expr`).
    /// The conclusion is unchanged, and for a stronger reason than the
    /// old wording gave: both resolve to a class fixed at *transpile*
    /// time -- the one whose `@implementation` encloses the send -- and a
    /// generated class method takes no receiver parameter, so there is
    /// nothing a dynamic dispatch could switch on even in principle.
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
    /// oz2c needs neither: `dealloc` has its own const-vtable
    /// mechanism, and an overridden `init` is caught by the same
    /// hierarchy analysis as any other selector.
    pub fn is_dynamically_dispatched(&self, selector: &str, is_class_method: bool) -> bool {
        const ALWAYS_DYNAMIC: &[&str] = &["isEqual:", "getDescription:maxLength:"];
        if is_class_method {
            return false;
        }
        if self.is_protocol_selector(selector, false) {
            return true;
        }
        if ALWAYS_DYNAMIC.contains(&selector) {
            return true;
        }
        // A selector named by a `@selector(...)` in a program that also
        // performs needs a dispatch function even with a single
        // implementor: `oz_perform` calls through the uniform-shape
        // wrapper, and the wrapper calls this. Without it a
        // `[obj performSelector:@selector(poke)]` against the only class
        // implementing `-poke` would reference a function that was never
        // generated.
        if self.needs_perform_wrapper(selector) {
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
    /// defined -- oz2c emits those itself
    /// (`emit::render_synthesized_accessor`).
    /// Will the generated C contain a definition of `class_name`'s
    /// `selector`? Referencing one that it will not is a link error, so this
    /// gates the protocol-dispatch table (`companion::render_protocol_dispatch`).
    ///
    /// The parse is the authority, not the Clang AST. oz2c emits a
    /// definition exactly when it parsed an `@implementation` defining the
    /// method, or when it synthesizes the accessor for a `@property`, so it
    /// already knows the answer without asking anyone. The AST is consulted
    /// only as an additional *positive* source, never to overrule a body
    /// that was parsed -- so supplying one can never suppress more than not
    /// supplying one.
    ///
    /// The case this exists for: `include/oz_sdk/Foundation/OZArray.h` and
    /// `OZDictionary.h` both declare `countByEnumeratingWithState:objects:
    /// count:`, which no `.m` in the repository implements. Routing to it
    /// emitted a dispatch function calling
    /// `OZArray_countByEnumeratingWithState_objects_count_`, and every
    /// sample that pulled in Foundation failed to link on the undefined
    /// symbol. The Python pipeline never mentions that selector at all,
    /// because it collects methods from implementations rather than
    /// declarations.
    ///
    /// A method implemented outside the transpiled program -- a hand-written
    /// C function providing `Foo_bar()` -- is therefore reported undefined.
    /// That is consistent with the whole-program model, and a Clang AST
    /// covering the file is the way to assert otherwise.
    pub fn method_is_defined(&self, class_name: &str, selector: &str, is_class_method: bool) -> bool {
        let Some(info) = self.classes.get(class_name) else {
            // Not a class this program describes at all; nothing to claim.
            return true;
        };
        if info.defined_selectors.contains(&(selector.to_string(), is_class_method)) {
            return true;
        }
        // A `@property`'s accessor has no body in source because oz2c
        // writes it (`emit::render_synthesized_accessor`).
        if info.properties.iter().any(|prop| {
            prop.getter_sel.as_deref() == Some(selector)
                || prop.setter_sel.as_deref() == Some(selector)
                || prop.name == selector
                || crate::collect::default_setter_sel(&prop.name) == selector
        }) {
            return true;
        }
        self.ast.as_ref().is_some_and(|facts| facts.has_method_body(class_name, selector))
    }

    /// The `@property` named `prop_name` visible on `class_name`, and the
    /// class that declares it -- searching up the superclass chain, since a
    /// property is inherited like a method.
    ///
    /// Needed to translate dot syntax (`obj.prop`): the field name in the
    /// source is the *property* name, but the call to emit is its accessor
    /// selector, which `getter=`/`setter=` can rename to anything.
    pub fn find_property(
        &self,
        class_name: &str,
        prop_name: &str,
    ) -> Option<(String, &PropertyInfo)> {
        let mut current = Some(class_name.to_string());
        while let Some(name) = current {
            let info = self.classes.get(&name)?;
            if let Some(prop) = info.properties.iter().find(|p| p.name == prop_name) {
                return Some((name, prop));
            }
            current = info.superclass.clone();
        }
        None
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

    /// Does `selector` need a uniform-shape `perform` wrapper, and so a
    /// dispatch function to call through?
    ///
    /// Exactly the selectors that can reach a `-performSelector:`. When
    /// every perform site names its selector with a literal, that is the
    /// set of those literals; one site taking a `SEL` from a value makes
    /// it undecidable and widens the answer to every reflectively-named
    /// selector.
    ///
    /// Narrowing this matters for more than code size. A selector needing
    /// a wrapper has to *fit* one -- at most two object-typed arguments,
    /// returning void or an object -- so treating every `@selector(...)`
    /// as performable-or-error forced signature changes on methods
    /// nothing ever performed. `samples/reflection_demo` was rejected by
    /// exactly that: its `-toggle` returns `int` for its protocol's sake
    /// and is only ever asked about with `-respondsToSelector:`.
    pub fn needs_perform_wrapper(&self, selector: &str) -> bool {
        if !self.uses_perform_selector {
            return false;
        }
        if self.performs_via_value {
            return self.reflected_selectors.contains(selector);
        }
        self.performed_selectors.contains(selector)
    }

    /// Does any strict subclass of `class_name` implement `selector`?
    ///
    /// This is the class-hierarchy-analysis test that decides whether a
    /// message send against a receiver *declared* as `class_name` can be
    /// devirtualized into a direct call. A declared type is only an upper
    /// bound on the receiver's real class -- `Base *b = (Base *)[Sub
    /// alloc];` is still a `Sub` -- so a direct call to the declared
    /// type's own implementation is sound only when no subclass could
    /// have overridden it. oz2c sees the whole program as one
    /// translation unit, so this analysis is exact rather than
    /// conservative.
    /// Every class whose implementation of `selector` a send can actually
    /// reach at run time, given the receiver's *static* type.
    ///
    /// The set the ownership of a dynamically dispatched send has to be
    /// decided from. Deciding it from the static type's own implementation
    /// alone is #365: `Base` classified as an owning factory, `Derived`
    /// overriding the selector to hand back a reference it keeps, and a
    /// caller holding a `Base *` releasing what it does not own -- a
    /// use-after-free rather than a leak.
    ///
    /// `None` means the receiver pins nothing down (a bare `id`, or a
    /// protocol-qualified one), so every implementor is reachable -- which
    /// is #361, the same question with a wider set.
    ///
    /// Answers *defining* classes, deduplicated, because that is what the
    /// dispatch calls and what `OwningMethods` is keyed on. Empty when the
    /// selector resolves nowhere.
    ///
    /// `companion::render_protocol_dispatch` builds the same set for its
    /// `routed` list and both are filtered by `method_is_defined` for the
    /// same reason: a selector declared and never defined is not a
    /// callable function, so it can be neither routed to nor polled for
    /// ownership.
    pub fn reachable_implementors(
        &self,
        receiver: Option<&str>,
        selector: &str,
        is_class_method: bool,
    ) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |defining: String| {
            if !out.contains(&defining) {
                out.push(defining);
            }
        };
        let candidates: Vec<&String> = match receiver {
            /* The receiver's own class, plus every subclass of it -- a
             * superclass's override is unreachable from here, since the
             * receiver is at least a `receiver`. */
            Some(class) => self
                .class_order
                .iter()
                .filter(|name| name.as_str() == class || self.is_descendant_of(name, class))
                .collect(),
            None => self.class_order.iter().collect(),
        };
        for name in candidates {
            let Some(defining) = self.find_defining_method_name(name, selector, is_class_method)
            else {
                continue;
            };
            if !self.method_is_defined(&defining, selector, is_class_method) {
                continue;
            }
            push(defining);
        }
        out
    }

    /// The class in `start`'s chain that declares `selector`, if any.
    ///
    /// The same single-inheritance walk `companion::find_defining_method`
    /// does; here so `reachable_implementors` can answer without reaching
    /// into that module.
    pub fn find_defining_method_name(
        &self,
        start: &str,
        selector: &str,
        is_class_method: bool,
    ) -> Option<String> {
        let mut cur = Some(start.to_string());
        while let Some(name) = cur {
            let info = self.classes.get(&name)?;
            if info
                .methods
                .iter()
                .any(|m| m.is_class_method == is_class_method && m.selector == selector)
            {
                return Some(name);
            }
            cur = info.superclass.clone();
        }
        None
    }

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
    /// 1-based line, and `col` the 1-based *byte* column.
    ///
    /// **Which buffer they index depends on whether `resolve_in` has
    /// run.** Until it does, they are positions in the merged,
    /// `#import`-spliced buffer that every pass walks -- and once
    /// anything has been spliced in, that is a line in no file on disk.
    /// A 9-line `.m` reported its defect at line 1989 (#456). After
    /// `resolve_in`, they are positions in `file`.
    pub line: usize,
    pub col: usize,
    /// Byte offset of the diagnostic in the merged buffer, kept so the
    /// position can be resolved back to the file it was written in.
    ///
    /// Byte offsets are the one coordinate that survives everything:
    /// `parse::repair_bare_macro_statements` overwrites a whitespace
    /// byte in place, so it preserves every offset while it can *eat a
    /// line* -- which is why this is an offset and not a line.
    ///
    /// `None` for the three whole-program checks with no node to point
    /// at: `attach_ast`, an unknown `--pool-sizes` class, and an
    /// unsizable slab cycle. Those keep the `(1, 1)` they always had;
    /// giving them a real anchor needs one threaded from upstream and is
    /// tracked separately.
    ///
    /// A *range*, not a start, so the renderer can underline the whole
    /// offending construct rather than put a single caret under its first
    /// byte. One field rather than a start plus an end, because two
    /// fields can disagree and this one cannot (#457).
    pub span: Option<std::ops::Range<usize>>,
    /// What is wrong beyond the one-line message: the reason, where the
    /// reason is neither the diagnosis nor the remedy.
    ///
    /// Rendered as rustc's `note:`. `None` when the message says all
    /// there is.
    pub note: Option<String>,
    /// What the author can do about it, one entry per distinct remedy.
    ///
    /// A `Vec`, not an `Option`, because a single diagnostic can carry
    /// several genuinely different fixes: the `-retain` rejection offers
    /// three (let ARC manage it, opt the slot out with
    /// `__unsafe_unretained`, or read the count with
    /// `oz_retain_count`), and they were one 90-word sentence
    /// before this (#457).
    pub help: Vec<String>,
    /// The source file `line`/`col` refer to, once `resolve_in` has run.
    ///
    /// `None` means they are still merged-buffer positions: either
    /// nothing resolved them (the pure `transpile()` form is handed a
    /// string with no files behind it) or the offset fell outside the
    /// map.
    pub file: Option<std::path::PathBuf>,
}

impl Diagnostic {
    /// A diagnostic at a merged-buffer position with no offset to resolve
    /// it by -- for a whole-program check that has no `Node` to blame.
    ///
    /// Prefer `at`. This spelling cannot be resolved to a file, so it is
    /// the one that still reports a position no reader can open.
    pub fn new(message: impl Into<String>, line: usize, col: usize) -> Self {
        Diagnostic {
            message: message.into(),
            line,
            col,
            span: None,
            note: None,
            help: Vec::new(),
            file: None,
        }
    }

    /// A diagnostic pointing a caret at `offset` in `src`, the merged
    /// buffer.
    ///
    /// For a site with a position but no end to offer. Delegates to
    /// `spanning`, which is the one place a located diagnostic is built
    /// -- so the two cannot disagree about how a position is recorded.
    ///
    /// `line`/`col` are derived here so no caller computes them, and the
    /// offset is kept so `resolve_in` can map the position back to the
    /// file the code was written in. A site that computes `line_col`
    /// itself and calls `new` instead produces a diagnostic that cannot
    /// be resolved -- the defect #456 fixed, so do not reintroduce it.
    pub fn at(message: impl Into<String>, src: &str, offset: usize) -> Self {
        Diagnostic::spanning(message, src, offset..offset)
    }

    /// `at`, underlining `span` rather than pointing a caret at its start.
    ///
    /// The span is a merged-buffer byte range, so it survives the repair
    /// for the same reason a single offset does. An empty range renders
    /// as a one-column caret, which is what a site with no end to offer
    /// gets.
    pub fn spanning(
        message: impl Into<String>,
        src: &str,
        span: std::ops::Range<usize>,
    ) -> Self {
        let (line, col) = crate::parse::line_col(src, span.start);
        Diagnostic {
            message: message.into(),
            line,
            col,
            span: Some(span),
            note: None,
            help: Vec::new(),
            file: None,
        }
    }

    /// Attach the reason, which is neither diagnosis nor remedy.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Attach one remedy. Called more than once for a diagnostic with
    /// more than one, which is why `help` is a `Vec`.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help.push(help.into());
        self
    }

    /// `at` when the anchor was found, and an unlocatable `(1, 1)` when it
    /// was not.
    ///
    /// One spelling for "located if we can find it" rather than an
    /// `unwrap_or((1, 1))` at the call site, so the fallback is visible
    /// here and a caller cannot accidentally produce the unresolvable
    /// shape while believing it passed an offset.
    pub fn maybe_at(message: impl Into<String>, src: &str, offset: Option<usize>) -> Self {
        match offset {
            Some(offset) => Diagnostic::at(message, src, offset),
            None => Diagnostic::new(message, 1, 1),
        }
    }

    /// Rewrite `line`/`col` as a position in the file the offset was
    /// spliced from, and record that file.
    ///
    /// A no-op when the diagnostic carries no offset, or when the offset
    /// is outside the map: in both cases the merged position it already
    /// holds is the most that can honestly be said, so it is left alone
    /// rather than replaced with a guess.
    pub fn resolve_in(&mut self, map: &crate::imports::SourceMap) {
        let Some(offset) = self.span.as_ref().map(|s| s.start) else { return };
        let Some((file, line, col)) = map.source_position(offset) else { return };
        self.file = Some(file.to_path_buf());
        self.line = line;
        self.col = col;
    }
}

/// The whole diagnostic as plain text, one tier per line.
///
/// This is not the rendered form -- `render::render` draws the snippet and
/// the caret for a terminal. This is the unrendered text, and it carries
/// `note`/`help` because they are *part of the diagnostic*: a remedy moved
/// out of `message` into `help` must still be visible to anything asking
/// whether the remedy was offered, and ~400 assertions in the suite ask
/// exactly that (#457).
///
/// The first line stays self-contained, so a caller that reads one line
/// still gets a complete summary.
impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.file {
            Some(path) => {
                write!(f, "{}:{}:{}: {}", path.display(), self.line, self.col, self.message)?
            }
            None => write!(f, "{}:{}: {}", self.line, self.col, self.message)?,
        }
        if let Some(note) = &self.note {
            write!(f, "\n  note: {}", note)?;
        }
        for help in &self.help {
            write!(f, "\n  help: {}", help)?;
        }
        Ok(())
    }
}
