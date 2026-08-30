// SPDX-License-Identifier: Apache-2.0
//
// lib.rs - OZ-091 Track B spike: static-subset Objective-C to C
// transpiler using in-place textual substitution.

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

/// Full pipeline: parse -> collect -> emit. Returns Ok on success, or the
/// full list of static-bar/emission diagnostics on failure. Never
/// silently degrades: anything the static subset doesn't accept is a
/// named, located hard error.
pub fn transpile(source: &str) -> Result<TranspileOutput, Vec<Diagnostic>> {
    transpile_with_pool_sizes(source, &PoolOverrides::new())
}

/// `transpile` with explicit slab sizes for named classes. Split out
/// rather than folded into `transpile` so the pure, argument-free form
/// stays what the test suite calls.
pub fn transpile_with_pool_sizes(
    source: &str,
    overrides: &PoolOverrides,
) -> Result<TranspileOutput, Vec<Diagnostic>> {
    let (program, mut diagnostics) = collect::collect(source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
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
    transpile_split_with_pool_sizes(source, origins, &PoolOverrides::new())
}

/// `transpile_split` with explicit slab sizes for named classes.
pub fn transpile_split_with_pool_sizes(
    source: &str,
    origins: &[(String, std::ops::Range<usize>)],
    overrides: &PoolOverrides,
) -> Result<emit::EmitSplitOutput, Vec<Diagnostic>> {
    let (program, mut diagnostics) = collect::collect(source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
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
