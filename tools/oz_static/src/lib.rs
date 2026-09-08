// SPDX-License-Identifier: Apache-2.0
//
// lib.rs - OZ-091 Track B spike: static-subset Objective-C to C
// transpiler using in-place textual substitution.

pub mod arc;
pub mod astinfo;
pub mod collect;
pub mod companion;
pub mod emit;
pub mod generics;
pub mod imports;
pub mod model;
pub mod parse;
pub mod pools;
pub mod progress;
pub mod staticbar;

pub use model::{Diagnostic, Program};

pub struct TranspileOutput {
    pub source_c: String,
    pub companion_h: String,
    pub companion_c: String,
}

/// Per-class slab sizes to use instead of the ones counted from the
/// source, as `--pool-sizes Class=N,...` supplies (see `pools`). Empty for
/// every caller that doesn't override anything.
pub type PoolOverrides = std::collections::HashMap<String, usize>;

/// Everything a caller can supply beyond the source text itself.
///
/// Grouped rather than passed as a widening list of arguments, because both
/// entry points take the same set and the pure `transpile(source)` form has
/// to keep working untouched -- it is what the whole test suite calls.
#[derive(Default)]
pub struct Options {
    pub pool_sizes: PoolOverrides,
    /// `clang -Xclang -ast-dump=json` dumps covering this source, which are
    /// the only authority on which ivars are objects the class owns (see
    /// `astinfo`). Produce them with `-fobjc-arc`, or they carry no
    /// ownership information and oz_static falls back to its own narrower
    /// rule.
    ///
    /// A list, not one dump: a program spread over several `.m` files needs
    /// one per file, since a single dump only shows the `@implementation`s
    /// written in that file. The facts are unioned -- see
    /// `astinfo::AstFacts::merge`.
    pub ast_json: Vec<String>,
    /// The same dumps, as paths to read lazily -- one at a time, parsed and
    /// dropped before the next is opened.
    ///
    /// This is what the CLI passes. `ast_json` held every dump in memory
    /// simultaneously, which on px-keyboard meant 742 MB of text plus the
    /// tree built over it: 1.30 GB resident to answer questions about two
    /// ivars (#299).
    ///
    /// Both fields are honoured and their facts unioned. `ast_json` stays
    /// because the hand-written fixtures in `tests/ast_facts.rs` and
    /// `astinfo`'s own tests are strings, not files, and requiring them to
    /// be written to disk would make them worse tests.
    ///
    /// Ordering, which the progress index depends on: `ast_json` entries
    /// are reported first, then `ast_paths`. The CLI supplies only paths,
    /// so its indices start at 0 and line up with its own `--ast` list.
    pub ast_paths: Vec<std::path::PathBuf>,
    /// Enable `+allocWithHeap:` and the heap-aware free path -- the oracle's
    /// `--heap-support`. Off by default: the field it adds to every object
    /// and the branch it adds to every free are only worth paying for if
    /// something actually allocates from a heap.
    pub heap_support: bool,
    /// Enable `-isKindOfClass:` and `-conformsToProtocol:`, which
    /// `CONFIG_OBJZ_INTROSPECTION` passes as `--introspection`.
    ///
    /// Off by default, exactly like `heap_support`: the flag's absence is
    /// what the Kconfig option's `n` means, and the Kconfig default (`y`)
    /// is what supplies it. With it off the two selectors stay hard
    /// located errors naming the option, so a build never quietly loses
    /// them.
    ///
    /// `+class`, `-class` and `-isMemberOfClass:` are deliberately *not*
    /// gated here -- a class identity is a constant or a bitfield read
    /// and generates no table, so there is nothing to switch off.
    pub introspection: bool,
    /// Enable `@selector`, `SEL`, `-respondsToSelector:` and the
    /// `-performSelector:` family, which `CONFIG_OBJZ_REFLECTION` passes
    /// as `--reflection`.
    ///
    /// Off by default like the others, so the flag's absence is what the
    /// Kconfig option's `n` means. With it off every one of those
    /// constructs stays a hard located error naming the option.
    pub reflection: bool,
    /// Slots for the shared collection element pool instead of the count
    /// taken from the source, as `--item-pool-size N` supplies. `None`
    /// leaves the counted size (or an `oz-item-pool:` directive) in force.
    ///
    /// A single number rather than a per-class map, because both OZArray
    /// and OZDictionary draw from one pool -- see
    /// `pools::PoolSizes::item_slots`.
    pub item_pool_size: Option<usize>,
    /// The merged-offset -> (`.m`/`.h`, line) map from
    /// `imports::ResolvedSource::source_map`, when the caller wants `#line`
    /// directives in the generated C (#305).
    ///
    /// **Its absence is the off switch**, and the only one: `None` -- the
    /// default, and what `oz2c` passes without `--line-directives`
    /// (`CONFIG_OBJZ_DEBUG_LINES=n`) -- emits the same bytes as before the
    /// feature existed. `transpile()` is `None` by construction: its source
    /// is a string with no file behind it, so there is nothing a directive
    /// could truthfully name.
    ///
    /// Owned (a clone of the resolution's map) rather than borrowed,
    /// because a `&SourceMap` here would put a lifetime parameter on
    /// `Options` -- which derives `Default` and which nearly every test
    /// constructs literally. It is a few Vecs: ~30 KB for px-keyboard's
    /// 106 KB buffer, cloned once per run, against a pipeline that reads
    /// hundreds of megabytes of Clang AST.
    pub source_map: Option<imports::SourceMap>,
    /// Directory each origin stem's generated `.h`/`.c` pair will be
    /// written to, so a `#line` handing attribution *back* to the generated
    /// file can name it absolutely. Only `main.rs` knows this (it decides
    /// the `Foundation/` split), and only it fills this in; a caller that
    /// writes nothing to disk leaves it empty and those directives name a
    /// bare `<stem>.c`.
    pub generated_dirs: std::collections::HashMap<String, std::path::PathBuf>,
    /// Byte ranges of the source that came from a header rather than an
    /// implementation file -- `imports::ResolvedSource::header_ranges`.
    /// Pass-through C from a header goes into the generated header, so every
    /// file that includes it sees it. Empty for the pure `transpile()` form,
    /// which has one output file and no such distinction to make.
    pub header_ranges: Vec<std::ops::Range<usize>>,
}

