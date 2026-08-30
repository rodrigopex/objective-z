// SPDX-License-Identifier: Apache-2.0
//
// lib.rs - OZ-091 Track B spike: static-subset Objective-C to C
// transpiler using in-place textual substitution.

pub mod astinfo;
pub mod collect;
pub mod companion;
pub mod emit;
pub mod generics;
pub mod imports;
pub mod model;
pub mod parse;
pub mod pools;
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
    /// A `clang -Xclang -ast-dump=json` dump of this same source, which is
    /// the only authority on which ivars are objects the class owns (see
    /// `astinfo`). Produce it with `-fobjc-arc`, or it carries no ownership
    /// information and oz_static falls back to its own narrower rule.
    pub ast_json: Option<String>,
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
        &Options { pool_sizes: overrides.clone(), ast_json: None },
    )
}

/// `transpile` with everything a caller can supply.
pub fn transpile_with_options(
    source: &str,
    options: &Options,
) -> Result<TranspileOutput, Vec<Diagnostic>> {
    let (mut program, mut diagnostics) = collect::collect(source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    if let Err(why) = attach_ast(&mut program, options) {
        return Err(vec![Diagnostic::new(why, 1, 1)]);
    }
    let overrides = &options.pool_sizes;
    diagnostics.extend(generics::check_program(source, &program));
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let pools = resolve_pools(source, &program, overrides, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let result = emit::emit(source, &program, &pools);
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
        &Options { pool_sizes: overrides.clone(), ast_json: None },
    )
}

/// `transpile_split` with everything a caller can supply.
pub fn transpile_split_with_options(
    source: &str,
    origins: &[(String, std::ops::Range<usize>)],
    options: &Options,
) -> Result<emit::EmitSplitOutput, Vec<Diagnostic>> {
    let (mut program, mut diagnostics) = collect::collect(source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    if let Err(why) = attach_ast(&mut program, options) {
        return Err(vec![Diagnostic::new(why, 1, 1)]);
    }
    let overrides = &options.pool_sizes;
    diagnostics.extend(generics::check_program(source, &program));
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let pools = resolve_pools(source, &program, overrides, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let mut result = emit::emit_split(source, &program, origins, &pools);
    diagnostics.extend(std::mem::take(&mut result.diagnostics));
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(result)
}

/// Parse the supplied Clang AST, if any, onto the program.
///
/// A malformed dump is a hard error rather than a silent fall-back to the
/// narrower built-in rule: the caller asked for Clang's answer, and quietly
/// substituting a guess would change which ivars get released with no
/// indication why.
fn attach_ast(program: &mut Program, options: &Options) -> Result<(), String> {
    let Some(text) = &options.ast_json else {
        return Ok(());
    };
    let facts = astinfo::AstFacts::from_json(text)?;
    if facts.is_empty() {
        return Err(
            "the supplied Clang AST describes no ivars at all -- it is probably not a dump of \
             this source (produce it with `clang -Xclang -ast-dump=json -fsyntax-only \
             -fobjc-arc`)"
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
    diagnostics: &mut Vec<Diagnostic>,
) -> pools::PoolSizes {
    let mut sizes = pools::PoolSizes::analyze(source, program);
    sizes.set_overrides(overrides.clone());
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