/// Full pipeline: parse -> collect -> emit. Returns Ok on success, or the
/// full list of static-bar/emission diagnostics on failure. Never
/// silently degrades: anything the static subset doesn't accept is a
/// named, located hard error.
pub fn transpile(source: &str) -> Result<TranspileOutput, Vec<Diagnostic>> {
    transpile_with_options(source, &Options::default())
}

/// `transpile` with explicit slab sizes for named classes.
pub fn transpile_with_pool_sizes(
    source: &str,
    overrides: &PoolOverrides,
) -> Result<TranspileOutput, Vec<Diagnostic>> {
    transpile_with_options(
        source,
        &Options { pool_sizes: overrides.clone(), ..Default::default() },
    )
}

/// Everything the passes before `emit` produce, which both entry points
/// need and neither one shapes differently.
///
/// `repaired` is in here because the whole pipeline reads the *repaired*
/// text by byte offset -- so it has to outlive the front end, and the
/// caller's `source` is the wrong string to hand to `emit`.
struct FrontEnd {
    repaired: String,
    repaired_semicolons: Vec<usize>,
    program: Program,
    pools: pools::PoolSizes,
}

/// Every pass up to and including pool sizing -- the half of the pipeline
/// `transpile_with_options` and `transpile_split_with_options` share.
///
/// They used to hold two copies of this sequence, identical line for line
/// from the repair through `resolve_pools` and differing only in which
/// `emit` they call. Two copies of a seven-pass ordering is an invitation
/// to fix a bug in one of them: the passes are order-dependent (the repair
/// must precede anything that reads a byte offset, `attach_ast` must
/// precede `arc::analyze`, and `generics` must see a fully-populated
/// `Program`), and nothing enforced that both agreed.
///
/// Diagnostics stop the pipeline at the first pass that produces any --
/// oz_static has no soft-diagnostic mode, so a returned `Err` is always
/// final and later passes would only report consequences of the first
/// failure.
fn front_end(
    source: &str,
    options: &Options,
    obs: &mut dyn progress::Observer,
) -> Result<FrontEnd, Vec<Diagnostic>> {
    /* Every later pass -- collect, arc, generics, pools, emit -- reads this
     * text by byte offset, so the repair has to happen before any of them
     * and has to preserve length. See
     * `parse::repair_bare_macro_statements` (#288, #289). */
    obs.enter(progress::Phase::Repair);
    let (repaired, repaired_semicolons) = parse::repair_bare_macro_statements(source);
    let text: &str = &repaired;
    obs.enter(progress::Phase::Collect);
    let (mut program, mut diagnostics) = collect::collect(text);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    /* Entered unconditionally, even with no dumps supplied, so the phase
     * sequence is the same shape whether or not `--ast` was given -- a test
     * can then compare it against one literal list. */
    obs.enter(progress::Phase::AstIngest);
    if let Err(why) = attach_ast(&mut program, options, obs) {
        return Err(vec![Diagnostic::new(why, 1, 1)]);
    }
    obs.enter(progress::Phase::Arc);
    program.owning_methods = arc::analyze(text, &program);
    program.heap_support = options.heap_support;
    program.introspection = options.introspection;
    program.reflection = options.reflection;
    obs.enter(progress::Phase::Generics);
    diagnostics.extend(generics::check_program(text, &program));
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    obs.enter(progress::Phase::Pools);
    let pools = resolve_pools(
        text,
        &program,
        &options.pool_sizes,
        options.item_pool_size,
        &mut diagnostics,
    );
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(FrontEnd { repaired, repaired_semicolons, program, pools })
}

/// `transpile` with everything a caller can supply.
pub fn transpile_with_options(
    source: &str,
    options: &Options,
) -> Result<TranspileOutput, Vec<Diagnostic>> {
    transpile_observed(source, options, &mut progress::Silent)
}

/// `transpile_with_options`, reporting each pass boundary to `obs`.
///
/// The observer is a separate argument rather than an `Options` field: a
/// `&mut dyn` field would force a lifetime parameter onto `Options`, which
/// derives `Default` and which nearly every test constructs literally.
pub fn transpile_observed(
    source: &str,
    options: &Options,
    obs: &mut dyn progress::Observer,
) -> Result<TranspileOutput, Vec<Diagnostic>> {
    let fe = front_end(source, options, obs)?;
    obs.enter(progress::Phase::Emit);
    let result = emit::emit(
        &fe.repaired,
        &fe.program,
        &fe.pools,
        &fe.repaired_semicolons,
        &line_directives(options),
    );
    if !result.diagnostics.is_empty() {
        return Err(result.diagnostics);
    }
    Ok(TranspileOutput {
        source_c: result.source_c,
        companion_h: result.companion_h,
        companion_c: result.companion_c,
    })
}

/// Origin-aware sibling of `transpile()` (OZ-096): same collect ->
/// emit pipeline and the same "any diagnostic is a hard error" rule,
/// but calls `emit::emit_split` instead of `emit::emit`, producing one
/// `.h`/`.c` pair per origin file instead of one combined `source_c`.
/// `origins` comes from `imports::ResolvedSource` -- only `main.rs` (or
/// any future filesystem-aware caller) has that; `transpile()` itself
/// stays the pure, single-string function every existing test uses.
///
/// The two are no longer separate implementations: since #254 both
/// assemble the output of one `emit::walk_top_level`, so they differ in
/// where text is placed and not in what text a node kind produces. This is
/// the one the CLI drives, and so the one every real build drives.
pub fn transpile_split(
    source: &str,
    origins: &[(String, std::ops::Range<usize>)],
) -> Result<emit::EmitSplitOutput, Vec<Diagnostic>> {
    transpile_split_with_options(source, origins, &Options::default())
}

/// `transpile_split` with explicit slab sizes for named classes.
pub fn transpile_split_with_pool_sizes(
    source: &str,
    origins: &[(String, std::ops::Range<usize>)],
    overrides: &PoolOverrides,
) -> Result<emit::EmitSplitOutput, Vec<Diagnostic>> {
    transpile_split_with_options(
        source,
        origins,
        &Options { pool_sizes: overrides.clone(), ..Default::default() },
    )
}

/// `transpile_split` with everything a caller can supply.
pub fn transpile_split_with_options(
    source: &str,
    origins: &[(String, std::ops::Range<usize>)],
    options: &Options,
) -> Result<emit::EmitSplitOutput, Vec<Diagnostic>> {
    transpile_split_observed(source, origins, options, &mut progress::Silent)
}

/// `transpile_split_with_options`, reporting each pass boundary to `obs`.
/// This is the one the CLI drives, and so the one a build's progress output
/// comes from.
pub fn transpile_split_observed(
    source: &str,
    origins: &[(String, std::ops::Range<usize>)],
    options: &Options,
    obs: &mut dyn progress::Observer,
) -> Result<emit::EmitSplitOutput, Vec<Diagnostic>> {
    let fe = front_end(source, options, obs)?;
    obs.enter(progress::Phase::Emit);
    let mut result = emit::emit_split(
        &fe.repaired,
        &fe.program,
        origins,
        &fe.pools,
        &options.header_ranges,
        &fe.repaired_semicolons,
        &line_directives(options),
    );
    let diagnostics = std::mem::take(&mut result.diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(result)
}

/// The `#line` policy these options describe -- a view over
/// `Options::source_map`/`generated_dirs`, borrowed for the one `emit`
/// call. Off (nothing emitted) when there is no map, which is every caller
/// that has no files behind its source. See `emit::LineDirectives`.
fn line_directives(options: &Options) -> emit::LineDirectives<'_> {
    emit::LineDirectives::new(options.source_map.as_ref(), Some(&options.generated_dirs))
}

/// Parse the supplied Clang AST, if any, onto the program.
///
/// A malformed dump is a hard error rather than a silent fall-back to the
/// narrower built-in rule: the caller asked for Clang's answer, and quietly
/// substituting a guess would change which ivars get released with no
/// indication why.
fn attach_ast(
    program: &mut Program,
    options: &Options,
    obs: &mut dyn progress::Observer,
) -> Result<(), String> {
    if options.ast_json.is_empty() && options.ast_paths.is_empty() {
        return Ok(());
    }
    let mut facts = astinfo::AstFacts::default();
    let mut index = 0usize;
    for text in &options.ast_json {
        facts.merge(astinfo::AstFacts::from_json(text)?);
        /* Reported after the merge, not before it: a complete line about
         * work that has finished is worth more than an announcement of work
         * in flight, and at ~0.6s per dump it still reads as live progress.
         * `index` is the caller's own position in its dump list, which is
         * how a report names the file without the library knowing paths. */
        obs.ast_dump(index, text.len());
        index += 1;
    }
    for path in &options.ast_paths {
        /* Read, parsed and dropped inside `from_path`, so the peak is one
         * dump rather than all of them. */
        let one = astinfo::AstFacts::from_path(path)?;
        let bytes = std::fs::metadata(path).map(|m| m.len() as usize).unwrap_or(0);
        facts.merge(one);
        obs.ast_dump(index, bytes);
        index += 1;
    }
    if facts.is_empty() {
        return Err(
            "the supplied Clang AST dumps describe no ivars at all -- they are probably not \
             dumps of this source (produce them with `clang -Xclang -ast-dump=json \
             -fsyntax-only -fobjc-arc`)"
                .to_string(),
        );
    }
    program.ast = Some(facts);
    Ok(())
}

/// Count allocation sites, apply any overrides, and reject an override
/// naming something that isn't a class in this program -- otherwise the
/// pool silently keeps its counted size and the author has no way to tell
/// the override never applied.
fn resolve_pools(
    source: &str,
    program: &Program,
    overrides: &PoolOverrides,
    item_pool_size: Option<usize>,
    diagnostics: &mut Vec<Diagnostic>,
) -> pools::PoolSizes {
    let mut sizes = pools::PoolSizes::analyze(source, program);
    sizes.set_overrides(overrides.clone());
    if let Some(slots) = item_pool_size {
        sizes.set_item_pool_override(slots);
    }
    for name in sizes.unknown_overrides(program) {
        diagnostics.push(Diagnostic::new(
            format!(
                "--pool-sizes names '{}', which is not a class in this source (nothing would \
                 use the override)",
                name
            ),
            1,
            1,
        ));
    }
    sizes
}
